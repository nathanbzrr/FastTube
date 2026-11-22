use super::super::*;
use super::helpers::*;

#[test]
fn wave1_batch2_async_persistence_coalesces_and_preserves_corrupt_files() {
    let dir = std::env::temp_dir().join(format!("fasttube-w1b2-persist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let hist_path = dir.join("history.json");
    let lib_path = dir.join("library.json");
    let queue_path = dir.join("queue.json");

    let videos = valid_youtube_videos();
    let mut app = FastTube::default();
    app.history.path = Some(hist_path.clone());
    app.history.loaded_path = Some(hist_path.clone());
    app.history.read_failed = false;
    app.library.path = Some(lib_path.clone());
    app.library.read_failed = false;
    app.queue.path = Some(queue_path.clone());
    app.queue.read_failed = false;

    // Rapid mutations coalesce through the async PersistenceWorker without blocking.
    let now = Instant::now();
    for step in 1..=10u64 {
        app.record_video_in_history(&videos[0], step * 2_000, Some(200_000))
            .unwrap();
        assert!(app.save_history_to_disk(now + Duration::from_secs(step * 25)));
    }
    app.toggle_collection_video("Study", &videos[0]);
    assert!(app.enqueue_video(videos[1].clone()).is_ok());
    app.flush_persistence();

    assert_eq!(
        history::load(&hist_path)
            .unwrap()
            .get("dQw4w9WgXcQ")
            .unwrap()
            .position_ms,
        20_000
    );
    assert_eq!(library::load(&lib_path).unwrap().collections.len(), 1);
    assert_eq!(
        Queue::load(&queue_path, DEFAULT_QUEUE_LIMIT).unwrap().len(),
        1
    );
    assert!(app.history.loaded_fingerprint.is_some());
    assert!(app.library.loaded_fingerprint.is_some());
    assert!(app.queue.loaded_fingerprint.is_some());

    // Simulate external corruption on disk: (mtime, len) detects the external edit and
    // refuses to overwrite any of the three corrupt files.
    std::fs::write(&hist_path, b"{corrupt-history").unwrap();
    std::fs::write(&lib_path, b"{corrupt-library").unwrap();
    std::fs::write(&queue_path, b"{corrupt-queue").unwrap();

    app.record_video_in_history(&videos[0], 99_000, Some(200_000))
        .unwrap();
    assert!(!app.save_history_to_disk(now + Duration::from_secs(500)));
    app.toggle_collection_video("Study", &videos[1]);
    app.persist_queue();
    app.flush_persistence();

    assert!(app.history.read_failed);
    assert!(app.library.read_failed);
    assert!(app.queue.read_failed);
    assert_eq!(std::fs::read(&hist_path).unwrap(), b"{corrupt-history");
    assert_eq!(std::fs::read(&lib_path).unwrap(), b"{corrupt-library");
    assert_eq!(std::fs::read(&queue_path).unwrap(), b"{corrupt-queue");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wave1_batch2_idle_repaint_has_zero_timers_and_uses_exact_toast_deadline() {
    // Completely idle or only async jobs running -> zero polling timers.
    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput::default()),
        None
    );
    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            async_jobs_active: true,
            ..Default::default()
        }),
        None
    );

    // Picker channels must not count as periodic repaint work.
    let mut app = FastTube::default();
    let (_req_id, _cancel, _tx) = app.settings.cache_picker_task.begin_manual();
    assert!(!app.has_active_async_jobs());

    // ToastManager schedules zero repaints when empty and exact fade-out deadline when steady.
    let mut toasts = toast::ToastManager::default();
    let t0 = Instant::now();
    assert_eq!(toasts.next_repaint_delay(t0), None);
    toasts.push_at(toast::ToastKind::Info, "Saved", Duration::from_secs(4), t0);
    // Mid-lifecycle (steady state at t0 + 500ms): fade-out starts at 4000 - 350 = 3650ms -> ~3150ms remaining.
    let steady_delay = toasts
        .next_repaint_delay(t0 + Duration::from_millis(500))
        .expect("active toast should schedule exact deadline");
    assert!(
        steady_delay >= Duration::from_secs(3),
        "expected exact deadline (~3.15s), got {steady_delay:?}"
    );
    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            next_toast_repaint: Some(steady_delay),
            ..Default::default()
        }),
        Some(steady_delay)
    );
    // Minimized window suppresses all repaints even with an active toast.
    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            minimized: true,
            next_toast_repaint: Some(steady_delay),
            ..Default::default()
        }),
        None
    );
}

#[test]
fn wave1_batch2_sync_now_playing_deduplicates_unchanged_smtc_updates() {
    let ctx = egui::Context::default();
    let mut desktop = desktop::DesktopIntegrations::default();

    let base = desktop::NowPlayingSnapshot {
        video_id: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        title: "Track A".into(),
        channel: "Artist A".into(),
        thumbnail_url: String::new(),
        playing: true,
        ended: false,
        position_secs: 12.1,
        duration_secs: Some(180.0),
        volume_fraction: Some(0.8),
        can_next: true,
        can_previous: true,
    };

    desktop.sync_now_playing(Some(base.clone()), &ctx);
    assert_eq!(desktop.now_playing_push_count(), 1);

    // Sub-second timeline drift within the same 1s bucket must be deduplicated.
    let mut same_bucket = base.clone();
    same_bucket.position_secs = 12.8;
    desktop.sync_now_playing(Some(same_bucket), &ctx);
    assert_eq!(desktop.now_playing_push_count(), 1);

    // Crossing into the next second bucket or toggling play/pause pushes an update.
    let mut next_bucket = base.clone();
    next_bucket.position_secs = 13.05;
    desktop.sync_now_playing(Some(next_bucket.clone()), &ctx);
    assert_eq!(desktop.now_playing_push_count(), 2);

    let mut paused = next_bucket;
    paused.playing = false;
    desktop.sync_now_playing(Some(paused), &ctx);
    assert_eq!(desktop.now_playing_push_count(), 3);

    // Clearing to None pushes once and deduplicates subsequent None calls.
    desktop.sync_now_playing(None, &ctx);
    desktop.sync_now_playing(None, &ctx);
    assert_eq!(desktop.now_playing_push_count(), 4);
}

#[test]
fn wave1_batch2_clipboard_polling_gates_on_focus_and_sequence_number() {
    let mut app = FastTube::default();
    let mut reads = 0usize;

    // Unfocused window must never read the OS clipboard.
    assert!(!app.poll_clipboard_with_state(false, 42, || {
        reads += 1;
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".into())
    }));
    assert_eq!(reads, 0);
    assert!(app.ui.clipboard_youtube_target.is_none());

    // Focused window with a new sequence number reads once and parses target.
    assert!(app.poll_clipboard_with_state(true, 42, || {
        reads += 1;
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".into())
    }));
    assert_eq!(reads, 1);
    assert!(app.ui.clipboard_youtube_target.is_some());

    // Same sequence number while focused skips OS clipboard read completely.
    assert!(!app.poll_clipboard_with_state(true, 42, || {
        reads += 1;
        Some("https://www.youtube.com/watch?v=9bZkp7q19f0".into())
    }));
    assert_eq!(reads, 1);

    // Changed sequence number triggers a fresh read.
    assert!(app.poll_clipboard_with_state(true, 43, || {
        reads += 1;
        Some("https://www.youtube.com/watch?v=9bZkp7q19f0".into())
    }));
    assert_eq!(reads, 2);
}

#[test]
fn wave1_batch2_command_palette_cache_and_search_suggest_64_prefix_lru() {
    let mut app = FastTube::default();
    app.ui.command_palette.open_palette();
    app.ui.command_palette.query = "tema".into();

    app.ensure_command_palette_cache();
    let builds_after_first = app.ui.command_palette_rebuilds;
    assert_eq!(builds_after_first, 1);
    assert!(!app.ui.command_palette_cached_items.is_empty());

    // Repeated frames with identical query and state must hit cache with zero rebuilds.
    app.ensure_command_palette_cache();
    app.ensure_command_palette_cache();
    assert_eq!(app.ui.command_palette_rebuilds, 1);

    // Mutating query or library revision invalidates the palette cache.
    app.ui.command_palette.query = "fila".into();
    app.ensure_command_palette_cache();
    assert_eq!(app.ui.command_palette_rebuilds, 2);
    app.library.revision = app.library.revision.wrapping_add(1);
    app.ensure_command_palette_cache();
    assert_eq!(app.ui.command_palette_rebuilds, 3);

    // 64-prefix bounded LRU for remote search suggestions + cached merge_search_suggestions.
    let mut suggest = app_state::SearchSuggestState::default();
    for idx in 0..70usize {
        suggest.insert_cached_prefix(&format!("prefix-{idx:02}"), vec![format!("result-{idx}")]);
    }
    assert_eq!(
        suggest.prefix_cache.len(),
        app_state::MAX_SEARCH_SUGGEST_PREFIX_CACHE
    );
    assert!(suggest.get_cached_prefix("prefix-00").is_none());
    assert_eq!(
        suggest.get_cached_prefix("prefix-69"),
        Some(vec!["result-69".to_owned()])
    );

    suggest.set_remote_suggestions(vec!["rust gui".into(), "rust audio".into()]);
    let recent = vec!["rust egui".to_owned()];
    let first_merged = suggest.merged_suggestions("rust", &recent, 8).to_vec();
    let cached_key = suggest.merged_cache_key.clone();
    let second_merged = suggest.merged_suggestions("rust", &recent, 8).to_vec();
    assert_eq!(first_merged, second_merged);
    assert_eq!(suggest.merged_cache_key, cached_key);
}

