use super::transcript_ui::estimate_transcript_segment_height;
use super::*;
use crate::browse_ui::compute_variable_visible_range;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

fn sample_video(id: &str, channel: &str) -> Video {
    Video {
        title: format!("Video {id}"),
        channel: channel.to_owned(),
        url: format!("https://www.youtube.com/watch?v={id}"),
        thumbnail: String::new(),
        duration: "10:00".to_owned(),
    }
}

#[test]
fn verified_badge_requires_real_channel_verified_flag() {
    assert!(!is_channel_verified(None));
    assert_eq!(
        format_sidebar_channel_label("Small Creator", None).as_deref(),
        Some("Small Creator")
    );

    let high_views_unverified = VideoMetadata {
        view_count: Some(500_000),
        channel_verified: false,
        ..Default::default()
    };
    assert!(!is_channel_verified(Some(&high_views_unverified)));
    let unverified_label =
        format_sidebar_channel_label("Viral Unverified", Some(&high_views_unverified))
            .expect("non-empty channel should produce a label");
    assert_eq!(unverified_label, "Viral Unverified");
    assert!(!unverified_label.contains('\u{2714}'));

    let low_views_verified = VideoMetadata {
        view_count: Some(120),
        channel_verified: true,
        ..Default::default()
    };
    assert!(is_channel_verified(Some(&low_views_verified)));
    assert_eq!(
        format_sidebar_channel_label("Verified Creator", Some(&low_views_verified)).as_deref(),
        Some("Verified Creator \u{2714}")
    );
    assert_eq!(
        format_sidebar_channel_label("", Some(&low_views_verified)),
        None
    );
}

#[test]
fn watch_channel_verified_uses_metadata_and_matching_channel_page() {
    let verified_meta = VideoMetadata {
        channel_verified: true,
        ..Default::default()
    };
    assert!(is_watch_channel_verified(Some(&verified_meta), None, None));

    let unverified_high_views = VideoMetadata {
        view_count: Some(2_000_000),
        channel_id: Some("UC1111111111111111111111".to_owned()),
        channel_verified: false,
        ..Default::default()
    };
    let target_a = ChannelTarget::ChannelId("UC1111111111111111111111".to_owned());
    assert!(!is_watch_channel_verified(
        Some(&unverified_high_views),
        None,
        Some(&target_a),
    ));

    let matching_verified_page = ChannelPage {
        target: target_a.clone(),
        channel_id: Some("UC1111111111111111111111".to_owned()),
        verified: Some(true),
        ..Default::default()
    };
    assert!(is_watch_channel_verified(
        Some(&unverified_high_views),
        Some(&matching_verified_page),
        Some(&target_a),
    ));

    let matching_unverified_page = ChannelPage {
        target: target_a.clone(),
        channel_id: Some("UC1111111111111111111111".to_owned()),
        verified: Some(false),
        ..Default::default()
    };
    assert!(!is_watch_channel_verified(
        Some(&unverified_high_views),
        Some(&matching_unverified_page),
        Some(&target_a),
    ));

    let other_verified_page = ChannelPage {
        target: ChannelTarget::ChannelId("UC2222222222222222222222".to_owned()),
        channel_id: Some("UC2222222222222222222222".to_owned()),
        verified: Some(true),
        ..Default::default()
    };
    assert!(!is_watch_channel_verified(
        Some(&unverified_high_views),
        Some(&other_verified_page),
        Some(&target_a),
    ));
}