#[test]
fn wave1_batch2_community_extras_updates_separate_revision_without_invalidating_feed() {
    let mut app = FastTube::default();
    let initial_meta_rev = app.feed.metadata_revision;
    let initial_comm_rev = app.watch.community_revision;

    let (tx, rx) = mpsc::channel();
    app.watch
        .community_task
        .attach_receiver(7, process::Cancellation::default(), rx);
    tx.send((
        7,
        app_state::CommunityExtrasFetchMessage {
            request_id: 7,
            outcome: community_apis::CommunityExtrasOutcome {
                video_id: "dQw4w9WgXcQ".into(),
                ryd: None,
                dearrow: None,
            },
        },
    ))
    .unwrap();

    app.poll_community_extras();
    assert_eq!(
        app.feed.metadata_revision, initial_meta_rev,
        "RYD/DeArrow updates must not bump feed.metadata_revision"
    );
    assert_eq!(app.watch.community_revision, initial_comm_rev + 1);
}

#[test]
fn wave1_batch2_bounded_lru_caps_enforce_all_memory_limits() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();

    // 1. home.cached_items capped at MAX_HOME_CACHED_ITEMS (300)
    let (req_id, _cancel, tx) = app.feed.home.batch_task.begin_manual();
    let oversized_items: Vec<SearchResult> = (0..360)
        .map(|i| SearchResult {
            video: Video {
                title: format!("Home {i}"),
                channel: format!("Chan {i}"),
                url: format!("https://www.youtube.com/watch?v={i:011}"),
                thumbnail: String::new(),
                duration: "4:00".into(),
            },
            metadata: VideoMetadata::default(),
        })
        .collect();
    tx.send((
        req_id,
        HomeBatchFetchMessage {
            request_id: req_id,
            page: 0,
            result: Ok(search::HomeFeed {
                items: oversized_items,
                playlists: Vec::new(),
                channels: Vec::new(),
            }),
        },
    ))
    .unwrap();
    app.poll_home_batch(&ctx);
    assert_eq!(app.feed.home.cached_items.len(), MAX_HOME_CACHED_ITEMS);

    // 2. video_metadata hard cap (MAX_VIDEO_METADATA_ENTRIES = 1500) preserving selected video
    let selected_url = "https://www.youtube.com/watch?v=SelActive01".to_owned();
    app.selected = Some(Video {
        title: "Selected".into(),
        channel: "SelectedChannel".into(),
        url: selected_url.clone(),
        thumbnail: String::new(),
        duration: "5:00".into(),
    });
    app.feed.videos.clear();
    app.video_metadata.clear();
    app.video_metadata
        .insert(selected_url.clone(), VideoMetadata::default());
    for i in 0..1_650 {
        let url = format!("https://www.youtube.com/watch?v=Vid{i:08}");
        app.feed.videos.push(Video {
            title: format!("V{i}"),
            channel: format!("Channel{i}"),
            url: url.clone(),
            thumbnail: String::new(),
            duration: "2:00".into(),
        });
        app.video_metadata.insert(url, VideoMetadata::default());
        app.feed
            .home
            .channel_avatars
            .insert(format!("channel{i}"), format!("https://img/{i}.jpg"));
    }
    app.prune_session_caches();
    assert!(app.video_metadata.len() <= MAX_VIDEO_METADATA_ENTRIES);
    assert!(app.video_metadata.contains_key(&selected_url));

    // 3. channel_avatars hard cap (MAX_CHANNEL_AVATARS_ENTRIES = 500)
    assert!(app.feed.home.channel_avatars.len() <= MAX_CHANNEL_AVATARS_ENTRIES);

    // 4. history.remote.feeds LRU cap (MAX_REMOTE_HISTORY_CACHED_FEEDS = 3)
    for (idx, filter) in [
        history::HistoryFilter::All,
        history::HistoryFilter::Videos,
        history::HistoryFilter::Shorts,
        history::HistoryFilter::Podcasts,
        history::HistoryFilter::Music,
    ]
    .into_iter()
    .enumerate()
    {
        app.apply_remote_history_page(
            filter,
            0,
            history_sync::RemoteHistoryPage {
                sections: vec![history_sync::RemoteHistorySection {
                    date_label: "Hoje".into(),
                    items: vec![history_sync::RemoteHistoryItem {
                        video_id: format!("Hist{idx:07}"),
                        title: format!("History {idx}"),
                        channel: "Ch".into(),
                        ..Default::default()
                    }],
                }],
                ..Default::default()
            },
        );
    }
    assert!(
        app.history.remote.feeds.len() <= history_sync::MAX_REMOTE_HISTORY_CACHED_FEEDS,
        "remote history feeds must obey LRU bound, got {}",
        app.history.remote.feeds.len()
    );

    // 5. watch.extras.comments capped at MAX_WATCH_COMMENTS (500)
    let mut extras = watch_extras::WatchExtras::default();
    let (c_tx, c_rx) = mpsc::channel();
    extras.comments_loading = Some(c_rx);
    let page_comments: Vec<oauth::Comment> = (0..620)
        .map(|i| oauth::Comment {
            id: format!("c{i}"),
            author: format!("User {i}"),
            author_avatar: None,
            text: format!("Comment {i}"),
            published: "2026-01-01".into(),
            likes: 0,
            replies: 0,
        })
        .collect();
    c_tx.send(Ok(oauth::CommentsPage {
        comments: page_comments,
        next_page: Some("next_token".into()),
        total_results: Some(1_000),
    }))
    .unwrap();
    extras.poll();
    assert_eq!(extras.comments.len(), watch_extras::MAX_WATCH_COMMENTS);
    assert!(extras.comments_next.is_none());
}

#[test]
fn wave1_batch2_warm_player_park_state_and_codec_format_preferences() {
    // Verify warm player lifecycle constants and codec-aware ytdl-format generation.
    #[cfg(windows)]
    {
        assert_eq!(
            player::WARM_PLAYER_IDLE_TIMEOUT,
            Duration::from_secs(5 * 60)
        );
        let vp9 =
            player::ytdl_format_for_codec_pref(Some(1080), settings::VideoCodecPreference::Vp9);
        assert!(vp9.contains("vcodec^=vp9") && vp9.contains("height<=1080"));
        let h264 =
            player::ytdl_format_for_codec_pref(Some(720), settings::VideoCodecPreference::H264);
        assert!(h264.contains("vcodec^=avc1") && h264.contains("height<=720"));
        let av1 =
            player::ytdl_format_for_codec_pref(Some(2160), settings::VideoCodecPreference::Av1);
        assert!(av1.contains("vcodec^=av01") && av1.contains("height<=2160"));
    }

    // Verify close_video cleanly resets watch selection while preserving warm lifecycle fields.
    let ctx = egui::Context::default();
    let mut app = FastTube {
        selected: Some(valid_youtube_videos()[0].clone()),
        ..FastTube::default()
    };
    app.playback.active_buffer = Some(BufferProfile::Large);
    app.playback.active_hwdec = Some(settings::HwdecMode::AutoSafe);
    app.close_video(&ctx);
    assert!(app.selected.is_none());
    #[cfg(windows)]
    assert!(app.playback.frame_snapshot.is_none());
    // When no libmpv Player instance exists, active_buffer/active_hwdec reset to None.
    assert!(app.playback.active_buffer.is_none());
    assert!(app.playback.active_hwdec.is_none());

    // Applying a new video codec preference updates AppPreferences cleanly.
    let mut next = app.settings.prefs.value.clone();
    next.video_codec_preference = settings::VideoCodecPreference::H264;
    app.apply_app_preferences(next);
    assert_eq!(
        app.settings.prefs.value.video_codec_preference,
        settings::VideoCodecPreference::H264
    );
}

#[test]
fn wave2_home_swr_hydration_and_innertube_chips_and_continuation() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.settings.prefs.value.language = i18n::UiLanguage::PtBr;

    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-wave2-home-swr-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    let cache_path = temp_dir.join("home-cache.json");
    let initial_page = innertube::InnerTubeHomePage {
        items: vec![
            Video {
                title: "Algoritmo Home 1".into(),
                channel: "Canal Alfa".into(),
                duration: "10:15".into(),
                url: "https://www.youtube.com/watch?v=home0000001".into(),
                thumbnail: "https://i.ytimg.com/vi/home0000001/hqdefault.jpg".into(),
            },
            Video {
                title: "Algoritmo Home 2".into(),
                channel: "Canal Beta".into(),
                duration: "4:20".into(),
                url: "https://www.youtube.com/watch?v=home0000002".into(),
                thumbnail: "https://i.ytimg.com/vi/home0000002/hqdefault.jpg".into(),
            },
        ],
        metadata: vec![
            VideoMetadata {
                channel_verified: true,
                ..Default::default()
            },
            VideoMetadata::default(),
        ],
        chips: vec![
            innertube::HomeFeedChip {
                title: "Tudo".into(),
                token: None,
                selected: true,
            },
            innertube::HomeFeedChip {
                title: "Música".into(),
                token: Some("chip_music_tok".into()),
                selected: false,
            },
        ],
        continuation: Some("home_cont_page_2".into()),
        visitor_data: Some("CgtTestVisitor".into()),
    };
    let swr =
        innertube::HomeSwrCache::from_page(&initial_page, i18n::UiLanguage::PtBr, 1_750_000_000);
    swr.save_to(&cache_path).expect("save home SWR cache");

    assert!(app.hydrate_home_from_swr_cache_at(&cache_path, &ctx));
    assert_eq!(app.feed.videos.len(), 2);
    assert_eq!(app.feed.home_chips.len(), 2);
    assert_eq!(
        app.feed.home_continuation.as_deref(),
        Some("home_cont_page_2")
    );
    assert!(app.feed.home.cached_loaded);
    assert!(
        app.video_metadata
            .get("https://www.youtube.com/watch?v=home0000001")
            .is_some_and(|m| m.channel_verified)
    );

    // Language mismatch does not hydrate stale locale cache.
    let mut en_app = FastTube::default();
    en_app.settings.prefs.value.language = i18n::UiLanguage::En;
    assert!(!en_app.hydrate_home_from_swr_cache_at(&cache_path, &ctx));

    // Continuation append deduplicates existing items and advances continuation token.
    let page2 = innertube::InnerTubeHomePage {
        items: vec![
            Video {
                title: "Duplicado".into(),
                channel: "Canal Alfa".into(),
                duration: "10:15".into(),
                url: "https://www.youtube.com/watch?v=home0000001".into(),
                thumbnail: String::new(),
            },
            Video {
                title: "Algoritmo Home 3".into(),
                channel: "Canal Gama".into(),
                duration: "8:00".into(),
                url: "https://www.youtube.com/watch?v=home0000003".into(),
                thumbnail: String::new(),
            },
        ],
        metadata: vec![VideoMetadata::default(), VideoMetadata::default()],
        chips: Vec::new(),
        continuation: Some("home_cont_page_3".into()),
        visitor_data: None,
    };
    app.apply_innertube_home_page(page2, true, false, &ctx);
    assert_eq!(app.feed.videos.len(), 3);
    assert_eq!(app.feed.videos[2].title, "Algoritmo Home 3");
    assert_eq!(
        app.feed.home_continuation.as_deref(),
        Some("home_cont_page_3")
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn wave2_search_pagination_spelling_and_filters_modal_escape_sync() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.ui.search_results_mode = true;
    app.feed.query = "rust lang".into();

    let (_req, query) = app.begin_search_request().expect("begin search");
    assert_eq!(query, "rust lang");
    app.feed.search_page_task.reserve_id_for_test(1);

    // Stale request id is ignored.
    assert!(!app.complete_search_page_request(
        99,
        false,
        Ok(search::SearchPageOutput::default()),
        &ctx
    ));

    // Page 0 populates spelling correction, continuation token, and videos.
    let page0 = search::SearchPageOutput {
        videos: vec![Video {
            title: "Rust Tutorial 1".into(),
            channel: "Rustacean".into(),
            duration: "12:00".into(),
            url: "https://www.youtube.com/watch?v=srch0000001".into(),
            thumbnail: String::new(),
        }],
        metadata: vec![VideoMetadata {
            channel_verified: true,
            ..Default::default()
        }],
        continuation: Some("search_cont_2".into()),
        spelling: Some(innertube::SearchSpellingCorrection {
            corrected_query: "rust programming language".into(),
            original_query: Some("rust lang".into()),
            is_auto_corrected: true,
        }),
        estimated_results: Some(42_000),
    };
    assert!(app.complete_search_page_request(1, false, Ok(page0), &ctx));
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(
        app.feed.search_continuation.as_deref(),
        Some("search_cont_2")
    );
    assert_eq!(
        app.feed
            .search_spelling
            .as_ref()
            .map(|s| s.corrected_query.as_str()),
        Some("rust programming language")
    );

    // Page 1 appends deduplicated results without clearing page 0 or spelling banner.
    app.feed.search_page_task.reserve_id_for_test(2);
    app.feed.search_loading_more = true;
    let page1 = search::SearchPageOutput {
        videos: vec![
            Video {
                title: "Duplicate".into(),
                channel: "Rustacean".into(),
                duration: "12:00".into(),
                url: "https://www.youtube.com/watch?v=srch0000001".into(),
                thumbnail: String::new(),
            },
            Video {
                title: "Rust Tutorial 2".into(),
                channel: "Rustacean".into(),
                duration: "15:00".into(),
                url: "https://www.youtube.com/watch?v=srch0000002".into(),
                thumbnail: String::new(),
            },
        ],
        metadata: vec![VideoMetadata::default(), VideoMetadata::default()],
        continuation: None,
        spelling: None,
        estimated_results: None,
    };
    assert!(app.complete_search_page_request(2, true, Ok(page1), &ctx));
    assert!(!app.feed.search_loading_more);
    assert_eq!(app.feed.videos.len(), 2);
    assert!(app.feed.search_continuation.is_none());
    assert!(app.feed.search_spelling.is_some());

    // Multi-select filters modal participates in modal guard.
    assert!(!app.has_modal_covering_mini_player());
    app.feed.search_filters_modal_open = true;
    assert!(app.has_modal_covering_mini_player());
    app.feed.search_filters_modal_open = false;
    assert!(!app.has_modal_covering_mini_player());
}

#[test]
fn wave2_channel_dynamic_tabs_sort_chips_and_uncapped_continuation() {
    let mut app = FastTube::default();
    let target = ChannelTarget::ChannelId("UC1234567890123456789012".into());
    let initial_videos: Vec<SearchResult> = (0..95)
        .map(|i| SearchResult {
            video: Video {
                title: format!("Channel Video {i}"),
                channel: "Canal Grande".into(),
                duration: "5:00".into(),
                url: format!("https://www.youtube.com/watch?v=chv{i:08}"),
                thumbnail: String::new(),
            },
            metadata: VideoMetadata::default(),
        })
        .collect();
    app.channels.page = Some(ChannelPage {
        target: target.clone(),
        channel_id: Some("UC1234567890123456789012".into()),
        title: Some("Canal Grande".into()),
        videos: initial_videos,
        next_continuation: Some("ch_cont_2".into()),
        sort_chips: vec![
            innertube::ChannelSortChip {
                title: "Mais recentes".into(),
                continuation: "sort_latest".into(),
                selected: true,
            },
            innertube::ChannelSortChip {
                title: "Populares".into(),
                continuation: "sort_popular".into(),
                selected: false,
            },
        ],
        tab_endpoints: vec![
            innertube::ChannelTabEndpoint {
                title: "Vídeos".into(),
                browse_id: "UC1234567890123456789012".into(),
                params: Some("EgZ2aWRlb3M".into()),
                selected: true,
                kind: innertube::ChannelTabKind::Videos,
            },
            innertube::ChannelTabEndpoint {
                title: "Shorts".into(),
                browse_id: "UC1234567890123456789012".into(),
                params: Some("EgZzaG9ydHM".into()),
                selected: false,
                kind: innertube::ChannelTabKind::Shorts,
            },
        ],
        ..ChannelPage::default()
    });

    // Appending 20 videos crosses 100 without enforcing a 100-video cap.
    app.channels
        .videos
        .continuation_task
        .reserve_id_for_test(10);
    let more_videos: Vec<SearchResult> = (95..115)
        .map(|i| SearchResult {
            video: Video {
                title: format!("Channel Video {i}"),
                channel: "Canal Grande".into(),
                duration: "6:00".into(),
                url: format!("https://www.youtube.com/watch?v=chv{i:08}"),
                thumbnail: String::new(),
            },
            metadata: VideoMetadata::default(),
        })
        .collect();
    assert!(
        app.complete_channel_continuation(app_state::ChannelContinuationFetchMessage {
            request_id: 10,
            target: target.clone(),
            mode: app_state::ChannelContinuationTarget::VideosAppend,
            result: Ok(search::ChannelTabPage {
                videos: more_videos,
                sort_chips: Vec::new(),
                next_continuation: Some("ch_cont_3".into()),
                tab_endpoints: Vec::new(),
                community_posts: Vec::new(),
            }),
        })
    );
    let page = app.channels.page.as_ref().unwrap();
    assert_eq!(page.videos.len(), 115);
    assert!(!app.channels.videos.reached_max);
    assert!(app.channels.videos.has_more);
    assert_eq!(page.next_continuation.as_deref(), Some("ch_cont_3"));

    // Dynamic Shorts tab populates channels.shorts cleanly.
    app.channels
        .videos
        .continuation_task
        .reserve_id_for_test(11);
    assert!(
        app.complete_channel_continuation(app_state::ChannelContinuationFetchMessage {
            request_id: 11,
            target,
            mode: app_state::ChannelContinuationTarget::DynamicTab(
                innertube::ChannelTabKind::Shorts
            ),
            result: Ok(search::ChannelTabPage {
                videos: vec![SearchResult {
                    video: Video {
                        title: "Channel Short 1".into(),
                        channel: "Canal Grande".into(),
                        duration: "0:45".into(),
                        url: "https://www.youtube.com/shorts/shrt0000001".into(),
                        thumbnail: String::new(),
                    },
                    metadata: VideoMetadata {
                        kind: search::SearchResultKind::Short,
                        ..Default::default()
                    },
                }],
                sort_chips: Vec::new(),
                next_continuation: None,
                tab_endpoints: Vec::new(),
                community_posts: Vec::new(),
            }),
        })
    );
    assert!(app.channels.shorts.loaded);
    assert_eq!(app.channels.shorts.items.len(), 1);
}