#[test]
fn from_channel_filter_matches_channel_id_and_normalized_name_locally() {
    let current = sample_video("aaaaaaaaaaa", "Canal Alpha");
    let current_meta = VideoMetadata {
        channel_id: Some("UCaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
        ..Default::default()
    };

    // Excludes the currently playing video (same URL or same YouTube video ID).
    assert!(!sidebar_video_matches_current_channel(
        &current,
        Some(&current_meta),
        None,
        None,
        &current,
        Some(&current_meta),
    ));
    let same_id_short_url = Video {
        url: "https://youtu.be/aaaaaaaaaaa".to_owned(),
        ..current.clone()
    };
    assert!(!sidebar_video_matches_current_channel(
        &current,
        Some(&current_meta),
        None,
        None,
        &same_id_short_url,
        Some(&current_meta),
    ));

    // Matches by channel ID even when the display name differs.
    let same_id_renamed = sample_video("bbbbbbbbbbb", "Canal Alpha Official");
    let same_id_meta = VideoMetadata {
        channel_id: Some("UCaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
        ..Default::default()
    };
    assert!(sidebar_video_matches_current_channel(
        &current,
        Some(&current_meta),
        None,
        None,
        &same_id_renamed,
        Some(&same_id_meta),
    ));

    // Rejects different channel ID even when the display name is identical.
    let different_id_same_name = sample_video("ccccccccccc", "Canal Alpha");
    let different_id_meta = VideoMetadata {
        channel_id: Some("UCbbbbbbbbbbbbbbbbbbbbbb".to_owned()),
        ..Default::default()
    };
    assert!(!sidebar_video_matches_current_channel(
        &current,
        Some(&current_meta),
        None,
        None,
        &different_id_same_name,
        Some(&different_id_meta),
    ));

    // Falls back to normalized channel name when metadata lacks channel ID.
    let name_match = sample_video("ddddddddddd", "  canal alpha  ");
    assert!(sidebar_video_matches_current_channel(
        &current,
        Some(&current_meta),
        None,
        None,
        &name_match,
        None,
    ));
    let name_mismatch = sample_video("eeeeeeeeeee", "Canal Beta");
    assert!(!sidebar_video_matches_current_channel(
        &current,
        Some(&current_meta),
        None,
        None,
        &name_mismatch,
        None,
    ));

    // Uses live-info name hint when current video has an empty channel field.
    let current_empty_channel = sample_video("fffffffffff", "");
    assert!(sidebar_video_matches_current_channel(
        &current_empty_channel,
        None,
        None,
        Some("Canal Alpha"),
        &name_match,
        None,
    ));

    // Matches raw UC channel ID in `video.channel` against `candidate_meta.channel_id`.
    let current_raw_id = sample_video("ggggggggggg", "UCaaaaaaaaaaaaaaaaaaaaaa");
    assert!(sidebar_video_matches_current_channel(
        &current_raw_id,
        None,
        None,
        None,
        &same_id_renamed,
        Some(&same_id_meta),
    ));
}

#[test]
fn from_channel_sidebar_cache_filters_locally_without_clobbering_search() {
    let mut app = FastTube::default();
    app.feed.query = "existing user search".to_owned();

    let current = sample_video("11111111111", "Canal Alpha");
    let other_channel = sample_video("22222222222", "Canal Beta");
    app.selected = Some(current.clone());
    app.feed.videos = vec![current.clone(), other_channel.clone()];
    app.video_metadata.insert(
        current.url.clone(),
        VideoMetadata {
            channel_id: Some("UCaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
            ..Default::default()
        },
    );
    app.watch.extras.sidebar_tab = watch_extras::SidebarTab::FromChannel;

    app.ensure_watch_sidebar_cache();
    assert!(app.watch.sidebar_cache.cards.is_empty());
    assert_eq!(app.feed.query, "existing user search");
    assert!(!app.feed.search.task.is_running());

    let same_channel_by_id = sample_video("33333333333", "Canal Alpha (Clips)");
    app.video_metadata.insert(
        same_channel_by_id.url.clone(),
        VideoMetadata {
            channel_id: Some("UCaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
            view_count: Some(1_000_000),
            channel_verified: false,
            ..Default::default()
        },
    );
    app.feed.videos.push(same_channel_by_id.clone());
    app.ensure_watch_sidebar_cache();

    assert_eq!(app.watch.sidebar_cache.cards.len(), 1);
    assert_eq!(
        app.watch.sidebar_cache.cards[0].video.url,
        same_channel_by_id.url
    );
    assert_eq!(
        app.watch.sidebar_cache.cards[0].channel_label.as_deref(),
        Some("Canal Alpha (Clips)")
    );
}

#[test]
fn reset_video_and_audio_eq_clears_picture_and_audio_preset() {
    let mut app = FastTube::default();
    app.playback.brightness = 20;
    app.playback.contrast = -15;
    app.playback.saturation = 30;
    app.playback.audio_eq = overlay::AudioEqPreset::BassBoost;

    app.reset_video_and_audio_eq();

    assert_eq!(app.playback.brightness, 0);
    assert_eq!(app.playback.contrast, 0);
    assert_eq!(app.playback.saturation, 0);
    assert_eq!(app.playback.audio_eq, overlay::AudioEqPreset::Flat);
}

#[test]
fn transcript_search_cache_reuses_and_invalidates_on_key_change() {
    let cues = vec![
        transcript::Cue {
            start_ms: 0,
            end_ms: 2_000,
            text: "Hello Rust world".into(),
        },
        transcript::Cue {
            start_ms: 2_000,
            end_ms: 4_000,
            text: "Audio and video playback".into(),
        },
        transcript::Cue {
            start_ms: 4_000,
            end_ms: 6_000,
            text: "Rust makes desktop apps fast".into(),
        },
    ];
    let key1 = TranscriptSearchCacheKey {
        video_url: Some("https://www.youtube.com/watch?v=abc12345678".into()),
        transcript_revision: 1,
        cue_count: cues.len(),
        filter_query: "rust".into(),
        language_track_url: Some("en".into()),
    };
    let first = resolve_transcript_search_cache(None, key1.clone(), &cues);
    assert_eq!(first.matched_indices, vec![0, 2]);
    assert_eq!(first.match_ranges.len(), 2);
    assert!(!first.match_ranges[0].is_empty());

    // Identical key reuses the same Arc allocation without recomputing.
    let reused = resolve_transcript_search_cache(Some(&first), key1.clone(), &cues);
    assert!(Arc::ptr_eq(&first, &reused));

    // Changing query or language invalidates the cache.
    let mut key2 = key1.clone();
    key2.filter_query = "audio".into();
    let updated = resolve_transcript_search_cache(Some(&first), key2, &cues);
    assert!(!Arc::ptr_eq(&first, &updated));
    assert_eq!(updated.matched_indices, vec![1]);

    let mut key3 = key1;
    key3.language_track_url = Some("pt".into());
    let lang_changed = resolve_transcript_search_cache(Some(&first), key3, &cues);
    assert!(!Arc::ptr_eq(&first, &lang_changed));
}

#[test]
fn live_chat_filter_cache_reuses_and_invalidates_on_message_or_query_change() {
    let mut messages = VecDeque::new();
    messages.push_back(live_chat::LiveChatMessage {
        id: "m1".into(),
        author_name: "Alice".into(),
        author_channel_id: None,
        author_avatar_url: None,
        badges: vec![live_chat::LiveChatBadgeKind::Moderator],
        text: "Welcome everyone!".into(),
        timestamp_text: None,
        kind: live_chat::LiveChatMessageKind::Text,
        ..Default::default()
    });
    messages.push_back(live_chat::LiveChatMessage {
        id: "m2".into(),
        author_name: "Bob".into(),
        author_channel_id: None,
        author_avatar_url: None,
        badges: Vec::new(),
        text: "Great stream".into(),
        timestamp_text: None,
        kind: live_chat::LiveChatMessageKind::Text,
        ..Default::default()
    });

    let key1 = LiveChatFilterCacheKey {
        session_id: 7,
        message_count: messages.len(),
        seen_ids_len: 2,
        first_msg_id: "m1".into(),
        last_msg_id: "m2".into(),
        filter_query: "@mod".into(),
    };
    let first = resolve_live_chat_filter_cache(None, key1.clone(), &messages);
    assert_eq!(first.filtered_indices, vec![0]);

    let reused = resolve_live_chat_filter_cache(Some(&first), key1.clone(), &messages);
    assert!(Arc::ptr_eq(&first, &reused));

    // New message invalidates cache.
    messages.push_back(live_chat::LiveChatMessage {
        id: "m3".into(),
        author_name: "Carol".into(),
        author_channel_id: None,
        author_avatar_url: None,
        badges: vec![live_chat::LiveChatBadgeKind::Moderator],
        text: "Pinned link".into(),
        timestamp_text: None,
        kind: live_chat::LiveChatMessageKind::Text,
        ..Default::default()
    });
    let key2 = LiveChatFilterCacheKey {
        session_id: 7,
        message_count: messages.len(),
        seen_ids_len: 3,
        first_msg_id: "m1".into(),
        last_msg_id: "m3".into(),
        filter_query: "@mod".into(),
    };
    let updated = resolve_live_chat_filter_cache(Some(&first), key2, &messages);
    assert!(!Arc::ptr_eq(&first, &updated));
    assert_eq!(updated.filtered_indices, vec![0, 2]);
}

#[test]
fn live_chat_visible_range_culls_offscreen_messages_with_exact_spacers() {
    let heights = vec![36.0; 120];
    let gap = 5.0;
    let expected_total = 120.0 * 36.0 + 119.0 * gap;
    let (range, top_h, bottom_h) = live_chat_visible_range(&heights, gap, 0.0, 1200.0, 1600.0, 2);
    assert!(range.start > 20);
    assert!(range.end < 60);
    let rendered_count = range.end - range.start;
    let rendered_h = rendered_count as f32 * 36.0 + rendered_count.saturating_sub(1) as f32 * gap;
    assert!((top_h + rendered_h + bottom_h - expected_total).abs() < 0.01);
}

#[test]
fn audio_track_poll_policy_avoids_per_frame_polling_while_menu_open_or_closed() {
    // Menu closed: never polls even after long intervals.
    assert!(!should_poll_audio_tracks(
        false,
        true,
        None,
        None,
        false,
        Some(Duration::from_secs(60)),
    ));
    // Opening AudioTracks menu triggers an immediate single refresh.
    assert!(should_poll_audio_tracks(
        false,
        true,
        Some(overlay::OverlayMenu::AudioTracks),
        Some(overlay::OverlayMenu::Root),
        false,
        Some(Duration::from_millis(10)),
    ));
    // Next frame while AudioTracks remains open does NOT re-poll.
    assert!(!should_poll_audio_tracks(
        false,
        true,
        Some(overlay::OverlayMenu::AudioTracks),
        Some(overlay::OverlayMenu::AudioTracks),
        false,
        Some(Duration::from_millis(16)),
    ));
    // After cooldown elapses while AudioTracks is open, refresh is permitted.
    assert!(should_poll_audio_tracks(
        false,
        true,
        Some(overlay::OverlayMenu::AudioTracks),
        Some(overlay::OverlayMenu::AudioTracks),
        false,
        Some(Duration::from_secs(2)),
    ));
}

#[test]
fn transcript_display_cache_reuses_and_invalidates_on_revision_or_chapters() {
    let mut app = FastTube::default();
    app.watch.extras.transcript = vec![
        transcript::Cue {
            start_ms: 0,
            end_ms: 3_000,
            text: "Introductory segment".into(),
        },
        transcript::Cue {
            start_ms: 60_000,
            end_ms: 64_000,
            text: "Deep dive segment".into(),
        },
    ];
    app.watch.extras.transcript_revision = 1;
    app.sync_transcript_display_cache();
    assert!(app.watch.extras.transcript_display_cache.valid);
    assert!(!app.watch.extras.transcript_display_cache.items.is_empty());

    // Populate a measured height sentinel and confirm cache hit preserves it without rebuild.
    app.watch.extras.transcript_display_cache.item_heights[0] = Some((320, true, 48.0));
    app.sync_transcript_display_cache();
    assert_eq!(
        app.watch.extras.transcript_display_cache.item_heights[0],
        Some((320, true, 48.0))
    );

    // Changing chapters invalidates the cached display projection and resets item_heights.
    app.watch.metadata_cache.description_chapters = vec![
        watch_extras::DescriptionChapter {
            title: "Intro".into(),
            label: "0:00".into(),
            seconds: 0,
        },
        watch_extras::DescriptionChapter {
            title: "Deep dive".into(),
            label: "1:00".into(),
            seconds: 60,
        },
    ];
    app.sync_transcript_display_cache();
    assert!(
        app.watch.extras.transcript_display_cache.item_heights[0].is_none(),
        "chapter change must invalidate and rebuild transcript display cache"
    );
    let items_with_chapters = app.watch.extras.transcript_display_cache.items.len();
    assert!(items_with_chapters >= 4);

    // Bumping transcript_revision also invalidates cleanly.
    app.watch.extras.transcript_display_cache.item_heights[0] = Some((320, true, 42.0));
    app.watch.extras.transcript_revision = 2;
    app.sync_transcript_display_cache();
    assert!(app.watch.extras.transcript_display_cache.item_heights[0].is_none());
}

#[test]
fn transcript_search_visible_range_culls_long_cue_lists_with_exact_spacers() {
    let cues: Vec<transcript::Cue> = (0..250)
        .map(|i| transcript::Cue {
            start_ms: (i as u64) * 5_000,
            end_ms: (i as u64 + 1) * 5_000,
            text: format!("Transcript cue line {i} covering video playback optimization"),
        })
        .collect();
    let heights: Vec<f32> = cues
        .iter()
        .map(|cue| estimate_transcript_segment_height(cue, 340.0, true, 14.0))
        .collect();
    let gap = 4.0;
    let expected_total: f32 =
        heights.iter().sum::<f32>() + (heights.len().saturating_sub(1) as f32) * gap;
    let (range, top_h, bottom_h) =
        compute_variable_visible_range(&heights, gap, 0.0, 2400.0, 2860.0, 4);
    assert!(range.start > 30);
    assert!(range.end < 120);
    let rendered_h: f32 = heights[range.clone()].iter().sum::<f32>()
        + ((range.end - range.start).saturating_sub(1) as f32) * gap;
    assert!((top_h + rendered_h + bottom_h - expected_total).abs() < 0.05);
}

#[test]
fn can_view_watch_comments_allows_oauth_free_innertube_comments() {
    let mut extras = watch_extras::WatchExtras::default();
    assert!(!can_view_watch_comments(&extras, false));
    assert!(can_view_watch_comments(&extras, true));

    extras.innertube_comments_continuation = Some("cont_token".into());
    assert!(can_view_watch_comments(&extras, false));

    extras.innertube_comments_continuation = None;
    extras.innertube_comments_loading = true;
    assert!(can_view_watch_comments(&extras, false));

    extras.innertube_comments_loading = false;
    extras.comments.push(oauth::Comment {
        id: "c1".into(),
        author: "@user".into(),
        author_avatar: None,
        text: "Public comment without OAuth".into(),
        published: "há 1h".into(),
        likes: 10,
        replies: 0,
    });
    assert!(can_view_watch_comments(&extras, false));
}

#[test]
fn resolve_watch_heatmap_points_prefers_innertube_and_caches_fallback() {
    let mut extras = watch_extras::WatchExtras::default();
    let live_info = live_chat::WatchLiveInfo {
        heatmap: vec![
            live_chat::HeatmapMarker {
                start_ms: 0,
                duration_ms: 10_000,
                intensity: 0.4,
            },
            live_chat::HeatmapMarker {
                start_ms: 10_000,
                duration_ms: 10_000,
                intensity: 0.9,
            },
        ],
        ..Default::default()
    };

    // Fallback to live_info.heatmap when extras.heatmap is empty.
    let ptr_fallback_1 = {
        let pts = resolve_watch_heatmap_points(&mut extras, Some(&live_info), 1);
        assert_eq!(pts.len(), 2);
        assert!((pts[0].start_secs - 0.0).abs() < 1e-6);
        assert!((pts[0].duration_secs - 10.0).abs() < 1e-6);
        assert!((pts[1].start_secs - 10.0).abs() < 1e-6);
        assert!((pts[1].intensity - 0.9).abs() < 1e-6);
        pts.as_ptr()
    };
    // Same (live_info_revision, extras.heatmap.len()) reuses cached allocation without reallocating.
    let ptr_fallback_2 = resolve_watch_heatmap_points(&mut extras, Some(&live_info), 1).as_ptr();
    assert_eq!(ptr_fallback_1, ptr_fallback_2);

    // Populating InnerTube watch/next heatmap overrides the fallback and invalidates key.
    extras.heatmap = vec![overlay::HeatmapPoint {
        start_secs: 0.0,
        duration_secs: 5.0,
        intensity: 0.75,
    }];
    let pts_it = resolve_watch_heatmap_points(&mut extras, Some(&live_info), 1);
    assert_eq!(pts_it.len(), 1);
    assert!((pts_it[0].intensity - 0.75).abs() < 1e-6);
}

#[test]
fn manifest_overlay_audio_tracks_selects_preferred_or_default_track() {
    let now = std::time::Instant::now();
    let manifest = stream_manifest::StreamManifest {
        video_id: "vid00000001".into(),
        title: "Multi-lang Video".into(),
        channel: "Creator".into(),
        channel_id: Some("UC1111111111111111111111".into()),
        channel_verified: true,
        duration_secs: Some(300.0),
        is_live: false,
        upload_date: None,
        video_formats: Vec::new(),
        audio_formats: vec![
            stream_manifest::AudioStreamFormat {
                format_id: "251-en".into(),
                url: "https://example.com/en.webm".into(),
                acodec: "opus".into(),
                abr_kbps: Some(128.0),
                ext: "webm".into(),
                language: Some("en".into()),
                language_label: "English (Original)".into(),
                is_default: true,
                is_dubbed: false,
                format_note: Some("original".into()),
                http_headers: HashMap::new(),
            },
            stream_manifest::AudioStreamFormat {
                format_id: "251-pt".into(),
                url: "https://example.com/pt.webm".into(),
                acodec: "opus".into(),
                abr_kbps: Some(128.0),
                ext: "webm".into(),
                language: Some("pt-BR".into()),
                language_label: "Português (Brasil) (Dubbed)".into(),
                is_default: false,
                is_dubbed: true,
                format_note: Some("dubbed".into()),
                http_headers: HashMap::new(),
            },
        ],
        muxed_formats: Vec::new(),
        chapters: Vec::new(),
        captions: Vec::new(),
        storyboard: None,
        fetched_at: now,
        expires_at: now + std::time::Duration::from_secs(600),
    };

    let default_tracks = manifest_overlay_audio_tracks(&manifest, None);
    assert_eq!(default_tracks.len(), 2);
    assert_eq!(default_tracks[0].id, "en");
    assert!(default_tracks[0].selected);
    assert!(!default_tracks[1].selected);

    let pt_tracks = manifest_overlay_audio_tracks(&manifest, Some("pt"));
    assert_eq!(pt_tracks.len(), 2);
    assert!(!pt_tracks[0].selected);
    assert_eq!(pt_tracks[1].id, "pt-BR");
    assert!(pt_tracks[1].selected);
}

#[test]
fn ensure_watch_sidebar_cache_prioritizes_real_related_videos_and_respects_from_channel() {
    let mut app = FastTube::default();
    let current = sample_video("cur00000001", "Canal Alpha");
    let feed_vid = sample_video("feed0000001", "Canal Gamma");
    let rel_other = sample_video("rel00000001", "Canal Beta");
    let rel_same = sample_video("rel00000002", "Canal Alpha");

    app.selected = Some(current);
    app.feed.videos = vec![feed_vid];
    app.watch.extras.related_videos = vec![rel_other.clone(), rel_same.clone()];
    app.watch.extras.sidebar_tab = watch_extras::SidebarTab::All;

    app.ensure_watch_sidebar_cache();
    assert_eq!(app.watch.sidebar_cache.cards.len(), 2);
    assert_eq!(app.watch.sidebar_cache.cards[0].video.url, rel_other.url);
    assert_eq!(app.watch.sidebar_cache.cards[1].video.url, rel_same.url);

    // Switching to "From channel" filters locally to the active channel while keeping real related items.
    app.watch.extras.sidebar_tab = watch_extras::SidebarTab::FromChannel;
    app.ensure_watch_sidebar_cache();
    assert_eq!(app.watch.sidebar_cache.cards.len(), 1);
    assert_eq!(app.watch.sidebar_cache.cards[0].video.url, rel_same.url);
}

#[test]
fn player_click_zone_and_single_click_debounce_distinguish_left_right_center_and_single() {
    let rect = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1000.0, 560.0));
    assert_eq!(
        classify_player_click_zone(150.0, rect),
        PlayerClickZone::Left
    );
    assert_eq!(
        classify_player_click_zone(600.0, rect),
        PlayerClickZone::Center
    );
    assert_eq!(
        classify_player_click_zone(950.0, rect),
        PlayerClickZone::Right
    );

    let t0 = Instant::now();
    let mut debounce = PlayerClickDebounceState::default();
    debounce.on_single_click(t0);
    assert!(debounce.is_pending());
    // Before debounce window expires, no toggle fires yet.
    assert_eq!(
        debounce.poll_ready_single_click(t0 + Duration::from_millis(80)),
        PlayerPointerGesture::None
    );
    // Double-click cancels pending single click and routes by zone cleanly.
    assert_eq!(
        debounce.on_double_click(PlayerClickZone::Left),
        PlayerPointerGesture::SeekBack10
    );
    assert!(!debounce.is_pending());
    assert_eq!(
        debounce.on_double_click(PlayerClickZone::Right),
        PlayerPointerGesture::SeekForward10
    );
    assert_eq!(
        debounce.on_double_click(PlayerClickZone::Center),
        PlayerPointerGesture::ToggleFullscreen
    );

    // Uninterrupted single click fires TogglePlay after debounce window.
    debounce.on_single_click(t0);
    assert_eq!(
        debounce.poll_ready_single_click(t0 + DOUBLE_CLICK_DEBOUNCE_WINDOW),
        PlayerPointerGesture::TogglePlay
    );
    assert!(!debounce.is_pending());
}

#[test]
fn hold_speed_boost_activates_2x_after_threshold_and_restores_prior_speed() {
    let t0 = Instant::now();
    let mut hold = HoldSpeedBoostState::default();

    // Initial press does not immediately jump to 2x.
    assert_eq!(hold.update(true, 1.25, t0), HoldSpeedTransition::None);
    assert!(!hold.active);

    // Quick tap release under threshold does not restore or suppress click.
    assert_eq!(
        hold.update(false, 1.25, t0 + Duration::from_millis(120)),
        HoldSpeedTransition::None
    );
    assert!(!hold.take_suppressed_click());

    // Holding past threshold activates 2x and stores previous speed.
    assert_eq!(hold.update(true, 1.25, t0), HoldSpeedTransition::None);
    assert_eq!(
        hold.update(true, 1.25, t0 + HOLD_SPEED_THRESHOLD),
        HoldSpeedTransition::Activate2x
    );
    assert!(hold.active);

    // Releasing restores prior 1.25x speed and marks click suppressed.
    assert_eq!(
        hold.update(
            false,
            2.0,
            t0 + HOLD_SPEED_THRESHOLD + Duration::from_millis(500)
        ),
        HoldSpeedTransition::Restore(1.25)
    );
    assert!(!hold.active);
    assert!(hold.take_suppressed_click());
    assert!(!hold.take_suppressed_click());
}

#[test]
fn shorts_wheel_snap_debounces_bursts_and_snaps_both_directions() {
    let t0 = Instant::now();
    let mut snap = ShortsWheelSnapState::default();

    // Small sub-threshold scroll accumulates until threshold triggers Next.
    assert_eq!(
        snap.feed_wheel_delta(-10.0, t0),
        ShortsWheelSnapDirection::None
    );
    assert_eq!(
        snap.feed_wheel_delta(-12.0, t0 + Duration::from_millis(20)),
        ShortsWheelSnapDirection::Next
    );

    // Immediate inertia events within cooldown are suppressed.
    assert_eq!(
        snap.feed_wheel_delta(-40.0, t0 + Duration::from_millis(100)),
        ShortsWheelSnapDirection::None
    );

    // After cooldown elapses, upward wheel snaps to Previous.
    let t_after = t0 + Duration::from_millis(20) + SHORTS_WHEEL_SNAP_COOLDOWN;
    assert_eq!(
        snap.feed_wheel_delta(24.0, t_after),
        ShortsWheelSnapDirection::Previous
    );
}

#[test]
fn sleep_timer_tick_handles_countdown_expiry_and_end_of_video() {
    let t0 = Instant::now();
    assert_eq!(
        evaluate_sleep_timer_tick(SleepTimerTarget::Off, false, t0),
        SleepTimerTickOutcome::Idle
    );
    let deadline = t0 + Duration::from_secs(90);
    assert_eq!(
        evaluate_sleep_timer_tick(SleepTimerTarget::Until(deadline), false, t0),
        SleepTimerTickOutcome::Active {
            remaining_secs: Some(90),
        }
    );
    assert_eq!(
        evaluate_sleep_timer_tick(SleepTimerTarget::Until(deadline), false, deadline),
        SleepTimerTickOutcome::ExpiredPause
    );
    assert_eq!(
        evaluate_sleep_timer_tick(SleepTimerTarget::EndOfVideo, false, t0),
        SleepTimerTickOutcome::Active {
            remaining_secs: None,
        }
    );
    assert_eq!(
        evaluate_sleep_timer_tick(SleepTimerTarget::EndOfVideo, true, t0),
        SleepTimerTickOutcome::ExpiredPause
    );
}

#[test]
fn up_next_countdown_counts_down_cancels_and_triggers_after_five_seconds() {
    let t0 = Instant::now();
    match evaluate_up_next_countdown(t0, false, t0 + Duration::from_secs(2)) {
        UpNextCountdownTick::Counting {
            remaining_secs,
            progress,
        } => {
            assert!((remaining_secs - 3.0).abs() < 0.05);
            assert!((progress - 0.4).abs() < 0.05);
        }
        other => panic!("expected Counting, got {other:?}"),
    }
    assert_eq!(
        evaluate_up_next_countdown(t0, true, t0 + Duration::from_secs(6)),
        UpNextCountdownTick::Inactive
    );
    assert_eq!(
        evaluate_up_next_countdown(t0, false, t0 + Duration::from_secs(5)),
        UpNextCountdownTick::TriggerNext
    );
}

#[test]
fn wheel_volume_target_steps_clamps_and_unmutes_cleanly() {
    assert_eq!(compute_wheel_volume_target(50.0, false, 0.2, 5.0), None);
    assert_eq!(
        compute_wheel_volume_target(50.0, false, 12.0, 5.0),
        Some(55.0)
    );
    assert_eq!(
        compute_wheel_volume_target(98.0, false, 12.0, 5.0),
        Some(100.0)
    );
    assert_eq!(
        compute_wheel_volume_target(40.0, true, 12.0, 5.0),
        Some(5.0)
    );
    assert_eq!(
        compute_wheel_volume_target(3.0, false, -12.0, 5.0),
        Some(0.0)
    );
}

#[test]
fn live_chat_replay_filter_syncs_messages_to_playback_position() {
    let offsets = vec![Some(5_000), Some(15_000), None, Some(30_000)];
    assert_eq!(
        filter_live_chat_replay_indices(&offsets, 10_000),
        vec![0, 2]
    );
    assert_eq!(
        filter_live_chat_replay_indices(&offsets, 15_000),
        vec![0, 1, 2]
    );
    assert_eq!(
        filter_live_chat_replay_indices(&offsets, 45_000),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn is_own_comment_matches_channel_title_and_handle_case_insensitively() {
    let handle = ChannelTarget::Handle("@FastTubeDev".into());
    assert!(is_own_comment("@fasttubedev", None, Some(&handle)));
    assert!(is_own_comment("FastTube Dev", Some("fasttube dev"), None));
    assert!(!is_own_comment(
        "@OtherUser",
        Some("FastTube Dev"),
        Some(&handle)
    ));
    assert!(!is_own_comment("", Some("FastTube Dev"), Some(&handle)));
}

#[test]
fn in_app_mini_player_compact_size_and_overlay_layout_have_no_overlaps() {
    for viewport_w in [480.0_f32, 900.0, 1280.0, 1440.0, 1920.0, 2560.0] {
        let size = FastTube::compute_in_app_mini_player_size(viewport_w);
        assert!(size.x >= 216.0 && size.x <= 336.0, "width {size:?}");
        assert!(size.y >= 121.0 && size.y <= 189.0, "height {size:?}");
        let vrect = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), size);
        let layout = MiniPlayerOverlayLayout::compute(vrect);

        let all_rects = [
            layout.top_scrim,
            layout.bottom_scrim,
            layout.title,
            layout.top_expand,
            layout.top_pip,
            layout.top_close,
            layout.center_prev,
            layout.center_play,
            layout.center_next,
            layout.bottom_mute,
            layout.bottom_volume,
            layout.bottom_time,
            layout.seek_hit,
        ];
        for r in all_rects {
            assert!(
                vrect.contains_rect(r),
                "rect {r:?} escaped video frame {vrect:?} at viewport {viewport_w}"
            );
        }

        let interactive = [
            ("title", layout.title),
            ("top_expand", layout.top_expand),
            ("top_pip", layout.top_pip),
            ("top_close", layout.top_close),
            ("center_prev", layout.center_prev),
            ("center_play", layout.center_play),
            ("center_next", layout.center_next),
            ("bottom_mute", layout.bottom_mute),
            ("bottom_volume", layout.bottom_volume),
            ("bottom_time", layout.bottom_time),
            ("seek_hit", layout.seek_hit),
        ];
        for i in 0..interactive.len() {
            for j in (i + 1)..interactive.len() {
                let (name_a, rect_a) = interactive[i];
                let (name_b, rect_b) = interactive[j];
                assert!(
                    !rect_a.intersects(rect_b),
                    "{name_a} ({rect_a:?}) overlaps {name_b} ({rect_b:?}) at viewport {viewport_w}"
                );
            }
        }
    }
}

#[test]
fn shorts_formatting_helpers_format_counts_views_dates_and_handles() {
    assert_eq!(format_short_compact_count(850, true), "850");
    assert_eq!(format_short_compact_count(7_500, true), "7,5 mil");
    assert_eq!(format_short_compact_count(7_500, false), "7.5K");
    assert_eq!(format_short_compact_count(38_000, true), "38 mil");
    assert_eq!(format_short_compact_count(1_200_000, true), "1,2 mi");
    assert_eq!(format_short_compact_count(1_200_000, false), "1.2M");

    assert_eq!(
        strip_views_word("38 mil visualiza\u{00e7}\u{00f5}es"),
        "38 mil"
    );
    assert_eq!(strip_views_word("\u{25b7} 1.4M views"), "1.4M");

    assert_eq!(
        format_short_stat_date(None, Some("h\u{00e1} 3 horas"), None, true),
        ("3 h".into(), "Atr\u{00e1}s".into())
    );
    assert_eq!(
        format_short_stat_date(None, Some("2 days ago"), None, false),
        ("2 d".into(), "Ago".into())
    );
    assert_eq!(
        format_short_stat_date(Some("14 de nov. de 2026"), None, None, true),
        ("14 nov.".into(), "2026".into())
    );
    assert_eq!(
        format_short_stat_date(None, None, Some("20261114"), true),
        ("14 nov.".into(), "2026".into())
    );

    assert_eq!(
        resolve_short_channel_handle(
            Some(&ChannelTarget::Handle("CienciaTodoDia".into())),
            "Ci\u{00ea}ncia Todo Dia"
        ),
        "@CienciaTodoDia"
    );
    assert_eq!(
        resolve_short_channel_handle(
            Some(&ChannelTarget::ChannelId("UC1234567890123456789012".into())),
            "Ci\u{00ea}ncia Todo Dia"
        ),
        "Ci\u{00ea}ncia Todo Dia"
    );
}

#[test]
fn shorts_watch_clicking_title_opens_description_panel_without_remix_or_promo() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = Video {
        title: "GTAVI Short Imperdivel".into(),
        channel: "Ciencia Todo Dia".into(),
        url: "https://www.youtube.com/watch?v=shortbykind1".into(),
        thumbnail: String::new(),
        duration: "0:42".into(),
    };
    app.video_metadata.insert(
        video.url.clone(),
        VideoMetadata {
            kind: search::SearchResultKind::Short,
            channel_url: Some("https://www.youtube.com/@CienciaTodoDia".into()),
            channel_verified: true,
            view_count_text: Some("38 mil visualiza\u{00e7}\u{00f5}es".into()),
            published_time_text: Some("h\u{00e1} 3 horas".into()),
            description: Some("Para celebrar o lancamento #YouTubePartner".into()),
            ..Default::default()
        },
    );
    app.selected = Some(video.clone());
    app.watch.extras.sync_video(Some(&video.url));
    app.watch.extras.like_count_text = Some("7,5 mil".into());
    app.watch.extras.comments_total = Some(105);

    // SearchResultKind::Short opens global Shorts player even on a /watch?v= URL and Home tab.
    assert!(app.is_watch_short_mode());

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 760.0));
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let _ = app.shorts_watch_page_ui(ui, &ctx);
            },
        );
        out.textures_delta.clear();
    }

    let mut initial_text = Vec::new();
    let mut title_pos = None;
    fn gather_text(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => {
                out.push((t.galley.text().to_owned(), t.visual_bounding_rect()))
            }
            egui::Shape::Vec(items) => {
                for item in items {
                    gather_text(item, out);
                }
            }
            _ => {}
        }
    }
    for c in &out.shapes {
        gather_text(&c.shape, &mut initial_text);
    }
    for (text, rect) in &initial_text {
        if text.contains("GTAVI Short Imperdivel") {
            title_pos = Some(rect.center());
        }
    }
    let rendered_initial = initial_text
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered_initial.contains("@CienciaTodoDia"),
        "{rendered_initial}"
    );
    assert!(rendered_initial.contains("7,5 mil"), "{rendered_initial}");
    assert!(rendered_initial.contains("105"), "{rendered_initial}");
    assert!(!rendered_initial.contains("Remix"), "{rendered_initial}");
    assert!(!rendered_initial.contains("promo"), "{rendered_initial}");

    let click_title = title_pos.expect("clickable short title rendered");
    let mut out2 = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(click_title),
                egui::Event::PointerButton {
                    pos: click_title,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_title,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let _ = app.shorts_watch_page_ui(ui, &ctx);
        },
    );
    out2.textures_delta.clear();
    assert_eq!(
        app.watch.shorts_side_panel,
        app_state::ShortsSidePanel::Description
    );

    let mut out3 = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let _ = app.shorts_watch_page_ui(ui, &ctx);
        },
    );
    out3.textures_delta.clear();

    let mut desc_text = Vec::new();
    for c in &out3.shapes {
        gather_text(&c.shape, &mut desc_text);
    }
    let rendered_desc = desc_text
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered_desc.contains("Marca\u{00e7}\u{00f5}es Gostei"),
        "{rendered_desc}"
    );
    assert!(rendered_desc.contains("38 mil"), "{rendered_desc}");
    assert!(
        rendered_desc.contains("Visualiza\u{00e7}\u{00f5}es"),
        "{rendered_desc}"
    );
    assert!(
        rendered_desc.contains("Para celebrar o lancamento"),
        "{rendered_desc}"
    );
}