#[test]
fn wave2_watch_next_heatmap_verified_and_oauth_free_comments() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
    let video = Video {
        title: "Watch Next Video".into(),
        channel: "Verified Creator".into(),
        duration: "10:00".into(),
        url: "https://www.youtube.com/watch?v=wnxt0000001".into(),
        thumbnail: String::new(),
    };
    app.selected = Some(video.clone());
    app.watch.extras.sync_video(Some(&video.url));
    app.watch.next_task.reserve_id_for_test(7);

    let next = innertube::InnerTubeWatchNext {
        related_videos: vec![Video {
            title: "Related Video 1".into(),
            channel: "Related Ch".into(),
            duration: "7:30".into(),
            url: "https://www.youtube.com/watch?v=relv0000001".into(),
            thumbnail: String::new(),
        }],
        related_metadata: vec![VideoMetadata {
            channel_verified: true,
            ..Default::default()
        }],
        related_chips: vec![
            innertube::RelatedFilterChip {
                title: "Todos".into(),
                continuation: None,
                selected: true,
            },
            innertube::RelatedFilterChip {
                title: "Do mesmo canal".into(),
                continuation: Some("rel_chip_tok".into()),
                selected: false,
            },
        ],
        related_continuation: Some("rel_next_tok".into()),
        comments_continuation: Some("pub_comments_tok".into()),
        heatmap: vec![
            innertube::HeatmapBucket {
                start_ms: 0,
                duration_ms: 5_000,
                intensity: 0.25,
            },
            innertube::HeatmapBucket {
                start_ms: 5_000,
                duration_ms: 5_000,
                intensity: 0.95,
            },
        ],
        channel_id: Some("UCverified00000000000001".into()),
        channel_verified: true,
        like_count_text: Some("12 mil".into()),
        subscriber_count_text: Some("500 mil inscritos".into()),
        description_runs: Vec::new(),
        ..Default::default()
    };

    assert!(app.complete_watch_next(
        app_state::WatchNextFetchMessage {
            request_id: 7,
            video_url: video.url.clone(),
            chip_index: None,
            is_chip_reload: false,
            result: Ok(next),
        },
        &ctx
    ));
    assert_eq!(app.watch.extras.related_videos.len(), 1);
    assert_eq!(app.watch.extras.related_chips.len(), 2);
    assert_eq!(app.watch.extras.heatmap.len(), 2);
    assert!(
        app.selected_metadata
            .as_ref()
            .is_some_and(|m| m.channel_verified)
    );
    assert_eq!(
        app.watch.extras.innertube_comments_continuation.as_deref(),
        Some("pub_comments_tok")
    );

    // OAuth-free comments page delivery works with auth.session == None.
    assert!(app.auth.session.is_none());
    app.watch.innertube_comments_task.reserve_id_for_test(3);
    assert!(
        app.complete_innertube_comments(app_state::InnerTubeCommentsFetchMessage {
            request_id: 3,
            video_url: video.url.clone(),
            append: false,
            result: Ok(innertube::InnerTubeCommentsPage {
                comments: vec![innertube::InnerTubeComment {
                    id: "c_pub_1".into(),
                    author: "@public_user".into(),
                    author_avatar: None,
                    text: "Excelente explicação!".into(),
                    published_text: "há 2 horas".into(),
                    like_count_text: Some("42".into()),
                    is_pinned: true,
                    is_hearted: false,
                    is_owner: false,
                    is_member: false,
                    reply_count: 1,
                    replies_continuation: None,
                }],
                continuation: Some("pub_comments_p2".into()),
                top_sort_continuation: None,
                newest_sort_continuation: None,
                total_count_text: Some("100".into()),
            }),
        })
    );
    assert!(app.watch.extras.comments_loaded);
    assert_eq!(app.watch.extras.comments.len(), 1);
    assert_eq!(
        app.watch.extras.innertube_comments_continuation.as_deref(),
        Some("pub_comments_p2")
    );

    // WatchLiveInfo also propagates channel_verified.
    let video2 = Video {
        title: "Live Verified".into(),
        channel: "Live Ch".into(),
        duration: "AO VIVO".into(),
        url: "https://www.youtube.com/watch?v=live0000001".into(),
        thumbnail: String::new(),
    };
    app.selected = Some(video2.clone());
    app.selected_metadata = None;
    app.watch.live.video_url = Some(video2.url.clone());
    app.watch.live.session_id = 55;
    app.handle_watch_live_info_result(
        55,
        Ok(live_chat::WatchLiveInfo {
            channel_verified: true,
            ..Default::default()
        }),
        &ctx,
    );
    assert!(
        app.video_metadata
            .get(&video2.url)
            .is_some_and(|m| m.channel_verified)
    );
    assert!(
        app.selected_metadata
            .as_ref()
            .is_some_and(|m| m.channel_verified)
    );
}

#[test]
fn wave2_stream_manifest_hydration_403_invalidation_and_speculative_prefetch() {
    let mut app = FastTube::default();
    let now = Instant::now();
    let video = Video {
        title: "Direct Stream Video".into(),
        channel: "Tech Channel".into(),
        duration: "10:00".into(),
        url: "https://www.youtube.com/watch?v=dstr0000001".into(),
        thumbnail: String::new(),
    };
    app.selected = Some(video.clone());
    app.watch.extras.sync_video(Some(&video.url));

    let manifest = stream_manifest::StreamManifest {
        video_id: "dstr0000001".into(),
        title: "Direct Stream Video".into(),
        channel: "Tech Channel".into(),
        channel_id: Some("UCtech000000000000000001".into()),
        channel_verified: true,
        duration_secs: Some(600.0),
        is_live: false,
        upload_date: None,
        video_formats: vec![
            stream_manifest::VideoStreamFormat {
                format_id: "248".into(),
                url: "https://rr1---sn.googlevideo.com/videoplayback?itag=248&expire=4102444800"
                    .into(),
                height: 1080,
                width: Some(1920),
                fps: Some(60.0),
                vcodec: "vp9".into(),
                is_hdr: false,
                tbr_kbps: Some(3500.0),
                ext: "webm".into(),
                has_audio: false,
                http_headers: HashMap::new(),
            },
            stream_manifest::VideoStreamFormat {
                format_id: "247".into(),
                url: "https://rr1---sn.googlevideo.com/videoplayback?itag=247&expire=4102444800"
                    .into(),
                height: 720,
                width: Some(1280),
                fps: Some(30.0),
                vcodec: "vp9".into(),
                is_hdr: false,
                tbr_kbps: Some(1500.0),
                ext: "webm".into(),
                has_audio: false,
                http_headers: HashMap::new(),
            },
        ],
        audio_formats: vec![
            stream_manifest::AudioStreamFormat {
                format_id: "251-en".into(),
                url: "https://rr1---sn.googlevideo.com/videoplayback?itag=251&lang=en".into(),
                acodec: "opus".into(),
                abr_kbps: Some(160.0),
                ext: "webm".into(),
                language: Some("en".into()),
                language_label: "English (original)".into(),
                is_default: true,
                is_dubbed: false,
                format_note: Some("original".into()),
                http_headers: HashMap::new(),
            },
            stream_manifest::AudioStreamFormat {
                format_id: "251-pt".into(),
                url: "https://rr1---sn.googlevideo.com/videoplayback?itag=251&lang=pt".into(),
                acodec: "opus".into(),
                abr_kbps: Some(128.0),
                ext: "webm".into(),
                language: Some("pt-BR".into()),
                language_label: "Português (dublado)".into(),
                is_default: false,
                is_dubbed: true,
                format_note: Some("dubbed".into()),
                http_headers: HashMap::new(),
            },
        ],
        muxed_formats: Vec::new(),
        chapters: vec![stream_manifest::ManifestChapter {
            title: "Intro".into(),
            start_secs: 0.0,
            end_secs: 60.0,
        }],
        captions: vec![stream_manifest::ManifestCaptionTrack {
            language_code: "pt".into(),
            label: "Português".into(),
            url: "https://www.youtube.com/api/timedtext?v=dstr0000001&lang=pt".into(),
            is_auto_generated: false,
        }],
        storyboard: Some(stream_manifest::ManifestStoryboardSpec {
            rows: 5,
            columns: 5,
            width: 160,
            height: 90,
            fps: Some(0.5),
            fragments: vec![stream_manifest::ManifestStoryboardFragment {
                url: "https://i.ytimg.com/sb/dstr0000001/storyboard3_L2/M0.jpg".into(),
                duration_secs: 50.0,
            }],
        }),
        fetched_at: now,
        expires_at: now + Duration::from_secs(600),
    };
    let arc_manifest = app.playback.stream_manifest_cache.insert(manifest);
    app.hydrate_from_stream_manifest(&video.url, &arc_manifest);

    assert!(app.playback.current_manifest.is_some());
    assert_eq!(app.playback.qualities, vec![1080, 720]);
    assert!(app.playback.storyboard.is_some());
    assert_eq!(app.watch.overlay_cache.chapters.len(), 1);
    assert_eq!(app.playback.audio_tracks.len(), 2);
    assert_eq!(app.watch.extras.subtitles.len(), 1);
    assert!(
        app.selected_metadata
            .as_ref()
            .is_some_and(|m| m.channel_verified)
    );

    // Direct stream URL matches telemetry path guard.
    let direct_url = "https://rr1---sn.googlevideo.com/videoplayback?itag=248&expire=4102444800";
    app.playback.active_direct_stream_url = Some(direct_url.into());
    assert!(app.raw_path_matches_video(Some(direct_url), &video.url));

    // Playback failure (e.g. HTTP 403) invalidates cached manifest and triggers fallback flag.
    app.handle_queue_player_event(PlaybackEvent::EndFile {
        playlist_entry_id: 1,
        failed: true,
    });
    assert!(
        app.playback
            .stream_manifest_cache
            .get("dstr0000001")
            .is_none()
    );
    assert!(app.playback.current_manifest.is_none());
    assert!(app.playback.direct_stream_fallback_attempted);

    // Speculative next-video prefetch prioritizes queue over active playlist and skips current video.
    let queued_next = Video {
        title: "Next Queued".into(),
        channel: "Ch".into(),
        duration: "3:00".into(),
        url: "https://www.youtube.com/watch?v=qnext000001".into(),
        thumbnail: String::new(),
    };
    let pl_next = Video {
        title: "Next Playlist".into(),
        channel: "Ch".into(),
        duration: "4:00".into(),
        url: "https://www.youtube.com/watch?v=pnext000001".into(),
        thumbnail: String::new(),
    };
    app.watch.active_playlist = Some(app_state::ActivePlaylistPlayback::new(
        "PLtest01".into(),
        "Test Playlist".into(),
        "Owner".into(),
        playlist_sync::PlaylistPrivacy::Public,
        false,
        vec![video.clone(), pl_next],
        Some(2),
        0,
        false,
        1,
        false,
        None,
    ));
    assert_eq!(
        app.next_prefetch_candidate().map(|(id, _)| id),
        Some("pnext000001".into())
    );
    app.queue.data.add(queued_next).unwrap();
    assert_eq!(
        app.next_prefetch_candidate().map(|(id, _)| id),
        Some("qnext000001".into())
    );
}