#[test]
fn format_watch_metadata_stats_uses_published_time_and_live_info_fallbacks() {
    let video = Video {
        title: "Watch Stats Test".into(),
        channel: "Creator".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
        duration: "10:00".into(),
    };
    let meta = VideoMetadata {
        view_count: Some(12_345),
        published_time_text: Some("há 7 meses".into()),
        ..Default::default()
    };
    let stats = FastTube::format_watch_metadata_stats(&video, Some(&meta), None, false);
    assert_eq!(stats.len(), 2);
    assert!(stats[1].contains("há 7 meses"));

    let live_info = live_chat::WatchLiveInfo {
        view_count_text: Some("8 mil visualizações".into()),
        relative_date_text: Some("há 2 h".into()),
        ..Default::default()
    };
    let fallback_stats =
        FastTube::format_watch_metadata_stats(&video, None, Some(&live_info), false);
    assert_eq!(
        fallback_stats,
        vec!["8 mil visualizações".to_owned(), "há 2 h".to_owned()]
    );
}

#[test]
fn volume_wheel_scroll_guard_blocks_scroll_through_until_pointer_moves_or_window_expires() {
    let t0 = Instant::now();
    let mut guard = VolumeWheelScrollGuard::default();
    let pos_vol = egui::pos2(180.0, 420.0);
    let pos_moved = egui::pos2(195.0, 420.0);

    // Entering and scrolling directly over the volume area is allowed immediately.
    assert!(guard.allow_volume_wheel(true, Some(pos_vol), true, t0));

    // Scrolling the page outside the volume area arms the scroll-through guard.
    assert!(!guard.allow_volume_wheel(false, Some(pos_vol), true, t0 + Duration::from_millis(10)));

    // If the volume bar slides under the exact same stationary pointer during the scroll burst,
    // volume wheel remains blocked and keeps extending while scrolling continues.
    assert!(!guard.allow_volume_wheel(true, Some(pos_vol), true, t0 + Duration::from_millis(80)));
    assert!(!guard.allow_volume_wheel(true, Some(pos_vol), true, t0 + Duration::from_millis(250)));

    // Moving the pointer intentionally (>2px) clears the guard immediately.
    assert!(guard.allow_volume_wheel(true, Some(pos_moved), true, t0 + Duration::from_millis(280)));

    // Re-arming the guard and waiting for the full window without moving also clears it.
    assert!(!guard.allow_volume_wheel(false, Some(pos_vol), true, t0 + Duration::from_millis(300)));
    assert!(!guard.allow_volume_wheel(true, Some(pos_vol), false, t0 + Duration::from_millis(500)));
    assert!(guard.allow_volume_wheel(
        true,
        Some(pos_vol),
        true,
        t0 + Duration::from_millis(300) + VOLUME_WHEEL_SCROLL_GUARD_WINDOW
    ));
}