#[test]
fn wave2_zero_quota_mutation_helpers_and_feedback() {
    let ctx = egui::Context::default();
    assert_eq!(
        FastTube::to_innertube_rating(Some(oauth::VideoRating::Like)),
        innertube::VideoRating::Like
    );
    assert_eq!(
        FastTube::to_innertube_rating(Some(oauth::VideoRating::Dislike)),
        innertube::VideoRating::Dislike
    );
    assert_eq!(
        FastTube::to_innertube_rating(Some(oauth::VideoRating::None)),
        innertube::VideoRating::Remove
    );
    assert_eq!(
        FastTube::to_innertube_rating(None),
        innertube::VideoRating::Remove
    );

    let add_wl =
        innertube::build_playlist_add_video_payload("WL", "dQw4w9WgXcQ").expect("WL payload");
    assert_eq!(add_wl["playlistId"], "WL");
    assert_eq!(add_wl["actions"][0]["action"], "ACTION_ADD_VIDEO");
    assert_eq!(add_wl["actions"][0]["addedVideoId"], "dQw4w9WgXcQ");

    let (rate_ep, rate_body) =
        innertube::build_rate_video_payload("dQw4w9WgXcQ", innertube::VideoRating::Like)
            .expect("rate payload");
    assert_eq!(rate_ep, "like/like");
    assert_eq!(rate_body["target"]["videoId"], "dQw4w9WgXcQ");

    let (sub_ep, sub_body) =
        innertube::build_subscription_payload("UC1234567890123456789012", true)
            .expect("subscribe payload");
    assert_eq!(sub_ep, "subscription/subscribe");
    assert_eq!(sub_body["channelIds"][0], "UC1234567890123456789012");

    let fb_body = innertube::build_feedback_payload("AB9zfpTestToken").expect("feedback payload");
    assert_eq!(fb_body["feedbackTokens"][0], "AB9zfpTestToken");

    let mut app = FastTube::default();
    app.feed.videos = vec![
        Video {
            title: "Keep Me".into(),
            channel: "Ch".into(),
            duration: "2:00".into(),
            url: "https://www.youtube.com/watch?v=keep0000001".into(),
            thumbnail: String::new(),
        },
        Video {
            title: "Hide Me".into(),
            channel: "Ch".into(),
            duration: "2:00".into(),
            url: "https://www.youtube.com/watch?v=hide0000001".into(),
            thumbnail: String::new(),
        },
    ];
    app.start_zero_quota_feedback("hide0000001".into(), None, true, &ctx);
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Keep Me");
    assert!(
        app.feed
            .search_reported_urls
            .contains("https://www.youtube.com/watch?v=hide0000001")
    );
}

#[test]
fn wave3_notifications_inbox_unread_count_and_mark_all_read() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    let url_dup = "https://www.youtube.com/watch?v=noti0000001".to_owned();
    let url_rss = "https://www.youtube.com/watch?v=rssnew00002".to_owned();
    app.feed.videos.push(Video {
        title: "RSS Upload".into(),
        channel: "Local Followed".into(),
        duration: "5:00".into(),
        url: url_rss.clone(),
        thumbnail: String::new(),
    });
    app.feed.notifications = vec![
        innertube::InnerTubeNotification {
            id: "n-read".into(),
            title: "Already Read".into(),
            channel: "Chan A".into(),
            thumbnail_url: None,
            channel_avatar_url: None,
            video_url: Some("https://www.youtube.com/watch?v=read0000001".into()),
            published_text: "2h ago".into(),
            is_read: true,
            mark_read_token: None,
            ..Default::default()
        },
        innertube::InnerTubeNotification {
            id: "n-unread".into(),
            title: "Fresh InnerTube".into(),
            channel: "Chan B".into(),
            thumbnail_url: None,
            channel_avatar_url: None,
            video_url: Some(url_dup.clone()),
            published_text: "5m ago".into(),
            is_read: false,
            mark_read_token: Some("tok-1".into()),
            ..Default::default()
        },
    ];
    // Duplicate URL matching InnerTube notification must not double-count.
    app.followed_channels.new_video_urls.insert(url_dup);
    app.followed_channels.new_video_urls.insert(url_rss.clone());

    assert_eq!(app.unread_notifications_count(), 2);

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 640.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.notifications_inbox_panel_ui(ui, &c);
        },
    );
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();
    assert!(rendered.contains("Fresh InnerTube"));
    assert!(rendered.contains("RSS Upload"));

    app.mark_notification_read(&format!("rss:{url_rss}"), None, &ctx);
    assert_eq!(app.unread_notifications_count(), 1);

    app.mark_all_notifications_read(&ctx);
    assert_eq!(app.unread_notifications_count(), 0);
    assert!(app.followed_channels.new_video_urls.is_empty());
    assert!(app.feed.notifications.iter().all(|n| n.is_read));
}

#[test]
fn wave3_incognito_mode_suppresses_watch_history_and_remote_sync() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
    let video = Video {
        title: "Private Video".into(),
        channel: "Creator".into(),
        duration: "10:00".into(),
        url: "https://www.youtube.com/watch?v=priv0000001".into(),
        thumbnail: String::new(),
    };

    app.set_incognito_mode(true, &ctx);
    assert!(app.ui.incognito_mode);

    app.record_video_accessed_in_history(&video, Some(15_000));
    assert!(app.history.data.entries().is_empty());

    app.activate_video_playback(&video, Some(45_000), None, Instant::now());
    if let Some(active) = app.playback.active.as_mut() {
        active.has_live_sample = true;
        active.duration_ms = Some(600_000);
    }
    assert!(!app.record_active_playback_in_memory());
    assert!(app.history.data.entries().is_empty());

    app.auth.cached_has_saved_profile = true;
    app.start_remote_watch_tracking(&video, None, &ctx);
    assert!(app.history.remote.watch_init_rx.is_none());

    // Header renders visible Incognito badge while active.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1360.0, 120.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    let header_text = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();
    assert!(
        header_text.contains("Anônimo") || header_text.contains("Incognito"),
        "expected visible incognito badge in masthead, got:\n{header_text}"
    );

    // Turning off incognito restores watch history recording.
    app.set_incognito_mode(false, &ctx);
    assert!(!app.ui.incognito_mode);
    app.record_video_accessed_in_history(&video, Some(15_000));
    assert_eq!(app.history.data.entries().len(), 1);
}

#[test]
fn wave3_restricted_mode_and_country_region_propagate_to_innertube_context() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();

    let default_it = app.innertube_context();
    assert!(!default_it.restricted_mode);

    app.set_restricted_mode(true, &ctx);
    app.set_country_region(Some("jp".into()), &ctx);
    app.set_ui_language(i18n::UiLanguage::En, &ctx);

    assert!(app.settings.prefs.value.restricted_mode);
    assert_eq!(app.settings.prefs.value.country_code.as_deref(), Some("JP"));
    assert_eq!(app.settings.prefs.value.language, i18n::UiLanguage::En);

    let updated_it = app.innertube_context();
    assert!(updated_it.restricted_mode);
    assert_eq!(updated_it.gl, "JP");
    assert_eq!(updated_it.hl, "en");
}

#[test]
fn wave3_report_history_atomic_persistence_and_corrupt_file_preservation() {
    let ctx = egui::Context::default();
    let dir = std::env::temp_dir().join(format!(
        "fasttube-wave3-report-history-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let valid_path = dir.join("report-history.json");
    let corrupt_path = dir.join("corrupt-report-history.json");

    let mut app = FastTube::default();
    app.feed.report_history_path = Some(valid_path.clone());
    app.selected = Some(Video {
        title: "Suspicious Stream".into(),
        channel: "Spammer".into(),
        duration: "3:15".into(),
        url: "https://www.youtube.com/watch?v=spam0000001".into(),
        thumbnail: String::new(),
    });

    app.submit_video_report(
        "Spam or misleading".into(),
        "Fake giveaway link at 1:10".into(),
        &ctx,
    );

    assert!(valid_path.is_file());
    let raw = std::fs::read_to_string(&valid_path).expect("read report-history.json");
    assert!(raw.contains("spam0000001"));
    assert!(raw.contains("Spam or misleading"));
    assert!(raw.contains("Fake giveaway link at 1:10"));
    assert!(!raw.contains("access_token"));
    assert!(!raw.contains("SAPISID"));

    let loaded = FastTube::load_report_history_entries_from(&valid_path)
        .expect("parse persisted report-history.json");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].video_id.as_deref(), Some("spam0000001"));

    // Corrupt file must stay untouched on disk.
    let corrupt_bytes = "{invalid-json-preserve-me";
    std::fs::write(&corrupt_path, corrupt_bytes).expect("seed corrupt file");
    app.feed.report_history_path = Some(corrupt_path.clone());
    app.submit_video_report("Violent content".into(), String::new(), &ctx);
    assert_eq!(
        std::fs::read_to_string(&corrupt_path).expect("read corrupt file"),
        corrupt_bytes
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wave3_remappable_shortcuts_search_help_f11_home_end_chapters_captions_volume() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();

    // 1. `/` requests focus on search bar; `?` toggles shortcuts modal.
    let slash_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Slash,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(slash_input, |ui| {
        app.handle_help_shortcuts(ui.ctx());
    });
    out.textures_delta.clear();
    assert!(app.ui.focus_search_requested);

    let question_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Questionmark,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(question_input, |ui| {
        app.handle_help_shortcuts(ui.ctx());
    });
    out.textures_delta.clear();
    assert!(app.ui.shortcuts_open);
    app.ui.shortcuts_open = false;

    // Set up active watch page with chapters and duration.
    let video = Video {
        title: "Chaptered Tutorial".into(),
        channel: "Rust Dev".into(),
        duration: "10:00".into(),
        url: "https://www.youtube.com/watch?v=chap0000001".into(),
        thumbnail: String::new(),
    };
    app.selected = Some(video.clone());
    app.playback.mini_player_volume = 100.0;
    app.activate_video_playback(&video, Some(120_000), None, Instant::now());
    if let Some(active) = app.playback.active.as_mut() {
        active.has_live_sample = true;
        active.duration_ms = Some(600_000);
    }
    app.watch.overlay_cache.chapters = vec![
        overlay::OverlayChapter {
            title: "Intro".into(),
            time_secs: 0.0,
        },
        overlay::OverlayChapter {
            title: "Part 1".into(),
            time_secs: 60.0,
        },
        overlay::OverlayChapter {
            title: "Part 2".into(),
            time_secs: 240.0,
        },
    ];

    // 2. Volume boost > 100% via ArrowUp.
    let vol_up_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::ArrowUp,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(vol_up_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert!(
        (app.playback.mini_player_volume - 105.0).abs() < 0.01,
        "expected volume boost to 105%, got {}",
        app.playback.mini_player_volume
    );

    // 3. Caption size +/- via Plus / Minus.
    let base_scale = app.settings.prefs.value.caption_style.font_scale;
    let cap_up_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Plus,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(cap_up_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert!(app.settings.prefs.value.caption_style.font_scale > base_scale);

    let cap_down_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Minus,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(cap_down_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert!((app.settings.prefs.value.caption_style.font_scale - base_scale).abs() < 0.01);

    // 4. Ctrl+ArrowRight / Ctrl+ArrowLeft jump between chapters.
    let next_ch_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(next_ch_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 240_000);

    let prev_ch_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::ArrowLeft,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(prev_ch_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 60_000);

    // 5. Home / End seek.
    let home_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Home,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(home_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 0);

    let end_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::End,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(end_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 599_000);

    // 6. F11 toggles fullscreen.
    assert!(!app.playback.fullscreen);
    let f11_input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::F11,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };
    let mut out = ctx.run_ui(f11_input, |ui| {
        app.handle_player_shortcuts(ui.ctx(), true);
    });
    out.textures_delta.clear();
    assert!(app.playback.fullscreen);
}

#[test]
fn wave3_persisted_downloads_restore_local_playback_detection_and_queue_repeat_sync() {
    let dir = std::env::temp_dir().join(format!(
        "fasttube-wave3-downloads-local-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let local_media = dir.join("offline_clip.mp4");
    std::fs::write(&local_media, b"fake-mp4-bytes").expect("create fake local file");
    let local_str = local_media.to_string_lossy().to_string();

    assert!(FastTube::is_local_offline_media_path(&local_str));
    assert!(!FastTube::is_local_offline_media_path(
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
    ));
    assert!(!FastTube::is_local_offline_media_path(
        &dir.join("missing.mp4").to_string_lossy()
    ));

    let dl_json = dir.join("downloads.json");
    let mut app = FastTube::default();
    app.downloads.manager.items.push(downloads::DownloadItem {
        id: 7,
        video: Video {
            title: "Offline Clip".into(),
            channel: "Creator".into(),
            duration: "2:00".into(),
            url: "https://www.youtube.com/watch?v=offl0000001".into(),
            thumbnail: String::new(),
        },
        format: downloads::DownloadFormat::Video1080p,
        output_dir: dir.clone(),
        status: downloads::DownloadStatus::Completed {
            output_path: Some(local_media.clone()),
        },
    });
    app.persist_downloads_state_at(&dl_json)
        .expect("persist downloads.json");

    let loaded = downloads::load_download_records_from(&dl_json).expect("load downloads.json");
    assert_eq!(loaded.len(), 1);
    let mut restored_app = FastTube::default();
    restored_app
        .downloads
        .manager
        .restore_from_records(loaded, &dir);
    assert_eq!(restored_app.downloads.manager.items.len(), 1);
    assert_eq!(
        restored_app.downloads.manager.items[0].video.title,
        "Offline Clip"
    );

    // Queue repeat_mode persists alongside queue.json.
    let queue_path = dir.join("queue.json");
    app.queue.path = Some(queue_path.clone());
    app.queue.read_failed = false;
    app.queue.repeat_mode = queue::RepeatMode::All;
    app.persist_queue();
    app.flush_persistence();
    let loaded_q = Queue::load(&queue_path, DEFAULT_QUEUE_LIMIT).expect("reload queue.json");
    assert_eq!(loaded_q.repeat_mode(), queue::RepeatMode::All);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wave3_explore_hub_manage_subscriptions_and_shorts_prefetch() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    // 1. Explore Hub category selection & headless render.
    app.ui.browse_section = BrowseSection::Explore;
    app.select_explore_category(innertube::ExploreCategory::Gaming, &ctx);
    assert_eq!(
        app.feed.explore_category,
        innertube::ExploreCategory::Gaming
    );

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut explore_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.explore_hub_page_ui(ui, &c);
        },
    );
    let explore_text = collect_rendered_text(&explore_out.shapes).join("\n");
    explore_out.textures_delta.clear();
    assert!(explore_text.contains("Explorar"));
    assert!(explore_text.contains("Jogos"));

    // 2. Manage Subscriptions view + per-channel notify level + unsubscribe.
    app.followed_channels
        .data
        .channels
        .push(followed_channels::FollowedChannel {
            channel_id: "UCmanage0000000000000001".into(),
            title: "Managed Rust Channel".into(),
            handle: Some("@managedrust".into()),
            url: "https://www.youtube.com/channel/UCmanage0000000000000001".into(),
            avatar_url: None,
            followed_at_ms: 1,
            last_seen_video_id: None,
        });
    app.ui.browse_section = BrowseSection::Subscriptions;
    app.ui.manage_subscriptions_open = true;
    app.set_channel_notification_level(
        "UCmanage0000000000000001",
        settings::ChannelNotifyLevel::All,
        &ctx,
    );
    assert_eq!(
        app.settings
            .prefs
            .value
            .per_channel_notifications
            .get("UCmanage0000000000000001")
            .copied(),
        Some(settings::ChannelNotifyLevel::All)
    );

    let mut manage_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.manage_subscriptions_page_ui(ui, &c);
        },
    );
    let manage_text = collect_rendered_text(&manage_out.shapes).join("\n");
    manage_out.textures_delta.clear();
    assert!(manage_text.contains("Managed Rust Channel"));

    app.unsubscribe_channel("UCmanage0000000000000001", &ctx);
    assert!(app.followed_channels.data.channels.is_empty());

    // 3. Shorts prefetch trigger with 2 Shorts in feed.
    let s1 = Video {
        title: "Short One".into(),
        channel: "Creator".into(),
        duration: "0:35".into(),
        url: "https://www.youtube.com/shorts/shrt0000001".into(),
        thumbnail: String::new(),
    };
    let s2 = Video {
        title: "Short Two".into(),
        channel: "Creator".into(),
        duration: "0:40".into(),
        url: "https://www.youtube.com/shorts/shrt0000002".into(),
        thumbnail: String::new(),
    };
    app.feed.videos = vec![s1.clone(), s2.clone()];
    app.selected = Some(s1);
    assert_eq!(app.shorts_feed_candidates().len(), 2);
    assert_eq!(app.next_or_prev_short(true).map(|v| v.url), Some(s2.url));
    app.prefetch_next_short(&ctx);
}

#[test]
fn wave4_theme_system_resolves_live_dark_light_and_backup_round_trips() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.settings.theme.path = None;
    app.settings.prefs.path = None;

    // 1. Theme::System follows live system dark/light preference.
    app.apply_theme(Theme::System);
    app.desktop_win11.test_system_dark_override = Some(true);
    assert_eq!(app.resolved_theme(), settings::ResolvedTheme::Dark);
    assert_eq!(app.effective_theme(), Theme::Dark);
    assert!(app.is_dark_theme());
    app.sync_theme_visuals(&ctx);
    assert!(ctx.global_style().visuals.dark_mode);

    app.desktop_win11.test_system_dark_override = Some(false);
    assert_eq!(app.resolved_theme(), settings::ResolvedTheme::Light);
    assert_eq!(app.effective_theme(), Theme::Light);
    assert!(!app.is_dark_theme());
    app.sync_theme_visuals(&ctx);
    assert!(!ctx.global_style().visuals.dark_mode);

    // 2. Backup export/import round-trips "system" theme cleanly.
    let dir = std::env::temp_dir().join(format!("fasttube-w4-theme-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let backup_path = dir.join("backup.json");

    app.backup.path_text = backup_path.to_string_lossy().into_owned();
    app.export_user_data();
    assert!(backup_path.exists());

    let mut restored = FastTube::default();
    restored.settings.theme.path = None;
    restored.settings.prefs.path = None;
    restored.apply_theme(Theme::Light);
    assert_eq!(restored.settings.theme.value, Theme::Light);

    restored.backup.path_text = backup_path.to_string_lossy().into_owned();
    let imported = restored
        .read_imported_user_data()
        .expect("exported backup with 'system' theme must decode cleanly");
    assert_eq!(imported.theme, Theme::System);
    restored.apply_theme(imported.theme);
    assert_eq!(restored.settings.theme.value, Theme::System);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wave4_jump_list_open_section_command_navigates_all_sections() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.path = None;

    let pt_tasks = win11::default_jump_list_tasks(true);
    let en_tasks = win11::default_jump_list_tasks(false);
    assert_eq!(pt_tasks.len(), 5);
    assert_eq!(en_tasks.len(), 5);

    let expected = [
        ("fasttube://section/home", BrowseSection::Home),
        (
            "fasttube://section/subscriptions",
            BrowseSection::Subscriptions,
        ),
        ("fasttube://section/history", BrowseSection::History),
        ("fasttube://section/downloads", BrowseSection::Downloads),
        ("fasttube://section/watch_later", BrowseSection::WatchLater),
    ];

    for (idx, (uri, expected_section)) in expected.iter().enumerate() {
        assert_eq!(pt_tasks[idx].args, *uri);
        assert_eq!(en_tasks[idx].args, *uri);
        let req = desktop::launch_argument_request(["fasttube.exe".to_owned(), (*uri).to_owned()]);
        let cmd = desktop::parse_instance_request(&req)
            .expect("Jump List URI must parse into InstanceCommand::OpenSection");
        assert!(matches!(cmd, desktop::InstanceCommand::OpenSection(_)));
        app.handle_instance_command(cmd, &ctx);
        assert_eq!(app.ui.browse_section, *expected_section);
        assert!(!app.ui.settings_open);
        assert!(!app.channels.view_open);
        assert!(!app.ui.search_results_mode);
    }

    // Stop any background fetches triggered during section switches.
    app.feed.home.batch_task.cancel();
    app.feed.home_innertube_task.cancel();
    app.feed.subs_innertube_task.cancel();
    app.followed_channels.feed_task.cancel();
}

#[test]
fn wave4_drag_and_drop_opens_youtube_urls_and_local_media_files() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.path = None;

    // 1. Dropped YouTube video URL schedules video open.
    let video_input = desktop::classify_dropped_text("https://youtu.be/dQw4w9WgXcQ")
        .expect("YouTube video URL must classify");
    assert!(matches!(video_input, desktop::DroppedInput::YouTubeUrl(_)));
    assert!(app.handle_dropped_input(video_input, &ctx));
    assert_eq!(
        app.playback
            .pending_open_video
            .as_ref()
            .map(|v| v.url.as_str()),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );

    // 2. Dropped YouTube channel URL opens channel view.
    let chan_input = desktop::classify_dropped_text("https://www.youtube.com/@RustLang")
        .expect("YouTube channel URL must classify");
    assert!(app.handle_dropped_input(chan_input, &ctx));
    assert!(app.channels.view_open);
    assert_eq!(
        app.channels.last_target,
        Some(ChannelTarget::Handle("@RustLang".into()))
    );
    app.channels.videos.task.cancel();

    // 3. Dropped .url shortcut file resolves YouTube target cleanly.
    let dir = std::env::temp_dir().join(format!("fasttube-w4-dnd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let shortcut = dir.join("clip.url");
    std::fs::write(
        &shortcut,
        "[InternetShortcut]\r\nURL=https://www.youtube.com/watch?v=9bZkp7q19f0\r\n",
    )
    .unwrap();
    let shortcut_input =
        desktop::classify_dropped_path(&shortcut).expect(".url shortcut must classify");
    assert!(app.handle_dropped_input(shortcut_input, &ctx));
    assert_eq!(
        app.playback
            .pending_open_video
            .as_ref()
            .map(|v| v.url.as_str()),
        Some("https://www.youtube.com/watch?v=9bZkp7q19f0")
    );

    // 4. Dropped local media file triggers offline playback cleanly.
    let media_file = dir.join("local_talk.mp4");
    std::fs::write(&media_file, b"fake-mp4-bytes").unwrap();
    let media_input =
        desktop::classify_dropped_path(&media_file).expect("local .mp4 file must classify");
    assert_eq!(
        media_input,
        desktop::DroppedInput::LocalMediaFile(media_file.clone())
    );
    assert!(app.handle_dropped_input(media_input, &ctx));
    assert_eq!(
        app.playback
            .pending_open_video
            .as_ref()
            .map(|v| v.title.as_str()),
        Some("local_talk")
    );
    assert_eq!(
        app.playback
            .pending_open_video
            .as_ref()
            .map(|v| v.url.as_str()),
        media_file.to_str()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wave4_per_channel_speed_memory_saves_and_restores_across_videos() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.settings.prefs.path = None;
    app.settings.playback.path = None;
    app.history.path = None;
    app.settings.playback.value.speed_quarters = 4; // 1.0x default
    app.settings.prefs.value.remember_speed_per_channel = true;

    let video_a1 = Video {
        title: "Rust Deep Dive 1".into(),
        channel: "Crustacean Labs".into(),
        url: "https://www.youtube.com/watch?v=spdA0000001".into(),
        thumbnail: String::new(),
        duration: "10:00".into(),
    };
    let video_a2 = Video {
        title: "Rust Deep Dive 2".into(),
        channel: "  crustacean labs  ".into(),
        url: "https://www.youtube.com/watch?v=spdA0000002".into(),
        thumbnail: String::new(),
        duration: "12:00".into(),
    };
    let video_b = Video {
        title: "Lo-Fi Beats".into(),
        channel: "Chill Radio".into(),
        url: "https://www.youtube.com/watch?v=spdB0000001".into(),
        thumbnail: String::new(),
        duration: "30:00".into(),
    };

    // Select video A1 and set custom speed 1.75x -> remembered for Crustacean Labs.
    app.selected = Some(video_a1);
    app.playback.speed = app.resolved_initial_playback_speed("Crustacean Labs");
    assert!((app.playback.speed - 1.0).abs() < 1e-6);
    app.apply_and_remember_playback_speed(1.75);
    assert!((app.playback.speed - 1.75).abs() < 1e-6);
    assert_eq!(
        settings::get_channel_playback_speed(
            &app.settings.prefs.value.per_channel_speeds,
            "Crustacean Labs"
        ),
        Some(1.75)
    );

    // Opening another video from the same channel (case/whitespace insensitive) restores 1.75x.
    let restored_a2 = app.resolved_initial_playback_speed(&video_a2.channel);
    app.selected = Some(video_a2);
    app.playback.speed = restored_a2;
    assert!((app.playback.speed - 1.75).abs() < 1e-6);

    // Opening a video from a different channel restores default speed 1.0x.
    let restored_b = app.resolved_initial_playback_speed(&video_b.channel);
    app.selected = Some(video_b);
    app.playback.speed = restored_b;
    assert!((app.playback.speed - 1.0).abs() < 1e-6);

    // Disabling remember_speed_per_channel falls back to default speed even for saved channel.
    app.settings.prefs.value.remember_speed_per_channel = false;
    assert!((app.resolved_initial_playback_speed("Crustacean Labs") - 1.0).abs() < 1e-6);
    app.close_video(&ctx);
}

#[test]
fn wave4_auto_skip_chapters_by_keyword_skips_once_without_looping() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.settings.prefs.value.language = i18n::UiLanguage::PtBr;
    app.settings.prefs.value.auto_skip_chapters_enabled = true;
    app.settings.prefs.value.auto_skip_chapter_keywords =
        vec!["sponsor".into(), "merch".into(), "outro".into()];
    app.watch.overlay_cache.chapters = vec![
        overlay::OverlayChapter {
            title: "Introduction".into(),
            time_secs: 0.0,
        },
        overlay::OverlayChapter {
            title: "Sponsor Message from Acme".into(),
            time_secs: 30.0,
        },
        overlay::OverlayChapter {
            title: "Main Benchmark".into(),
            time_secs: 90.0,
        },
        overlay::OverlayChapter {
            title: "Outro & Links".into(),
            time_secs: 240.0,
        },
    ];

    // Inside non-matching chapter -> no skip.
    assert_eq!(app.try_auto_skip_keyword_chapter(12.0, Some(300.0)), None);

    // Inside matching Sponsor chapter -> skips to next chapter (90.0s) and emits PT toast.
    assert_eq!(
        app.try_auto_skip_keyword_chapter(35.0, Some(300.0)),
        Some(90.0)
    );
    assert!(app.watch.auto_skipped_chapter_indices.contains(&1));
    assert!(app.toasts.visible_toasts().iter().any(|t| {
        t.message
            .contains("Capítulo pulado: Sponsor Message from Acme")
    }));

    // User scrubs back into chapter 1 -> must NOT skip again (no infinite skip loop).
    assert_eq!(app.try_auto_skip_keyword_chapter(40.0, Some(300.0)), None);

    // Matching final Outro chapter skips near video end (duration - 0.5s) in English too.
    app.settings.prefs.value.language = i18n::UiLanguage::En;
    let outro_target = app
        .try_auto_skip_keyword_chapter(250.0, Some(300.0))
        .expect("final matching chapter should skip to near end");
    assert!((outro_target - 299.5).abs() < 1e-6);
    assert!(
        app.toasts
            .visible_toasts()
            .iter()
            .any(|t| t.message.contains("Skipped chapter: Outro & Links"))
    );

    // Closing the video resets auto_skipped_chapter_indices for the next session.
    app.close_video(&ctx);
    assert!(app.watch.auto_skipped_chapter_indices.is_empty());
}

#[test]
fn wave4_async_task_unification_enforces_cancellation_and_monotonic_ids() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();

    // 1. Monotonic request IDs & automatic cancellation of superseded AsyncTask.
    let (id1, cancel1, tx1) = app.watch.community_task.begin_manual();
    let (id2, cancel2, tx2) = app.watch.community_task.begin_manual();
    assert!(id2 > id1);
    assert!(cancel1.cancelled(), "superseded task must be cancelled");
    assert!(!cancel2.cancelled());
    assert_eq!(app.watch.community_task.active_request_id(), Some(id2));
    assert!(
        tx1.send((
            id1,
            app_state::CommunityExtrasFetchMessage {
                request_id: id1,
                outcome: community_apis::CommunityExtrasOutcome {
                    video_id: "stale000001".into(),
                    ryd: None,
                    dearrow: None,
                },
            },
        ))
        .is_err(),
        "dropped superseded channel must reject sends"
    );

    // Stale-ID payload pushed onto active channel is discarded without clearing active task.
    tx2.send((
        id1,
        app_state::CommunityExtrasFetchMessage {
            request_id: id1,
            outcome: community_apis::CommunityExtrasOutcome {
                video_id: "stale000002".into(),
                ryd: None,
                dearrow: None,
            },
        },
    ))
    .unwrap();
    app.poll_community_extras();
    assert!(app.watch.community_cache.get("stale000002").is_none());
    assert!(app.watch.community_task.is_running());
    assert_eq!(app.watch.community_task.active_request_id(), Some(id2));

    // Matching-ID payload is applied and clears active state.
    tx2.send((
        id2,
        app_state::CommunityExtrasFetchMessage {
            request_id: id2,
            outcome: community_apis::CommunityExtrasOutcome {
                video_id: "fresh000001".into(),
                ryd: None,
                dearrow: None,
            },
        },
    ))
    .unwrap();
    app.poll_community_extras();
    assert!(app.watch.community_cache.get("fresh000001").is_some());
    assert!(!app.watch.community_task.is_running());

    // 2. AsyncStreamTask enforces monotonic IDs & cancellation on app exit.
    let (s_id1, s_cancel1, _s_tx1) = app.feed.home.stream_task.begin_manual();
    let (s_id2, s_cancel2, _s_tx2) = app.feed.home.stream_task.begin_manual();
    assert!(s_id2 > s_id1);
    assert!(s_cancel1.cancelled());
    assert!(!s_cancel2.cancelled());
    assert!(app.has_active_async_jobs());

    <FastTube as eframe::App>::on_exit(&mut app, None);
    assert!(s_cancel2.cancelled());
    assert!(!app.feed.home.stream_task.is_running());
    let _ = ctx;
}

#[test]
fn header_removes_create_voice_keyboard_and_aligns_search_pill_and_renders_official_notifications()
{
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let palette = app.palette();

    for width in [1280.0_f32, 640.0] {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 140.0));
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.header_ui(ui, &c);
            },
        );
        out.textures_delta.clear();
        let texts = collect_rendered_text(&out.shapes).join("\n");
        assert!(!texts.contains("+ Criar"));
        assert!(!texts.contains("+ Create"));
    }

    // Verify search pill renders cleanly at requested width.
    let search_screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 80.0));
    let mut pill_rect = egui::Rect::NOTHING;
    let mut s_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(search_screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            pill_rect = app.header_search_ui(ui, &c, palette, 460.0);
        },
    );
    s_out.textures_delta.clear();
    assert!((pill_rect.width() - 460.0).abs() <= 2.0);
    assert!((pill_rect.height() - 38.0).abs() <= 1.0);

    fn collect_filled_rects(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Rect(r) if r.fill != egui::Color32::TRANSPARENT => out.push(r.rect),
            egui::Shape::Vec(items) => {
                for item in items {
                    collect_filled_rects(item, out);
                }
            }
            _ => {}
        }
    }
    let mut filled_rects = Vec::new();
    for clipped in &s_out.shapes {
        collect_filled_rects(&clipped.shape, &mut filled_rects);
    }
    assert!(
        filled_rects.iter().any(|r| {
            (r.width() - 64.0).abs() <= 0.5
                && (r.height() - 38.0).abs() <= 0.5
                && (r.right() - pill_rect.right()).abs() <= 0.5
        }),
        "expected search button rect flush with pill right edge ({pill_rect:?}), got: {filled_rects:?}"
    );

    // Notifications: connect prompt when session missing.
    app.feed.notifications_needs_connect = true;
    let notif_screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 680.0));
    let mut connect_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(notif_screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.notifications_inbox_panel_ui(ui, &c);
        },
    );
    connect_out.textures_delta.clear();
    let connect_text = collect_rendered_text(&connect_out.shapes).join("\n");
    assert!(connect_text.contains("Conectar YouTube"));

    // Notifications: error + retry state.
    app.feed.notifications_needs_connect = false;
    app.feed.notifications_error = Some("Falha temporária".into());
    let mut err_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(notif_screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.notifications_inbox_panel_ui(ui, &c);
        },
    );
    err_out.textures_delta.clear();
    let err_text = collect_rendered_text(&err_out.shapes).join("\n");
    assert!(err_text.contains("Tentar novamente"));

    // Notifications: official items with section header, non-duplicated channel subtitle, and Load more.
    app.feed.notifications_error = None;
    app.feed.notifications_continuation = Some("NEXT_PAGE_TOK".into());
    app.feed.notifications = vec![innertube::InnerTubeNotification {
        id: "yt-1".into(),
        title: "Hayashii Lives: A PRIMEIRA SEXTA de GTA 6 de MUITAS! | GTA 6 (PARTE 04)".into(),
        channel: "Hayashii Lives".into(),
        published_text: "há 15 horas".into(),
        section_title: Some("Importantes".into()),
        is_read: false,
        hide_feedback_token: Some("FB_HIDE".into()),
        ..Default::default()
    }];
    let mut list_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(notif_screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.notifications_inbox_panel_ui(ui, &c);
        },
    );
    list_out.textures_delta.clear();
    let list_text = collect_rendered_text(&list_out.shapes).join("\n");
    assert!(list_text.contains("Importantes"));
    assert!(list_text.contains("há 15 horas"));
    assert!(!list_text.contains("Hayashii Lives • há 15 horas"));
    assert!(list_text.contains("Carregar mais"));
}
