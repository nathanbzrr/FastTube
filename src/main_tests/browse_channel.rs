use super::super::*;
use super::helpers::*;

#[test]
fn sidebar_subscription_catalog_only_contains_oauth_subscriptions() {
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.ui.browse_section = BrowseSection::Subscriptions;
    app.feed.home.title = Some("Inscri\u{00e7}\u{00f5}es: envios recentes".into());
    let (req_id, _cancel, sender) = app.feed.home.stream_task.begin_manual();
    sender
        .send((
            req_id,
            HomeFetchMessage::Progress(oauth::SubscriptionProgress {
                items: Vec::new(),
                channels: vec![search::HomeChannel {
                    title: "Canal inscrito".into(),
                    channel_id: "UC000000001".into(),
                    avatar_url: None,
                }],
                completed_channels: 0,
                total_channels: 1,
                reached_limit: false,
            }),
        ))
        .unwrap();
    app.poll_async_tasks(&egui::Context::default());

    assert_eq!(app.feed.home.subscription_channels.len(), 1);
    assert_eq!(
        app.feed.home.subscription_channels[0].title,
        "Canal inscrito"
    );
    assert!(app.feed.home.subscription_channels_loaded);

    app.apply_search_results(
        &egui::Context::default(),
        vec![SearchResult {
            video: Video {
                title: "Resultado de busca".into(),
                channel: "Canal pesquisado".into(),
                url: "https://www.youtube.com/watch?v=search00001".into(),
                thumbnail: String::new(),
                duration: "1:00".into(),
            },
            metadata: VideoMetadata::default(),
        }],
    );
    app.ensure_feed_cache();

    assert_eq!(app.feed.home.subscription_channels.len(), 1);
    assert_eq!(
        app.feed.home.subscription_channels[0].title,
        "Canal inscrito"
    );
    assert_ne!(
        app.feed.home.subscription_channels[0].title,
        "Canal pesquisado"
    );
}

#[test]
fn feed_derived_cache_stays_clean_until_invalidated() {
    let mut app = FastTube::default();
    app.ensure_feed_cache();
    assert!(!app.feed.cache_dirty);
    let cached_topics = app.feed.cache.chips.clone();
    app.ensure_feed_cache();
    assert_eq!(app.feed.cache.chips, cached_topics);
    assert!(!app.feed.cache_dirty);
    app.invalidate_feed_cache();
    assert!(app.feed.cache_dirty);
}

#[test]
fn headless_ui_passes_cover_browse_side_by_side_and_stacked_watch_modes() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.feed.videos = sample_videos();
    app.ui.settings_open = true;

    let wide_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1180.0, 760.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(wide_input.clone(), |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            app.header_ui(ui, &ctx);
            app.navigation_ui(ui, &ctx);
            let _ = app.browse_ui(ui, &ctx);
        });
        app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();

    app.selected = Some(app.feed.videos[0].clone());
    app.playback.active_buffer = Some(BufferProfile::Default);
    let mut observed_mode_wide = None;
    let mut output = ctx.run_ui(wide_input, |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            let columns = compute_watch_columns(ui.available_width());
            observed_mode_wide = Some(columns.mode);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(columns.primary_width);
                    app.watch_primary_ui(
                        ui,
                        &ctx,
                        columns.primary_width,
                        ui.available_height(),
                        columns.mode,
                    );
                });
                ui.add_space(columns.gap);
                ui.vertical(|ui| {
                    ui.set_width(columns.secondary_width);
                    let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_wide_sidebar");
                });
            });
        });
        app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();
    assert_eq!(observed_mode_wide, Some(WatchLayoutMode::SideBySide));

    let narrow_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(680.0, 520.0),
        )),
        ..Default::default()
    };
    let mut observed_mode_narrow = None;
    let mut output = ctx.run_ui(narrow_input, |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            let columns = compute_watch_columns(ui.available_width());
            observed_mode_narrow = Some(columns.mode);
            app.watch_primary_ui(
                ui,
                &ctx,
                columns.primary_width,
                ui.available_height(),
                columns.mode,
            );
            let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_narrow_stacked");
        });
    });
    output.textures_delta.clear();
    assert_eq!(observed_mode_narrow, Some(WatchLayoutMode::Stacked));

    app.close_video(&ctx);
    assert!(app.selected.is_none());
    assert!(app.playback.active_buffer.is_none());
    assert!(!app.playback.window_visible);
    assert_eq!(app.feed.videos.len(), 2);
}

#[test]
fn optional_metadata_from_find_with_metadata_is_rendered_without_invented_values() {
    let _: fn(&str) -> Result<Vec<Video>, String> = search::find;
    let _: fn(&str) -> Result<Vec<SearchResult>, String> = search::find_with_metadata;

    assert_eq!(format_view_count(0), "0 visualizações");
    assert_eq!(format_view_count(1), "1 visualização");
    assert_eq!(format_view_count(1_234_567), "1.234.567 visualizações");
    assert_eq!(
        format_upload_date("20261106").as_deref(),
        Some("06/11/2026")
    );
    assert_eq!(format_upload_date("   "), None);
    assert_eq!(clean_description(Some("  ")), None);
    assert_eq!(
        clean_description(Some("  Descrição real do vídeo  ")).as_deref(),
        Some("Descrição real do vídeo")
    );

    let full_meta = VideoMetadata {
        view_count: Some(42_500),
        upload_date: Some("20261106".into()),
        description: Some("Linha 1\nLinha 2 com detalhes reais.".into()),
        ..Default::default()
    };
    assert_eq!(
        format_card_metadata_line("10:15", Some(&full_meta)).as_deref(),
        Some("10:15 • 42.500 visualizações • 06/11/2026")
    );

    let empty_meta = VideoMetadata {
        view_count: None,
        upload_date: None,
        description: None,
        ..Default::default()
    };
    assert_eq!(format_card_metadata_line("", Some(&empty_meta)), None);
    assert_eq!(
        format_card_metadata_line("4:30", Some(&empty_meta)).as_deref(),
        Some("4:30")
    );

    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let sample = sample_videos();
    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: sample[0].clone(),
                metadata: full_meta,
            },
            SearchResult {
                video: sample[1].clone(),
                metadata: empty_meta,
            },
        ],
    );
    assert_eq!(app.feed.videos.len(), 2);
    assert!(
        app.metadata_for(&sample[0].url)
            .unwrap()
            .view_count
            .is_some()
    );
    assert!(
        app.metadata_for(&sample[1].url)
            .unwrap()
            .view_count
            .is_none()
    );

    app.selected = Some(sample[0].clone());
    app.selected_metadata = app.metadata_for(&sample[0].url).cloned();
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1024.0, 720.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw_input, |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            let _ = app.browse_ui(ui, &ctx);
            app.watch_metadata_card_ui(ui, &sample[0], 640.0, Some("Reproduzindo em 1920 × 1080"));
            app.watch_metadata_card_ui(ui, &sample[1], 640.0, None);
        });
    });
    output.textures_delta.clear();
}

#[test]
fn channel_formatting_and_button_gating_only_accept_valid_metadata_targets() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);

    assert_eq!(format_subscriber_count(0), "0 inscritos");
    assert_eq!(format_subscriber_count(1), "1 inscrito");
    assert_eq!(format_subscriber_count(999), "999 inscritos");
    assert_eq!(format_subscriber_count(1_000), "1.000 inscritos");
    assert_eq!(format_subscriber_count(12_500), "12.500 inscritos");
    assert_eq!(format_subscriber_count(1_500_000), "1.500.000 inscritos");

    let uc_target = ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into());
    let handle_target = ChannelTarget::Handle("@RustLang".into());
    assert_eq!(
        channel_target_display_label(&uc_target),
        "UC38IQsAvIsxxjztdMZQtwHA"
    );
    assert_eq!(channel_target_display_label(&handle_target), "@RustLang");

    // Video without valid channel target (only display name or invalid IDs) must NOT get a target.
    let mut app = FastTube::default();
    let videos = valid_youtube_videos();
    let unresolvable_meta = VideoMetadata {
        channel_id: Some("not-a-valid-uc-id".into()),
        channel_url: Some("https://www.youtube.com/user/legacy".into()),
        ..Default::default()
    };
    let valid_meta = VideoMetadata {
        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
        channel_url: Some("https://www.youtube.com/@RustLang".into()),
        ..Default::default()
    };
    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: videos[0].clone(),
                metadata: unresolvable_meta.clone(),
            },
            SearchResult {
                video: videos[1].clone(),
                metadata: valid_meta.clone(),
            },
        ],
    );

    assert_eq!(app.channel_target_for_video(&videos[0]), None);
    assert_eq!(
        app.channel_target_for_video(&videos[1]),
        Some(ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into()))
    );

    // Attempt to open channel from unresolvable metadata must fail safely without inventing a target.
    assert!(!app.open_channel_for_metadata(&unresolvable_meta, &ctx));
    assert!(app.channels.error.is_some());
    assert!(!app.channels.view_open);
    assert!(!app.is_channel_loading());

    // Forged/invalid ChannelTarget enum value passed directly to begin_channel_request is rejected.
    assert!(
        app.begin_channel_request(ChannelTarget::Handle("invalid_no_at".into()), &ctx)
            .is_none()
    );
    assert!(app.channels.error.is_some());

    // Valid metadata opens channel request cleanly.
    let (req_id, resolved_target) = app
        .begin_channel_request(
            search::channel_target_for_metadata(&valid_meta).unwrap(),
            &ctx,
        )
        .expect("valid channel target should start request");
    assert_eq!(req_id, 1);
    assert_eq!(
        resolved_target,
        ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into())
    );
    assert!(app.channels.view_open);
    assert!(app.is_channel_loading());
    assert!(app.channels.error.is_none());
}

#[test]
fn channel_page_displays_real_data_without_inventing_missing_fields() {
    let full_page = ChannelPage {
        target: ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into()),
        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
        title: Some("Blender Foundation".into()),
        handle: Some("@BlenderOfficial".into()),
        canonical_url: "https://www.youtube.com/channel/UC38IQsAvIsxxjztdMZQtwHA".into(),
        description: Some("Open source 3D creation.\n\n\n Official channel.".into()),
        subscriber_count: Some(1_250_000),
        view_count: None,
        avatar_url: Some("https://yt3.googleusercontent.com/avatar.jpg".into()),
        banner_url: Some("https://yt3.googleusercontent.com/banner.jpg".into()),
        verified: Some(true),
        tags: Vec::new(),
        videos: Vec::new(),
        ..Default::default()
    };
    assert_eq!(channel_page_display_title(&full_page), "Blender Foundation");
    assert_eq!(
        format_channel_metadata_line(&full_page).as_deref(),
        Some("@BlenderOfficial • 1.250.000 inscritos • ID: UC38IQsAvIsxxjztdMZQtwHA • 0 vídeos")
    );
    assert_eq!(
        format_channel_media_line(&full_page).as_deref(),
        Some("Avatar informado • Banner informado")
    );

    let sparse_page = ChannelPage {
        target: ChannelTarget::Handle("@SparseCreator".into()),
        channel_id: None,
        title: None,
        handle: None,
        canonical_url: "https://www.youtube.com/@SparseCreator".into(),
        description: None,
        subscriber_count: None,
        view_count: None,
        avatar_url: None,
        banner_url: None,
        verified: None,
        tags: Vec::new(),
        videos: Vec::new(),
        ..Default::default()
    };
    assert_eq!(channel_page_display_title(&sparse_page), "@SparseCreator");
    assert_eq!(
        format_channel_metadata_line(&sparse_page).as_deref(),
        Some("0 vídeos")
    );
    assert_eq!(format_channel_media_line(&sparse_page), None);
}

#[test]
fn concurrent_searches_and_channel_fetches_ignore_stale_results_and_handle_errors() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let mut videos = valid_youtube_videos();
    videos.push(Video {
        title: "Vídeo YouTube Três".into(),
        channel: "Canal Gama".into(),
        url: "https://www.youtube.com/watch?v=3JZ_D3ELwOQ".into(),
        thumbnail: String::new(),
        duration: "5:45".into(),
    });

    // 1. Concurrent searches: start search 1, then search 2 before search 1 finishes.
    app.feed.query = "primeira busca".into();
    let (search_req_1, q1) = app.begin_search_request().unwrap();
    assert_eq!(q1, "primeira busca");
    let (tx_s1, rx_s1) = mpsc::channel();
    app.feed
        .search
        .task
        .attach_receiver(search_req_1, process::Cancellation::default(), rx_s1);

    app.feed.query = "segunda busca".into();
    let (search_req_2, q2) = app.begin_search_request().unwrap();
    assert_eq!(q2, "segunda busca");
    assert!(search_req_2 > search_req_1);
    let (tx_s2, rx_s2) = mpsc::channel();
    app.feed
        .search
        .task
        .attach_receiver(search_req_2, process::Cancellation::default(), rx_s2);

    // Stale search 1 result arrives via complete_search_request -> rejected.
    let _ = tx_s1.send((
        search_req_1,
        SearchFetchMessage {
            request_id: search_req_1,
            result: Ok(vec![SearchResult {
                video: videos[0].clone(),
                metadata: VideoMetadata::default(),
            }]),
        },
    ));
    assert!(!app.complete_search_request(
        search_req_1,
        Ok(vec![SearchResult {
            video: videos[0].clone(),
            metadata: VideoMetadata::default(),
        }]),
        &ctx,
    ));
    assert!(app.feed.videos.is_empty());
    assert_eq!(app.feed.search.task.active_request_id(), Some(search_req_2));

    // Active search 2 arrives via channel polling -> accepted.
    tx_s2
        .send((
            search_req_2,
            SearchFetchMessage {
                request_id: search_req_2,
                result: Ok(vec![SearchResult {
                    video: videos[1].clone(),
                    metadata: VideoMetadata {
                        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
                        view_count: Some(500),
                        ..Default::default()
                    },
                }]),
            },
        ))
        .unwrap();
    app.poll_async_tasks(&ctx);
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].url, videos[1].url);
    assert!(!app.feed.search.task.is_running());
    assert!(app.feed.search.task.active_request_id().is_none());

    // 2. Concurrent channel fetches: start channel A, then channel B before A finishes.
    let target_a = ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into());
    let target_b = ChannelTarget::Handle("@RustLang".into());

    let (chan_req_a, valid_a) = app.begin_channel_request(target_a.clone(), &ctx).unwrap();
    let (tx_ca, rx_ca) = mpsc::channel();
    app.channels
        .videos
        .task
        .attach_receiver(chan_req_a, process::Cancellation::default(), rx_ca);

    let (chan_req_b, valid_b) = app.begin_channel_request(target_b.clone(), &ctx).unwrap();
    assert!(chan_req_b > chan_req_a);
    let (tx_cb, rx_cb) = mpsc::channel();
    app.channels
        .videos
        .task
        .attach_receiver(chan_req_b, process::Cancellation::default(), rx_cb);

    let page_a = ChannelPage {
        target: valid_a.clone(),
        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
        title: Some("Canal Antigo Obsoleto".into()),
        handle: Some("@OldChannel".into()),
        canonical_url: valid_a.canonical_url(),
        description: None,
        subscriber_count: Some(10),
        view_count: None,
        avatar_url: None,
        banner_url: None,
        verified: Some(false),
        tags: Vec::new(),
        videos: vec![SearchResult {
            video: videos[0].clone(),
            metadata: VideoMetadata::default(),
        }],
        ..Default::default()
    };
    let page_b = ChannelPage {
        target: valid_b.clone(),
        channel_id: Some("UCaYhcUwRBNscFNUKTjgPFiA".into()),
        title: Some("Rust Official".into()),
        handle: Some("@RustLang".into()),
        canonical_url: valid_b.canonical_url(),
        description: Some("Canal oficial.".into()),
        subscriber_count: Some(42_000),
        view_count: None,
        avatar_url: Some("https://yt3.googleusercontent.com/rust.jpg".into()),
        banner_url: None,
        verified: Some(true),
        tags: Vec::new(),
        videos: vec![SearchResult {
            video: videos[2].clone(),
            metadata: VideoMetadata {
                channel_id: Some("UCaYhcUwRBNscFNUKTjgPFiA".into()),
                channel_url: Some("https://www.youtube.com/@RustLang".into()),
                view_count: Some(7_777),
                upload_date: Some("20261106".into()),
                description: Some("Vídeo do canal Rust.".into()),
                ..Default::default()
            },
        }],
        ..Default::default()
    };

    // Stale response A must be ignored.
    let _ = tx_ca.send((
        chan_req_a,
        ChannelFetchMessage {
            request_id: chan_req_a,
            target: valid_a.clone(),
            result: Ok(search::ChannelFetchOutcome {
                page: page_a.clone(),
                from_cache: false,
            }),
        },
    ));
    assert!(!app.complete_channel_request(chan_req_a, &valid_a, Ok(page_a), &ctx));
    assert!(app.channels.page.is_none());
    assert!(app.is_channel_loading());

    // Active response B arrives via poll_async_tasks -> applied.
    tx_cb
        .send((
            chan_req_b,
            ChannelFetchMessage {
                request_id: chan_req_b,
                target: valid_b.clone(),
                result: Ok(search::ChannelFetchOutcome {
                    page: page_b,
                    from_cache: false,
                }),
            },
        ))
        .unwrap();
    app.poll_async_tasks(&ctx);

    assert!(!app.is_channel_loading());
    assert!(app.channels.error.is_none());
    let loaded_page = app.channels.page.as_ref().expect("channel page B loaded");
    assert_eq!(loaded_page.title.as_deref(), Some("Rust Official"));
    assert_eq!(loaded_page.videos.len(), 1);
    assert_eq!(loaded_page.videos[0].video.url, videos[2].url);

    // Channel video metadata is accessible even though videos[2] was not in search results.
    assert_eq!(
        app.metadata_for(&videos[2].url).and_then(|m| m.view_count),
        Some(7_777)
    );

    // 3. Error and disconnect handling on channel fetch preserves previous channel_page if same target or clears if different.
    let (chan_req_c, valid_c) = app.begin_channel_request(target_b.clone(), &ctx).unwrap();
    assert!(app.channels.page.is_some()); // Same target keeps previous page visible during refresh
    assert!(app.complete_channel_request(
        chan_req_c,
        &valid_c,
        Err("Falha simulada de rede ao consultar canal.".into()),
        &ctx,
    ));
    assert!(!app.is_channel_loading());
    assert_eq!(
        app.channels.error.as_deref(),
        Some("Falha simulada de rede ao consultar canal.")
    );
    assert!(app.channels.page.is_some());

    // Disconnected channel receiver reports clear error message.
    let (chan_req_d, _valid_d) = app.begin_channel_request(target_a, &ctx).unwrap();
    assert!(app.channels.page.is_none()); // Different target clears old channel_page
    let (tx_drop, rx_drop) = mpsc::channel::<(u64, ChannelFetchMessage)>();
    drop(tx_drop);
    app.channels
        .videos
        .task
        .attach_receiver(chan_req_d, process::Cancellation::default(), rx_drop);
    app.poll_async_tasks(&ctx);
    assert!(!app.is_channel_loading());
    assert_eq!(
        app.channels.error.as_deref(),
        Some("A consulta do canal foi interrompida.")
    );
}

#[test]
fn starting_a_search_cancels_in_flight_channel_request() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.feed.query = "rust".into();

    let (channel_request, valid_target) = app
        .begin_channel_request(ChannelTarget::Handle("@RustLang".into()), &ctx)
        .expect("valid channel target should start request");
    let (tx_channel, rx_channel) = mpsc::channel::<(u64, ChannelFetchMessage)>();
    app.channels.videos.task.attach_receiver(
        channel_request,
        process::Cancellation::default(),
        rx_channel,
    );
    assert!(app.channels.view_open);
    assert!(app.is_channel_loading());

    let (search_request, query) = app
        .begin_search_request()
        .expect("non-empty query should start a search");
    assert_eq!(query, "rust");
    assert!(search_request > 0);
    assert!(!app.channels.view_open);
    assert!(!app.is_channel_loading());
    assert!(!app.channels.videos.task.is_running());
    assert!(app.channels.videos.task.active_request_id().is_none());

    // A page that arrives after the user started a new search must not be applied.
    let stale_page = ChannelPage {
        target: valid_target.clone(),
        channel_id: Some("UCaYhcUwRBNscFNUKTjgPFiA".into()),
        title: Some("Canal Obsoleto".into()),
        handle: Some("@RustLang".into()),
        canonical_url: valid_target.canonical_url(),
        description: None,
        subscriber_count: None,
        view_count: None,
        avatar_url: None,
        banner_url: None,
        verified: None,
        tags: Vec::new(),
        videos: Vec::new(),
        ..Default::default()
    };
    let _ = tx_channel.send((
        channel_request,
        ChannelFetchMessage {
            request_id: channel_request,
            target: valid_target.clone(),
            result: Ok(search::ChannelFetchOutcome {
                page: stale_page.clone(),
                from_cache: false,
            }),
        },
    ));
    app.poll_async_tasks(&ctx);
    assert!(app.channels.page.is_none());
    assert!(!app.complete_channel_request(channel_request, &valid_target, Ok(stale_page), &ctx));
    assert!(app.channels.page.is_none());
    assert!(app.channels.error.is_none());
}

#[test]
fn channel_view_preserves_navigation_queue_player_themes_history_and_thumbnail_cache_headless() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-channel-headless-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let history_path = temp_dir.join("history.json");
    let theme_path = temp_dir.join("theme-settings.txt");

    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut videos = valid_youtube_videos();
    videos.push(Video {
        title: "Vídeo YouTube Três".into(),
        channel: "Canal Gama".into(),
        url: "https://www.youtube.com/watch?v=3JZ_D3ELwOQ".into(),
        thumbnail: String::new(),
        duration: "5:45".into(),
    });
    videos[0].thumbnail = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".into();
    videos[1].thumbnail = "https://i.ytimg.com/vi/9bZkp7q19f0/hqdefault.jpg".into();
    videos[2].thumbnail = "https://i.ytimg.com/vi/3JZ_D3ELwOQ/hqdefault.jpg".into();

    let mut app = FastTube::default();
    app.queue.data = Queue::new(4);
    app.history.data = History::default();
    app.history.path = Some(history_path.clone());
    app.history.loaded_path = Some(history_path.clone());
    app.settings.theme.path = Some(theme_path.clone());
    app.settings.theme.loaded_path = Some(theme_path.clone());
    app.settings.buffer.profile = BufferProfile::Large;
    app.playback.active_buffer = Some(BufferProfile::Large);

    // Populate search results with 1 video that has a valid channel target and 1 that does not.
    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: videos[0].clone(),
                metadata: VideoMetadata {
                    channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
                    channel_url: Some("https://www.youtube.com/@ChanAlpha".into()),
                    view_count: Some(10_000),
                    upload_date: Some("20261106".into()),
                    description: Some("Vídeo com canal válido.".into()),
                    ..Default::default()
                },
            },
            SearchResult {
                video: videos[1].clone(),
                metadata: VideoMetadata {
                    channel_id: None,
                    channel_url: None,
                    view_count: Some(20_000),
                    ..Default::default()
                },
            },
        ],
    );

    // Enqueue videos[1] and record history for videos[0].
    assert!(app.enqueue_video(videos[1].clone()).is_ok());
    let t0 = Instant::now();
    app.record_video_in_history(&videos[0], 30_000, Some(120_000))
        .unwrap();
    app.save_history_to_disk(t0);

    // Start playing videos[0] in watch mode.
    app.selected = Some(videos[0].clone());
    app.selected_metadata = app.metadata_for(&videos[0].url).cloned();
    app.activate_video_playback(&videos[0], Some(30_000), None, t0);
    app.sync_player_window_visibility();
    assert!(app.playback.window_visible);

    // Run a first UI pass on search results to populate ThumbnailCache entries.
    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(base_rect),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_chan_initial_sidebar");
            });
        },
    );
    output.textures_delta.clear();
    let initial_retained_thumbs = app.thumbnails.retained_count();
    assert!(initial_retained_thumbs >= 1);

    // Open channel from videos[0]'s metadata target.
    let target = app
        .channel_target_for_video(&videos[0])
        .expect("videos[0] has valid channel target");
    let (req_id, valid_target) = app.begin_channel_request(target, &ctx).unwrap();
    assert!(app.channels.view_open);
    assert!(app.is_channel_loading());
    // Player remains active and visible while viewing channel in watch sidebar!
    assert_eq!(
        app.selected.as_ref().map(|v| v.url.as_str()),
        Some(videos[0].url.as_str())
    );
    assert!(app.playback.window_visible);
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.thumbnails.retained_count(), initial_retained_thumbs);

    // Render loading state in watch mode and browse mode across Dark and Light themes.
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(base_rect),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                app.header_ui(ui, &ctx);
                app.navigation_ui(ui, &ctx);
                app.watch_primary_ui(ui, &ctx, 760.0, 520.0, WatchLayoutMode::SideBySide);
                let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_chan_loading_sidebar");
            });
        },
    );
    output.textures_delta.clear();

    // Complete channel request with real channel page containing videos[2].
    let channel_page = ChannelPage {
        target: valid_target.clone(),
        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
        title: Some("Canal Alpha Oficial".into()),
        handle: Some("@ChanAlpha".into()),
        canonical_url: "https://www.youtube.com/channel/UC38IQsAvIsxxjztdMZQtwHA".into(),
        description: Some("Descrição real do canal Alpha.".into()),
        subscriber_count: Some(250_000),
        view_count: None,
        avatar_url: Some("https://yt3.googleusercontent.com/alpha_avatar.jpg".into()),
        banner_url: None,
        verified: Some(true),
        tags: Vec::new(),
        videos: vec![SearchResult {
            video: videos[2].clone(),
            metadata: VideoMetadata {
                channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
                channel_url: Some("https://www.youtube.com/@ChanAlpha".into()),
                view_count: Some(88_888),
                upload_date: Some("20261105".into()),
                description: Some("Vídeo exclusivo da aba do canal.".into()),
                ..Default::default()
            },
        }],
        ..Default::default()
    };
    assert!(app.complete_channel_request(req_id, &valid_target, Ok(channel_page), &ctx));
    assert!(!app.is_channel_loading());
    assert!(app.channels.view_open);
    assert!(app.channels.error.is_none());

    // Search results, queue, player state, history, and thumbnail cache are all preserved.
    assert_eq!(app.feed.videos.len(), 2);
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.history.data.entries().len(), 1);
    assert!(app.thumbnails.retained_count() >= initial_retained_thumbs);
    assert_eq!(
        app.selected.as_ref().map(|v| v.url.as_str()),
        Some(videos[0].url.as_str())
    );

    // Enqueue channel video (videos[2]) and advance queue to it: metadata from channel_video_metadata is preserved!
    assert!(app.enqueue_video(videos[2].clone()).is_ok());
    assert_eq!(app.queue.data.len(), 2);
    let first_advanced = app.advance_queue_at(t0 + Duration::from_secs(5)).unwrap();
    assert_eq!(first_advanced.url, videos[1].url);
    app.handle_queue_player_event(PlaybackEvent::StartFile {
        playlist_entry_id: 201,
    });
    app.handle_queue_player_event(PlaybackEvent::FileLoaded);
    let second_advanced = app.advance_queue_at(t0 + Duration::from_secs(10)).unwrap();
    assert_eq!(second_advanced.url, videos[2].url);
    app.handle_queue_player_event(PlaybackEvent::StartFile {
        playlist_entry_id: 202,
    });
    app.handle_queue_player_event(PlaybackEvent::FileLoaded);
    assert_eq!(
        app.selected_metadata.as_ref().and_then(|m| m.view_count),
        Some(88_888)
    );

    // Render loaded channel page in both Dark and Light themes in watch sidebar and browse view.
    for theme in [Theme::Dark, Theme::Light] {
        app.apply_theme(theme);
        app.sync_theme_visuals(&ctx);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(base_rect),
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| {
                    app.header_ui(ui, &ctx);
                    app.navigation_ui(ui, &ctx);
                    app.watch_primary_ui(ui, &ctx, 760.0, 520.0, WatchLayoutMode::SideBySide);
                    let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_chan_loaded_sidebar");
                    let _ = app.browse_ui(ui, &ctx);
                });
            },
        );
        output.textures_delta.clear();
        assert_eq!(ctx.global_style().visuals.dark_mode, theme == Theme::Dark);
    }

    // Closing channel view returns to search results while keeping channel_page cached for navigation_ui.
    app.close_channel_view();
    assert!(!app.channels.view_open);
    assert!(app.channels.page.is_some());
    assert_eq!(
        app.current_channel_target(),
        Some(ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into()))
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn channel_progressive_videos_pagination_playlists_and_about_tabs_work_and_preserve_oauth_playlists()
 {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.channels.video_limit = 2;

    // Preserve OAuth playlists state alongside channel public playlists.
    app.feed.home.title = Some("Playlists da conta Google".into());
    app.feed.home.playlist_urls = vec!["fasttube-google-playlist:PL_OAUTH_USER_123".into()];

    let target = ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into());
    let (req_id, valid_target) = app.begin_channel_request(target.clone(), &ctx).unwrap();

    let v1 = SearchResult {
        video: Video {
            title: "Vídeo 1".into(),
            channel: "Blender".into(),
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            thumbnail: String::new(),
            duration: "2:00".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(100),
            ..Default::default()
        },
    };
    let v2 = SearchResult {
        video: Video {
            title: "Vídeo 2".into(),
            channel: "Blender".into(),
            url: "https://www.youtube.com/watch?v=9bZkp7q19f0".into(),
            thumbnail: String::new(),
            duration: "3:00".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(200),
            ..Default::default()
        },
    };
    let initial_page = ChannelPage {
        target: valid_target.clone(),
        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
        title: Some("Blender Foundation".into()),
        handle: Some("@BlenderOfficial".into()),
        canonical_url: valid_target.canonical_url(),
        description: Some("Short description.".into()),
        subscriber_count: Some(1_000_000),
        view_count: None,
        avatar_url: Some("https://yt3.googleusercontent.com/avatar.jpg".into()),
        banner_url: None,
        verified: Some(true),
        tags: Vec::new(),
        videos: vec![v1.clone(), v2.clone()],
        ..Default::default()
    };
    assert!(app.complete_channel_request(req_id, &valid_target, Ok(initial_page), &ctx));
    assert!(app.channels.videos.has_more);
    assert!(!app.channels.videos.reached_max);

    // 1. Progressive video pagination with deduplication & error handling.
    let (more_req_1, more_target, start, count) = app
        .begin_channel_more_videos_request()
        .expect("should start progressive video batch");
    assert_eq!(more_target, valid_target);
    assert_eq!(start, 3);
    assert_eq!(count, search::CHANNEL_VIDEOS_PAGE_STEP);
    assert!(app.is_channel_more_videos_loading());

    // Simulate an error first, then retry and succeed.
    assert!(app.complete_channel_more_videos(
        more_req_1,
        &valid_target,
        Err("Falha temporária ao paginar vídeos.".into()),
    ));
    assert!(!app.is_channel_more_videos_loading());
    assert_eq!(
        app.channels.videos.more_error.as_deref(),
        Some("Falha temporária ao paginar vídeos.")
    );

    let (more_req_2, _, start_retry, _) = app.begin_channel_more_videos_request().unwrap();
    assert_eq!(start_retry, 3);
    let v3 = SearchResult {
        video: Video {
            title: "Vídeo 3 Novo".into(),
            channel: "Blender".into(),
            url: "https://www.youtube.com/watch?v=3JZ_D3ELwOQ".into(),
            thumbnail: String::new(),
            duration: "4:10".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(300),
            ..Default::default()
        },
    };
    assert!(app.complete_channel_more_videos(
        more_req_2,
        &valid_target,
        Ok(search::ChannelVideosBatch {
            videos: vec![v2.clone(), v3.clone()], // v2 is duplicate and must be deduplicated
            start: 3,
            end: 22,
            has_more: false,
            reached_max_limit: false,
        }),
    ));
    assert!(app.channels.videos.more_error.is_none());
    assert!(!app.channels.videos.has_more);
    assert_eq!(app.channels.page.as_ref().unwrap().videos.len(), 3);
    assert_eq!(
        app.metadata_for(&v3.video.url).and_then(|m| m.view_count),
        Some(300)
    );

    // 2. Public playlists tab on-demand load and opening playlist videos.
    app.channels.tab = ChannelTab::Playlists;
    let (pl_req_id, pl_target) = app
        .begin_channel_playlists_request()
        .expect("should start channel playlists fetch");
    assert_eq!(pl_target, valid_target);
    assert!(app.is_channel_playlists_loading());

    let public_playlist = search::ChannelPlaylist {
        id: "PL_BLENDER_OPEN_MOVIES".into(),
        title: "Blender Open Movies".into(),
        url: "https://www.youtube.com/playlist?list=PL_BLENDER_OPEN_MOVIES".into(),
        item_count: Some(12),
        thumbnail: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".into(),
        description: Some("Official open movies playlist.".into()),
        privacy: crate::playlist_sync::PlaylistPrivacy::Public,
    };
    assert!(app.complete_channel_playlists(
        pl_req_id,
        &valid_target,
        Ok(vec![public_playlist.clone()]),
        &ctx,
    ));
    assert!(!app.is_channel_playlists_loading());
    assert!(app.channels.playlists.loaded);
    assert_eq!(app.channels.playlists.items.len(), 1);

    // Open public playlist videos inside the channel Playlists tab.
    let (pl_vids_req, pl_vids_target, pid) = app
        .begin_channel_playlist_videos_request(public_playlist.clone(), &ctx)
        .expect("valid playlist should start videos request");
    assert_eq!(pl_vids_target, valid_target);
    assert_eq!(pid, "PL_BLENDER_OPEN_MOVIES");
    assert!(app.is_channel_playlist_videos_loading());

    let pl_video = SearchResult {
        video: Video {
            title: "Sprite Fright - Open Movie".into(),
            channel: "Blender Foundation".into(),
            url: "https://www.youtube.com/watch?v=_cMxraX_5RE".into(),
            thumbnail: String::new(),
            duration: "10:29".into(),
        },
        metadata: VideoMetadata {
            channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
            view_count: Some(950_000),
            ..Default::default()
        },
    };
    assert!(app.complete_channel_playlist_videos(
        pl_vids_req,
        &valid_target,
        &pid,
        Ok(vec![pl_video.clone()]),
    ));
    assert!(!app.is_channel_playlist_videos_loading());
    assert!(app.channels.playlist_detail.loaded);
    assert_eq!(app.channels.playlist_detail.items.len(), 1);
    assert_eq!(
        app.metadata_for(&pl_video.video.url)
            .and_then(|m| m.view_count),
        Some(950_000)
    );
    // OAuth playlists state remains untouched.
    assert_eq!(
        app.feed.home.playlist_urls,
        vec!["fasttube-google-playlist:PL_OAUTH_USER_123".to_owned()]
    );

    // 3. Channel About tab on-demand load and metadata enrichment.
    app.channels.tab = ChannelTab::About;
    let (about_req_id, about_target) = app
        .begin_channel_about_request()
        .expect("should start channel about fetch");
    assert_eq!(about_target, valid_target);
    assert!(app.is_channel_about_loading());

    let about_info = search::ChannelAboutInfo {
        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
        title: Some("Blender Foundation".into()),
        handle: Some("@BlenderOfficial".into()),
        canonical_url: Some(valid_target.canonical_url()),
        description: Some(
            "Blender Foundation is the non-profit organization behind Blender 3D.".into(),
        ),
        subscriber_count: Some(1_250_000),
        view_count: Some(145_000_000),
        avatar_url: None,
        banner_url: Some("https://yt3.googleusercontent.com/banner.jpg".into()),
        verified: Some(true),
        tags: vec!["blender".into(), "3d".into(), "open source".into()],
        ..Default::default()
    };
    assert!(app.complete_channel_about(about_req_id, &valid_target, Ok(about_info)));
    assert!(!app.is_channel_about_loading());
    assert!(app.channels.about.loaded);
    let updated_page = app.channels.page.as_ref().unwrap();
    assert_eq!(updated_page.subscriber_count, Some(1_250_000));
    assert_eq!(updated_page.view_count, Some(145_000_000));
    assert_eq!(updated_page.tags, vec!["blender", "3d", "open source"]);
    assert_eq!(
        updated_page.description.as_deref(),
        Some("Blender Foundation is the non-profit organization behind Blender 3D.")
    );

    // Render all channel tabs in headless UI.
    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    for tab in [
        ChannelTab::Home,
        ChannelTab::Videos,
        ChannelTab::Shorts,
        ChannelTab::Streams,
        ChannelTab::Playlists,
        ChannelTab::Search,
        ChannelTab::About,
    ] {
        app.channels.tab = tab;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(base_rect),
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = app.channel_section_ui(
                        ui,
                        &ctx,
                        "test_expanded_channel_tabs",
                        false,
                        Some(BROWSE_MAX_CONTENT_WIDTH),
                    );
                });
            },
        );
        output.textures_delta.clear();
    }

    // Closing the selected public playlist returns to the channel's playlists list.
    app.close_channel_public_playlist(&ctx);
    assert!(app.channels.playlist_detail.selected.is_none());
    assert!(app.channels.playlist_detail.items.is_empty());

    // Cancellation clears active sub-requests cleanly.
    app.channels.videos.has_more = true;
    let (cancelled_more_request, cancelled_more_target, _, _) =
        app.begin_channel_more_videos_request().unwrap();
    let _ = app.begin_channel_playlists_request();
    let _ = app.begin_channel_about_request();
    assert!(app.is_channel_more_videos_loading());
    assert!(app.is_channel_playlists_loading());
    assert!(app.is_channel_about_loading());
    app.cancel_channel_subrequests();
    assert!(!app.is_channel_more_videos_loading());
    assert!(!app.is_channel_playlists_loading());
    assert!(!app.is_channel_about_loading());
    assert!(!app.complete_channel_more_videos(
        cancelled_more_request,
        &cancelled_more_target,
        Ok(search::ChannelVideosBatch {
            videos: Vec::new(),
            start: 4,
            end: 23,
            has_more: false,
            reached_max_limit: false,
        }),
    ));
    assert!(app.channels.videos.has_more);
}

#[test]
fn channel_home_shelves_sort_chips_inline_search_and_about_modal_work_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    let target = ChannelTarget::Handle("@Thebausffs".into());
    let (req_id, valid_target) = app.begin_channel_request(target, &ctx).unwrap();

    let featured = SearchResult {
        video: Video {
            title: "THE ULTIMATE SION GUIDE | Thebausffs".into(),
            channel: "Thebausffs".into(),
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            thumbnail: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".into(),
            duration: "33:44".into(),
        },
        metadata: VideoMetadata {
            channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
            channel_url: Some("https://www.youtube.com/@Thebausffs".into()),
            view_count: Some(2_500_000),
            upload_date: Some("20201106".into()),
            description: Some("Sion matchup sheet and guide.".into()),
            ..Default::default()
        },
    };
    let latest_low = SearchResult {
        video: Video {
            title: "THE BEST GRAGAS YOU WILL EVER SEE!".into(),
            channel: "Thebausffs".into(),
            url: "https://www.youtube.com/watch?v=9bZkp7q19f0".into(),
            thumbnail: "https://i.ytimg.com/vi/9bZkp7q19f0/hqdefault.jpg".into(),
            duration: "14:39".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(314_000),
            upload_date: Some("20261110".into()),
            ..Default::default()
        },
    };
    let popular_high = SearchResult {
        video: Video {
            title: "EUW CHALLENGER SION MONTAGE".into(),
            channel: "Thebausffs".into(),
            url: "https://www.youtube.com/watch?v=3JZ_D3ELwOQ".into(),
            thumbnail: "https://i.ytimg.com/vi/3JZ_D3ELwOQ/hqdefault.jpg".into(),
            duration: "12:05".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(1_850_000),
            upload_date: Some("20240101".into()),
            ..Default::default()
        },
    };
    let short_item = SearchResult {
        video: Video {
            title: "SOLOBOLO! #shorts".into(),
            channel: "Thebausffs".into(),
            url: "https://www.youtube.com/shorts/abcdefghijk".into(),
            thumbnail: "https://i.ytimg.com/vi/abcdefghijk/hqdefault.jpg".into(),
            duration: "0:35".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(420_000),
            ..Default::default()
        },
    };

    let page = ChannelPage {
        target: valid_target.clone(),
        channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
        title: Some("Thebausffs".into()),
        handle: Some("@Thebausffs".into()),
        canonical_url: "https://www.youtube.com/@Thebausffs".into(),
        description: Some("Hey guys it's me Thebausffs EUW challenger OTP full AD Sion.".into()),
        description_preview: Some(
            "Hey guys it's me Thebausffs EUW challenger OTP full AD Sion.".into(),
        ),
        subscriber_count: Some(1_120_000),
        subscriber_count_text: Some("1,12 mi de inscritos".into()),
        video_count_text: Some("1,2 mil vídeos".into()),
        view_count_text: Some("420.000.000 visualizações".into()),
        country: Some("Suécia".into()),
        joined_date_text: Some("Inscreveu-se em 15 de mar. de 2016".into()),
        links: vec![
            search::ChannelExternalLink {
                title: "Twitch".into(),
                url: "https://www.twitch.tv/thebausffs".into(),
            },
            search::ChannelExternalLink {
                title: "Discord".into(),
                url: "https://discord.gg/thebausffs".into(),
            },
        ],
        more_links_label: Some("e mais 1 link".into()),
        featured_video: Some(featured.clone()),
        home_shelves: vec![
            search::ChannelHomeShelf {
                title: "Vídeos".into(),
                subtitle: None,
                kind: search::ChannelShelfKind::Videos,
                playlist_id: None,
                items: vec![latest_low.clone(), popular_high.clone()],
            },
            search::ChannelHomeShelf {
                title: "Shorts".into(),
                subtitle: None,
                kind: search::ChannelShelfKind::Shorts,
                playlist_id: None,
                items: vec![short_item.clone()],
            },
        ],
        has_shorts_tab: true,
        has_streams_tab: true,
        has_playlists_tab: true,
        banner_url: Some("https://yt3.googleusercontent.com/baus_banner.jpg".into()),
        avatar_url: Some("https://yt3.googleusercontent.com/baus_avatar.jpg".into()),
        verified: Some(true),
        videos: vec![latest_low.clone(), popular_high.clone()],
        ..Default::default()
    };

    assert!(app.complete_channel_request(req_id, &valid_target, Ok(page), &ctx));
    assert_eq!(app.channels.tab, ChannelTab::Home);
    assert_eq!(
        app.metadata_for(&featured.video.url)
            .and_then(|m| m.view_count),
        Some(2_500_000)
    );
    assert_eq!(
        app.metadata_for(&short_item.video.url)
            .and_then(|m| m.view_count),
        Some(420_000)
    );
    assert!(app.channels.about.info.is_some());

    // Local fallback sorting when continuation tokens are absent.
    assert!(!app.start_channel_sorted_videos(search::ChannelVideosSort::Popular, &ctx));
    assert_eq!(app.channels.videos.sort, search::ChannelVideosSort::Popular);
    assert_eq!(app.channels.videos.sorted_items.len(), 2);
    assert_eq!(
        app.channels.videos.sorted_items[0].video.url,
        popular_high.video.url
    );

    assert!(!app.start_channel_sorted_videos(search::ChannelVideosSort::Oldest, &ctx));
    assert_eq!(app.channels.videos.sort, search::ChannelVideosSort::Oldest);
    assert_eq!(
        app.channels.videos.sorted_items[0].video.url,
        popular_high.video.url
    );
    assert_eq!(
        app.channels.videos.sorted_items[1].video.url,
        latest_low.video.url
    );

    assert!(!app.start_channel_sorted_videos(search::ChannelVideosSort::Latest, &ctx));
    assert_eq!(app.channels.videos.sort, search::ChannelVideosSort::Latest);

    // Remote continuation sort lifecycle & stale rejection.
    if let Some(p) = app.channels.page.as_mut() {
        p.videos_popular_continuation = Some("CONT_POPULAR_TOKEN".into());
    }
    assert!(app.start_channel_sorted_videos(search::ChannelVideosSort::Popular, &ctx));
    let active_sort_req = app.channels.videos.sorted_task.active_request_id().unwrap();
    assert!(!app.complete_channel_sorted_videos(
        active_sort_req + 99,
        &valid_target,
        search::ChannelVideosSort::Popular,
        Ok(vec![latest_low.clone()]),
    ));
    assert!(app.complete_channel_sorted_videos(
        active_sort_req,
        &valid_target,
        search::ChannelVideosSort::Popular,
        Ok(vec![popular_high.clone()]),
    ));
    assert_eq!(app.channels.videos.sorted_items.len(), 1);
    assert_eq!(
        app.channels.videos.sorted_items[0].video.url,
        popular_high.video.url
    );

    // Inline channel search lifecycle (empty query cancels, valid query opens Search tab).
    assert!(!app.start_channel_search("   ", &ctx));
    assert!(app.start_channel_search("sion", &ctx));
    assert_eq!(app.channels.tab, ChannelTab::Search);
    assert_eq!(app.channels.search.query, "sion");
    let search_req_id = app.channels.search.task.active_request_id().unwrap();
    assert!(!app.complete_channel_search(
        search_req_id + 1,
        &valid_target,
        "sion".into(),
        Ok(Vec::new()),
    ));
    assert!(app.complete_channel_search(
        search_req_id,
        &valid_target,
        "sion".into(),
        Ok(vec![featured.clone()]),
    ));
    assert!(app.channels.search.loaded);
    assert_eq!(app.channels.search.items.len(), 1);

    // Render Home, Videos, Search, and the About modal in both Dark and Light themes.
    app.channels.about_modal_open = true;
    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1360.0, 900.0));
    for theme in [Theme::Dark, Theme::Light] {
        app.apply_theme(theme);
        app.sync_theme_visuals(&ctx);
        for tab in [ChannelTab::Home, ChannelTab::Videos, ChannelTab::Search] {
            app.channels.tab = tab;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(base_rect),
                    ..Default::default()
                },
                |ui| {
                    let ctx = ui.ctx().clone();
                    egui::CentralPanel::default().show(ui, |ui| {
                        let _ = app.channel_section_ui(
                            ui,
                            &ctx,
                            "test_baus_channel_ui",
                            false,
                            Some(BROWSE_MAX_CONTENT_WIDTH),
                        );
                    });
                    app.channel_about_modal_ui(&ctx);
                },
            );
            output.textures_delta.clear();
        }
    }

    // Closing channel view resets modal state cleanly.
    app.close_channel_view();
    assert!(!app.channels.about_modal_open);
}

#[test]
fn youtube_layout_and_custom_recommendation_engine_work_across_filters_and_shelves() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    let v_fav = Video {
        title: "5 Vilões que seriam ótimos namorados #animação".into(),
        channel: "5 Alguma Coisa".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
        duration: "8:01".into(),
    };
    let v_music = Video {
        title: "The Strokes - Lonely in the Future (Live Music Video)".into(),
        channel: "The Strokes".into(),
        url: "https://www.youtube.com/watch?v=9bZkp7q19f0".into(),
        thumbnail: String::new(),
        duration: "3:46".into(),
    };
    let v_short = Video {
        title: "Por que essa garrafa de vinho não abre? #shorts".into(),
        channel: "Bolebo".into(),
        url: "https://www.youtube.com/watch?v=3JZ_D3ELwOQ".into(),
        thumbnail: String::new(),
        duration: "0:42".into(),
    };

    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: v_fav.clone(),
                metadata: VideoMetadata {
                    view_count: Some(47_000),
                    upload_date: Some("2026-11-09T22:30:07Z".into()),
                    ..Default::default()
                },
            },
            SearchResult {
                video: v_music.clone(),
                metadata: VideoMetadata {
                    view_count: Some(32_000),
                    upload_date: Some("2026-11-09T20:00:00Z".into()),
                    ..Default::default()
                },
            },
            SearchResult {
                video: v_short.clone(),
                metadata: VideoMetadata {
                    view_count: Some(15_000),
                    upload_date: Some("2026-11-09T19:00:00Z".into()),
                    ..Default::default()
                },
            },
        ],
    );

    app.history
        .data
        .record(history::Entry {
            video_id: "___________".into(),
            title: "Animação anterior de 5 Alguma Coisa #animação".into(),
            channel: "5 Alguma Coisa".into(),
            position_ms: 210_000,
            duration_ms: Some(220_000),
            watched_at_ms: None,
            ..Default::default()
        })
        .unwrap();

    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    for filter in [
        FeedFilter::All,
        FeedFilter::Recommended,
        FeedFilter::Music,
        FeedFilter::Shorts,
        FeedFilter::RecentlyUploaded,
        FeedFilter::NewToYou,
        FeedFilter::Channel("5 Alguma Coisa".into()),
    ] {
        app.feed.filter = filter;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(base_rect),
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                egui::Panel::top("header").show(ui, |ui| {
                    app.header_ui(ui, &ctx);
                });
                egui::Panel::left("navigation")
                    .exact_size(layout::compute_youtube_sidebar_width(1280.0))
                    .show(ui, |ui| {
                        app.navigation_ui(ui, &ctx);
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = app.browse_ui(ui, &ctx);
                });
            },
        );
        output.textures_delta.clear();
    }
}

#[test]
fn recent_searches_deduplicates_and_ignores_internal_queries() {
    let mut app = FastTube::default();
    app.record_recent_search("rust egui");
    app.record_recent_search("  Rust Egui  ");
    app.record_recent_search(":ytrec");
    app.record_recent_search("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    app.record_recent_search("mpv player");

    assert_eq!(
        app.feed.recent_searches,
        vec!["mpv player".to_owned(), "Rust Egui".to_owned()]
    );
}

#[test]
fn next_recommended_and_shorts_navigation_resolve_expected_videos() {
    let mut app = FastTube::default();
    let videos = vec![
        Video {
            title: "Short 1".into(),
            channel: "Canal A".into(),
            duration: "0:30".into(),
            url: "https://www.youtube.com/shorts/short000001".into(),
            thumbnail: String::new(),
        },
        Video {
            title: "Short 2".into(),
            channel: "Canal B".into(),
            duration: "0:45".into(),
            url: "https://www.youtube.com/shorts/short000002".into(),
            thumbnail: String::new(),
        },
    ];
    app.feed.videos = videos.clone();
    app.feed.cache_dirty = true;
    app.selected = Some(videos[0].clone());

    let next_rec = app.next_recommended_video().expect("next recommended");
    assert_eq!(next_rec.url, videos[1].url);

    let next_short = app.next_or_prev_short(true).expect("next short");
    assert_eq!(next_short.url, videos[1].url);

    app.selected = Some(videos[1].clone());
    let prev_short = app.next_or_prev_short(false).expect("prev short");
    assert_eq!(prev_short.url, videos[0].url);
}

#[test]
fn channel_tab_videos_completion_populates_shorts_and_streams() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let target = ChannelTarget::Handle("rustlang".into());
    app.channels.view_open = true;
    app.channels.last_target = Some(target.clone());

    let (req_shorts, _) = app.channels.begin_shorts_request().expect("shorts req");
    let item = SearchResult {
        video: Video {
            title: "Rust Short".into(),
            channel: "Rust".into(),
            duration: "0:40".into(),
            url: "https://www.youtube.com/shorts/abc12345678".into(),
            thumbnail: String::new(),
        },
        metadata: VideoMetadata::default(),
    };
    assert!(app.complete_channel_tab_videos(
        req_shorts,
        &target,
        search::ChannelFeedTab::Shorts,
        Ok(vec![item]),
    ));
    assert!(app.channels.shorts.loaded);
    assert_eq!(app.channels.shorts.items.len(), 1);

    app.reset_channel_tabs_state(&ctx);
    assert!(!app.channels.shorts.loaded);
    assert!(app.channels.shorts.items.is_empty());
}

#[test]
fn poll_my_channel_stores_avatar_and_only_navigates_when_requested() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("token"));
    app.auth.my_channel_open_when_ready = false;
    let (req_id, _cancel, tx) = app.auth.my_channel_task.begin_manual();
    tx.send((
        req_id,
        (
            Some(oauth::TokenSession::test_session("token-2")),
            Ok(oauth::MyChannelProfile {
                target: ChannelTarget::ChannelId("UCabc123".into()),
                title: Some("Canal Teste".into()),
                avatar_url: Some("https://yt3.ggpht.com/avatar=s88".into()),
                subscriber_count: Some(3),
            }),
        ),
    ))
    .unwrap();

    app.poll_my_channel(&ctx);

    assert_eq!(
        app.auth.my_channel,
        Some(ChannelTarget::ChannelId("UCabc123".into()))
    );
    assert_eq!(app.auth.my_channel_title.as_deref(), Some("Canal Teste"));
    assert_eq!(
        app.auth.my_channel_avatar_url.as_deref(),
        Some("https://yt3.ggpht.com/avatar=s88")
    );
    assert_eq!(app.auth.my_channel_subscribers, Some(3));
    assert!(!app.channels.view_open);
}

#[test]
fn home_feed_cache_restores_without_refetch_on_inicio() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    let (req_id, _cancel, sender) = app.feed.home.batch_task.begin_manual();
    let mix_url = "https://www.youtube.com/playlist?list=RDmix01".to_owned();
    sender
        .send((
            req_id,
            HomeBatchFetchMessage {
                request_id: req_id,
                page: 0,
                result: Ok(search::HomeFeed {
                    items: vec![
                        SearchResult {
                            video: Video {
                                title: "Mix de Rock".into(),
                                channel: "Banda X e outros".into(),
                                url: mix_url.clone(),
                                thumbnail: String::new(),
                                duration: String::new(),
                            },
                            metadata: VideoMetadata::default(),
                        },
                        SearchResult {
                            video: Video {
                                title: "Video Principal".into(),
                                channel: "Canal Principal".into(),
                                url: "https://www.youtube.com/watch?v=mainvid0001".into(),
                                thumbnail: String::new(),
                                duration: "12:34".into(),
                            },
                            metadata: VideoMetadata::default(),
                        },
                    ],
                    playlists: vec![mix_url.clone()],
                    channels: Vec::new(),
                }),
            },
        ))
        .unwrap();

    app.poll_async_tasks(&ctx);
    assert!(app.feed.home.cached_loaded);
    assert_eq!(app.feed.home.cached_items.len(), 2);
    assert_eq!(app.feed.videos.len(), 2);

    // Switch away to a playlist/subscriptions view, then call start_public_home:
    // it must restore cached Home instantly without spawning a new batch_task fetch.
    app.feed.home.title = Some("Inscri\u{00e7}\u{00f5}es: envios recentes".into());
    app.feed.videos.clear();
    app.start_public_home(&ctx);

    assert!(app.feed.home.title.is_none());
    assert!(!app.feed.home.batch_task.is_running());
    assert_eq!(app.feed.videos.len(), 2);
    assert!(app.feed.home.playlist_urls.contains(&mix_url));
}

#[test]
fn home_feed_batch_pagination_accumulates_past_100_items() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();

    for page in 0..2usize {
        let (request_id, _cancel, sender) = app.feed.home.batch_task.begin_manual();
        let items: Vec<SearchResult> = (0..65usize)
            .map(|idx| {
                let global_idx = page * 65 + idx;
                SearchResult {
                    video: Video {
                        title: format!("Video Home {global_idx}"),
                        channel: format!("Canal {}", global_idx % 9),
                        url: format!("https://www.youtube.com/watch?v=vid{global_idx:08}"),
                        thumbnail: String::new(),
                        duration: "8:10".into(),
                    },
                    metadata: VideoMetadata::default(),
                }
            })
            .collect();
        sender
            .send((
                request_id,
                HomeBatchFetchMessage {
                    request_id,
                    page,
                    result: Ok(search::HomeFeed {
                        items,
                        playlists: Vec::new(),
                        channels: Vec::new(),
                    }),
                },
            ))
            .unwrap();
        app.poll_async_tasks(&ctx);
    }

    assert_eq!(app.feed.home.cached_items.len(), 130);
    assert_eq!(app.feed.videos.len(), 130);
    assert_eq!(app.feed.home.page, 2);
    assert!(app.feed.home.has_more);
}

#[test]
fn youtube_home_grid_renders_mix_playlist_verified_channel_and_shorts_novo() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    let days_since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        / 86_400;
    // Howard Hinnant civil-from-days algorithm for today's YYYYMMDD.
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let today_ymd = format!("{y:04}{m:02}{d:02}");

    let mix_url = "https://www.youtube.com/playlist?list=RD0001".to_owned();
    app.feed.home.playlist_urls = vec![mix_url.clone()];
    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: Video {
                    title: "Mix de One way trigger - The Strokes".into(),
                    channel: "The Strokes e outros".into(),
                    url: mix_url,
                    thumbnail: String::new(),
                    duration: String::new(),
                },
                metadata: VideoMetadata::default(),
            },
            SearchResult {
                video: Video {
                    title: "Gameplay Especial".into(),
                    channel: "Canal Verificado".into(),
                    url: "https://www.youtube.com/watch?v=normal00001".into(),
                    thumbnail: String::new(),
                    duration: "15:01".into(),
                },
                metadata: VideoMetadata {
                    view_count: Some(288_000),
                    channel_url: Some("https://www.youtube.com/@CanalVerificado".into()),
                    ..Default::default()
                },
            },
            SearchResult {
                video: Video {
                    title: "Curta recente".into(),
                    channel: "Shorts Criador".into(),
                    url: "https://www.youtube.com/shorts/short000001".into(),
                    thumbnail: String::new(),
                    duration: "0:32".into(),
                },
                metadata: VideoMetadata {
                    view_count: Some(12_000),
                    upload_date: Some(today_ymd),
                    ..Default::default()
                },
            },
        ],
    );

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360.0, 840.0),
            )),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                app.filter_chips_bar_ui(ui, &c);
                let _ = app.home_grid_ui(ui, &c);
            });
        },
    );
    let rendered = collect_rendered_text(&output.shapes).join("\n");
    output.textures_delta.clear();

    assert!(rendered.contains("Tudo"), "rendered:\n{rendered}");
    assert!(rendered.contains("Mix"), "rendered:\n{rendered}");
    assert!(rendered.contains("Playlist"), "rendered:\n{rendered}");
    assert!(
        rendered.contains("Canal Verificado"),
        "rendered:\n{rendered}"
    );
    assert!(rendered.contains("Shorts"), "rendered:\n{rendered}");
    assert!(rendered.contains("Novo"), "rendered:\n{rendered}");
}

#[test]
fn search_results_ui_shows_youtube_chips_filters_and_non_overlapping_cards() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.ui.search_results_mode = true;
    app.ui.browse_section = BrowseSection::Home;
    app.feed.query = "resident evil".into();

    let playlist_url = "https://www.youtube.com/playlist?list=PLsearch0001".to_owned();
    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: Video {
                    title: "Live Jogando Agora".into(),
                    channel: "Canal Live".into(),
                    url: "https://www.youtube.com/watch?v=live0000001".into(),
                    thumbnail: String::new(),
                    duration: String::new(),
                },
                metadata: VideoMetadata {
                    kind: search::SearchResultKind::Video,
                    is_live: true,
                    view_count: Some(64_000),
                    view_count_text: Some("64 mil assistindo".into()),
                    channel_verified: true,
                    badges: vec!["AO VIVO".into()],
                    description: Some("Transmissão ao vivo agora mesmo.".into()),
                    ..Default::default()
                },
            },
            SearchResult {
                video: Video {
                    title: "Gameplay Comentada".into(),
                    channel: "Canal Gamer".into(),
                    url: "https://www.youtube.com/watch?v=vod00000002".into(),
                    thumbnail: String::new(),
                    duration: "13:17".into(),
                },
                metadata: VideoMetadata {
                    kind: search::SearchResultKind::Video,
                    view_count: Some(53_000),
                    view_count_text: Some("53 mil visualizações".into()),
                    published_time_text: Some("há 1 hora".into()),
                    badges: vec!["Novo".into(), "HD".into()],
                    description: Some("Análise completa em 60fps.".into()),
                    ..Default::default()
                },
            },
            SearchResult {
                video: Video {
                    title: "Canal Oficial Exemplo".into(),
                    channel: "@CanalExemplo".into(),
                    url: "https://www.youtube.com/@CanalExemplo".into(),
                    thumbnail: String::new(),
                    duration: String::new(),
                },
                metadata: VideoMetadata {
                    kind: search::SearchResultKind::Channel,
                    subscriber_count_text: Some("989 inscritos".into()),
                    channel_verified: true,
                    description: Some("Vídeos semanais de tecnologia.".into()),
                    ..Default::default()
                },
            },
            SearchResult {
                video: Video {
                    title: "Trilha Sonora Completa".into(),
                    channel: "Canal Música".into(),
                    url: playlist_url.clone(),
                    thumbnail: String::new(),
                    duration: String::new(),
                },
                metadata: VideoMetadata {
                    kind: search::SearchResultKind::Playlist,
                    playlist_count: Some("91 vídeos".into()),
                    ..Default::default()
                },
            },
        ],
    );
    assert!(app.feed.home.playlist_urls.contains(&playlist_url));

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 1400.0),
            )),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.browse_page_ui(ui, &c);
            });
        },
    );
    let text_rects = collect_rendered_text_rects(&output.shapes);
    let rendered = text_rects
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    output.textures_delta.clear();

    // Home-only chips must not leak into search results mode.
    assert!(!rendered.contains("Jogos"), "rendered:\n{rendered}");
    assert!(!rendered.contains("Mixes"), "rendered:\n{rendered}");
    assert_eq!(
        rendered.matches("AO VIVO").count(),
        1,
        "rendered:\n{rendered}"
    );

    // Official search chips and right-aligned Filtros button are visible.
    for expected in [
        "Tudo",
        "Shorts",
        "Não assistidos",
        "Vídeos",
        "Enviados recentemente",
        "Ao vivo",
        "Filtros",
        "64 mil assistindo",
        "AO VIVO",
        "53 mil visualizações • há 1 hora",
        "13:17",
        "Novo",
        "HD",
        "@CanalExemplo • 989 inscritos",
        "Ver playlist completa",
        "91 vídeos",
    ] {
        assert!(
            rendered.contains(expected),
            "missing '{expected}' in rendered:\n{rendered}"
        );
    }

    // Geometry check: card title sits cleanly to the right of thumbnail/duration badge (zero overlap),
    // and Filtros button sits cleanly to the right of the Ao vivo chip.
    let dur_rect = text_rects
        .iter()
        .find(|(t, _)| t == "13:17")
        .expect("duration rect")
        .1;
    let title_rect = text_rects
        .iter()
        .find(|(t, _)| t.contains("Gameplay Comentada"))
        .expect("title rect")
        .1;
    assert!(
        title_rect.min.x > dur_rect.max.x + 8.0,
        "title overlapped thumbnail column: title={title_rect:?} dur={dur_rect:?}"
    );

    let ao_vivo_chip_rect = text_rects
        .iter()
        .find(|(t, _)| t == "Ao vivo")
        .expect("Ao vivo chip")
        .1;
    let filtros_rect = text_rects
        .iter()
        .find(|(t, _)| t == "Filtros")
        .expect("Filtros button")
        .1;
    assert!(
        filtros_rect.min.x > ao_vivo_chip_rect.max.x,
        "Filtros button should be right-aligned past search chips: chip={ao_vivo_chip_rect:?} filtros={filtros_rect:?}"
    );
}

#[test]
fn search_filters_modal_renders_five_youtube_columns_and_toggles_sort() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.ui.search_results_mode = true;
    app.feed.search_filters_open = true;

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut output = egui::FullOutput::default();
    for _ in 0..2 {
        output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.search_filters_modal_ui(&c);
            },
        );
        output.textures_delta.clear();
    }
    let text_rects = collect_rendered_text_rects(&output.shapes);
    let rendered = text_rects
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    for expected in [
        "Filtros de pesquisa",
        "DATA DE ENVIO",
        "TIPO",
        "DURAÇÃO",
        "RECURSOS",
        "ORDENAR POR",
        "Última hora",
        "Hoje",
        "Esta semana",
        "Este mês",
        "Este ano",
        "Vídeo",
        "Canal",
        "Playlist",
        "Filme",
        "Menos de 4 min",
        "4 a 20 min",
        "Mais de 20 min",
        "Ao vivo",
        "4K",
        "HD",
        "Legendas/CC",
        "Relevância",
        "Data de envio",
        "Contagem de visualizações",
        "Avaliação",
        "Limpar filtros",
        "Aplicar",
    ] {
        assert!(
            rendered.contains(expected),
            "missing '{expected}' in filter modal:\n{rendered}"
        );
    }

    let views_pos = text_rects
        .iter()
        .find(|(t, _)| t == "Contagem de visualizações")
        .expect("Contagem de visualizações option")
        .1
        .center();
    let apply_pos = text_rects
        .iter()
        .find(|(t, _)| t == "Aplicar")
        .expect("Aplicar button")
        .1
        .center();

    let click_views = vec![
        egui::Event::PointerMoved(views_pos),
        egui::Event::PointerButton {
            pos: views_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: views_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_views,
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.search_filters_modal_ui(&c);
        },
    );
    click_out.textures_delta.clear();

    assert_eq!(app.feed.search_filters.sort, search::SearchSort::Views);

    let click_apply = vec![
        egui::Event::PointerMoved(apply_pos),
        egui::Event::PointerButton {
            pos: apply_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: apply_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let mut apply_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_apply,
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.search_filters_modal_ui(&c);
        },
    );
    apply_out.textures_delta.clear();

    assert!(!app.feed.search_filters_open);
    assert!(!app.feed.search_filters_modal_open);
}

#[test]
fn search_unwatched_chip_filters_watched_history_videos() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.ui.search_results_mode = true;
    app.ui.browse_section = BrowseSection::Home;
    app.feed.query = "tutorial".into();

    app.history
        .data
        .record(history::Entry {
            video_id: "watched0001".into(),
            title: "Watched Video Title".into(),
            channel: "Canal A".into(),
            position_ms: 95_000,
            duration_ms: Some(300_000),
            watched_at_ms: None,
            ..Default::default()
        })
        .unwrap();

    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: Video {
                    title: "Watched Video Title".into(),
                    channel: "Canal A".into(),
                    url: "https://www.youtube.com/watch?v=watched0001".into(),
                    thumbnail: String::new(),
                    duration: "5:00".into(),
                },
                metadata: VideoMetadata::default(),
            },
            SearchResult {
                video: Video {
                    title: "Unwatched Video Title".into(),
                    channel: "Canal B".into(),
                    url: "https://www.youtube.com/watch?v=unwatch0002".into(),
                    thumbnail: String::new(),
                    duration: "8:20".into(),
                },
                metadata: VideoMetadata::default(),
            },
        ],
    );

    app.feed.search_chip = search::SearchChip::Unwatched;
    let mut out_unwatched = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.search_results_ui(ui, &c);
            });
        },
    );
    let rendered_unwatched = collect_rendered_text(&out_unwatched.shapes).join("\n");
    out_unwatched.textures_delta.clear();

    assert!(
        !rendered_unwatched.contains("Watched Video Title"),
        "rendered:\n{rendered_unwatched}"
    );
    assert!(
        rendered_unwatched.contains("Unwatched Video Title"),
        "rendered:\n{rendered_unwatched}"
    );

    app.feed.search_chip = search::SearchChip::All;
    let mut out_all = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.search_results_ui(ui, &c);
            });
        },
    );
    let rendered_all = collect_rendered_text(&out_all.shapes).join("\n");
    out_all.textures_delta.clear();
    assert!(rendered_all.contains("Watched Video Title"));
    assert!(rendered_all.contains("Unwatched Video Title"));
}

#[test]
fn search_save_and_report_modals_render_and_hide_reported_video() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let target_video = Video {
        title: "Resultado Suspeito".into(),
        channel: "Canal X".into(),
        url: "https://www.youtube.com/watch?v=report00001".into(),
        thumbnail: String::new(),
        duration: "4:12".into(),
    };
    app.apply_search_results(
        &ctx,
        vec![SearchResult {
            video: target_video.clone(),
            metadata: VideoMetadata::default(),
        }],
    );

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    app.feed.search_save_modal_video = Some(target_video.clone());
    let mut save_out = egui::FullOutput::default();
    for _ in 0..2 {
        save_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.search_save_modal_ui(&c);
            },
        );
        save_out.textures_delta.clear();
    }
    let save_rendered = collect_rendered_text(&save_out.shapes).join("\n");
    assert!(
        save_rendered.contains("Assistir mais tarde"),
        "save modal:\n{save_rendered}"
    );
    app.feed.search_save_modal_video = None;

    app.feed.search_report_modal_video = Some(target_video.clone());
    let mut report_out = egui::FullOutput::default();
    for _ in 0..2 {
        report_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.search_report_modal_ui(&c);
            },
        );
        report_out.textures_delta.clear();
    }
    let report_rects = collect_rendered_text_rects(&report_out.shapes);
    let report_rendered = report_rects
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        report_rendered.contains("Denunciar imagem ou título"),
        "report modal:\n{report_rendered}"
    );
    for expected_reason in [
        "Conteúdo sexual",
        "Conteúdo violento ou repulsivo",
        "Conteúdo abominável ou abusivo",
        "Atos perigosos ou nocivos",
        "Spam ou enganoso",
    ] {
        assert!(
            report_rendered.contains(expected_reason),
            "missing reason '{expected_reason}':\n{report_rendered}"
        );
    }

    let click_at = |pos: egui::Pos2| -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    };

    // Clicking "Denunciar" before picking a reason must keep the modal open.
    let btn_pos = report_rects
        .iter()
        .find(|(t, _)| t == "Denunciar")
        .expect("Denunciar button")
        .1
        .center();
    let mut disabled_click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_at(btn_pos),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.search_report_modal_ui(&c);
        },
    );
    disabled_click_out.textures_delta.clear();
    assert!(app.feed.search_report_modal_video.is_some());
    assert!(app.feed.search_reported_urls.is_empty());

    // Select "Spam ou enganoso" first.
    let reason_pos = report_rects
        .iter()
        .find(|(t, _)| t == "Spam ou enganoso")
        .expect("Spam ou enganoso option")
        .1
        .center();
    let mut select_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_at(reason_pos),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.search_report_modal_ui(&c);
        },
    );
    select_out.textures_delta.clear();
    assert_eq!(app.feed.search_report_reason, Some(4));

    // Now clicking "Denunciar" submits and hides the video.
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_at(btn_pos),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.search_report_modal_ui(&c);
        },
    );
    click_out.textures_delta.clear();

    assert!(app.feed.search_report_modal_video.is_none());
    assert!(app.feed.search_reported_urls.contains(&target_video.url));
    assert!(app.feed.videos.is_empty());
    assert!(app.message.contains("Spam ou enganoso"));
}

#[test]
fn subscriptions_swr_fresh_cache_skips_network_and_renders_instantly() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("token-1"));
    app.feed.home.subscriptions_disk_cache_loaded = true;
    app.feed.home.subscriptions_cached_loaded = true;
    app.feed.home.subscriptions_cached_at = Some(std::time::Instant::now());
    app.feed.home.subscriptions_cached_items = vec![test_sub_result(
        "dQw4w9WgXcQ",
        "Video Instantaneo",
        "Canal A",
        "UC1234567890123456789012",
    )];

    app.open_subscriptions_section(false, &ctx);

    assert_eq!(app.ui.browse_section, BrowseSection::Subscriptions);
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Video Instantaneo");
    assert!(!app.feed.home.stream_task.is_running());
    assert!(!app.feed.home.subscriptions_refreshing_in_background);
    assert!(!app.feed.home.subscriptions_progressive_streaming);
}

#[test]
fn subscriptions_swr_stale_disk_cache_displays_immediately_and_refreshes_in_background() {
    let dir = std::env::temp_dir().join(format!("fasttube-main-subs-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cache_path = dir.join("subscriptions-cache.json");

    let channel_id = "UC1234567890123456789012";
    let feed = search::HomeFeed {
        items: vec![test_sub_result(
            "dQw4w9WgXcQ",
            "Video do Disco",
            "Canal A",
            channel_id,
        )],
        playlists: Vec::new(),
        channels: vec![search::HomeChannel {
            title: "Canal A".into(),
            channel_id: channel_id.into(),
            avatar_url: None,
        }],
    };
    let mut rss_cache = std::collections::HashMap::new();
    rss_cache.insert(
        channel_id.to_owned(),
        oauth::ChannelRssCacheEntry {
            etag: Some("\"etag-disk\"".into()),
            last_modified: None,
            items: feed.items.clone(),
        },
    );
    let disk_cache = oauth::SubscriptionsDiskCache::from_feed(&feed, &rss_cache, 1);
    oauth::save_subscriptions_cache_to_path(&cache_path, &disk_cache).unwrap();

    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("token-1"));
    app.feed.home.subscriptions_disk_cache_path = Some(cache_path);

    app.open_subscriptions_section(false, &ctx);

    assert_eq!(app.ui.browse_section, BrowseSection::Subscriptions);
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Video do Disco");
    assert!(app.feed.home.subscription_channels_loaded);
    assert_eq!(
        app.feed
            .home
            .subscriptions_rss_cache
            .get(channel_id)
            .and_then(|entry| entry.etag.as_deref()),
        Some("\"etag-disk\"")
    );
    assert!(app.feed.home.stream_task.is_running());
    assert!(app.feed.home.subscriptions_refreshing_in_background);
    assert!(!app.feed.home.subscriptions_progressive_streaming);

    app.cancel_home_feed(&ctx);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn subscriptions_cold_open_streams_progress_directly_and_ignores_stale_request_id() {
    let dir =
        std::env::temp_dir().join(format!("fasttube-main-subs-stream-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cache_path = dir.join("subscriptions-cache.json");

    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    app.auth.authenticated = true;
    app.ui.browse_section = BrowseSection::Subscriptions;
    app.feed.home.subscriptions_disk_cache_path = Some(cache_path.clone());
    app.feed.home.subscriptions_progressive_streaming = true;
    let (sender, receiver) = mpsc::channel();
    app.feed
        .home
        .stream_task
        .attach_receiver(42, process::Cancellation::default(), receiver);

    // Stale progress from an older request_id must be discarded.
    sender
        .send((
            41,
            HomeFetchMessage::SubscriptionProgress {
                request_id: 41,
                progress: oauth::SubscriptionProgress {
                    items: vec![test_sub_result(
                        "aaaaaaaaaaa",
                        "Antigo",
                        "Canal Old",
                        "UC1111111111111111111111",
                    )],
                    channels: Vec::new(),
                    completed_channels: 1,
                    total_channels: 10,
                    reached_limit: false,
                },
            },
        ))
        .unwrap();
    // Active first batch should render immediately while still fetching.
    sender
        .send((
            42,
            HomeFetchMessage::SubscriptionProgress {
                request_id: 42,
                progress: oauth::SubscriptionProgress {
                    items: vec![test_sub_result(
                        "bbbbbbbbbbb",
                        "Lote Inicial",
                        "Canal B",
                        "UC2222222222222222222222",
                    )],
                    channels: vec![search::HomeChannel {
                        title: "Canal B".into(),
                        channel_id: "UC2222222222222222222222".into(),
                        avatar_url: None,
                    }],
                    completed_channels: 5,
                    total_channels: 20,
                    reached_limit: false,
                },
            },
        ))
        .unwrap();
    app.poll_async_tasks(&ctx);

    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Lote Inicial");
    assert!(app.feed.home.stream_task.is_running());

    let final_item = test_sub_result(
        "ccccccccccc",
        "Final Completo",
        "Canal B",
        "UC2222222222222222222222",
    );
    let mut rss_cache = std::collections::HashMap::new();
    rss_cache.insert(
        "UC2222222222222222222222".to_owned(),
        oauth::ChannelRssCacheEntry {
            etag: Some("\"etag-final\"".into()),
            last_modified: None,
            items: vec![final_item.clone()],
        },
    );
    sender
        .send((
            42,
            HomeFetchMessage::SubscriptionFinished {
                request_id: 42,
                token: Some(oauth::TokenSession::test_session("token-2")),
                result: Ok(oauth::SubscriptionFeedOutput {
                    feed: search::HomeFeed {
                        items: vec![final_item],
                        playlists: Vec::new(),
                        channels: vec![search::HomeChannel {
                            title: "Canal B".into(),
                            channel_id: "UC2222222222222222222222".into(),
                            avatar_url: None,
                        }],
                    },
                    rss_cache,
                    used_cached_channels: true,
                    rss_hits: 1,
                    not_modified_hits: 0,
                    api_fallbacks: 0,
                }),
            },
        ))
        .unwrap();
    app.poll_async_tasks(&ctx);

    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Final Completo");
    assert!(app.feed.home.subscriptions_cached_loaded);
    assert!(!app.feed.home.subscriptions_progressive_streaming);
    assert!(cache_path.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn subscriptions_background_refresh_keeps_visible_cards_and_respects_tab_switch() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    app.auth.authenticated = true;
    app.ui.browse_section = BrowseSection::Subscriptions;
    app.feed.videos = vec![Video {
        title: "Video em Cache".into(),
        channel: "Canal A".into(),
        url: "https://www.youtube.com/watch?v=ddddddddddd".into(),
        thumbnail: String::new(),
        duration: String::new(),
    }];
    app.feed.home.subscriptions_progressive_streaming = false;
    app.feed.home.subscriptions_refreshing_in_background = true;
    let (sender, receiver) = mpsc::channel();
    app.feed
        .home
        .stream_task
        .attach_receiver(7, process::Cancellation::default(), receiver);

    // Partial batch during background refresh must not jump cards on screen.
    sender
        .send((
            7,
            HomeFetchMessage::SubscriptionProgress {
                request_id: 7,
                progress: oauth::SubscriptionProgress {
                    items: vec![test_sub_result(
                        "eeeeeeeeeee",
                        "Parcial Novo",
                        "Canal A",
                        "UC1234567890123456789012",
                    )],
                    channels: Vec::new(),
                    completed_channels: 3,
                    total_channels: 10,
                    reached_limit: false,
                },
            },
        ))
        .unwrap();
    app.poll_async_tasks(&ctx);
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Video em Cache");

    // User switches to Home before the background sync finishes; cache must still update
    // without overwriting Home videos.
    app.ui.browse_section = BrowseSection::Home;
    app.feed.videos = vec![Video {
        title: "Video da Home".into(),
        channel: "Canal Home".into(),
        url: "https://www.youtube.com/watch?v=fffffffffff".into(),
        thumbnail: String::new(),
        duration: String::new(),
    }];
    sender
        .send((
            7,
            HomeFetchMessage::SubscriptionFinished {
                request_id: 7,
                token: Some(oauth::TokenSession::test_session("token-3")),
                result: Ok(oauth::SubscriptionFeedOutput {
                    feed: search::HomeFeed {
                        items: vec![test_sub_result(
                            "ggggggggggg",
                            "Atualizado em Background",
                            "Canal A",
                            "UC1234567890123456789012",
                        )],
                        playlists: Vec::new(),
                        channels: Vec::new(),
                    },
                    rss_cache: std::collections::HashMap::new(),
                    used_cached_channels: true,
                    rss_hits: 1,
                    not_modified_hits: 0,
                    api_fallbacks: 0,
                }),
            },
        ))
        .unwrap();
    app.poll_async_tasks(&ctx);

    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Video da Home");
    assert_eq!(app.feed.home.subscriptions_cached_items.len(), 1);
    assert_eq!(
        app.feed.home.subscriptions_cached_items[0].video.title,
        "Atualizado em Background"
    );
    assert!(!app.feed.home.subscriptions_refreshing_in_background);
}

#[test]
fn sign_out_clears_subscriptions_memory_rss_and_disk_cache() {
    let dir =
        std::env::temp_dir().join(format!("fasttube-main-subs-signout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cache_path = dir.join("subscriptions-cache.json");
    std::fs::write(&cache_path, b"{}").unwrap();

    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("token-signout"));
    app.feed.home.subscriptions_disk_cache_path = Some(cache_path.clone());
    app.feed.home.subscriptions_cached_loaded = true;
    app.feed.home.subscriptions_cached_at = Some(std::time::Instant::now());
    app.feed.home.subscriptions_cached_items = vec![test_sub_result(
        "hhhhhhhhhhh",
        "Privado",
        "Canal A",
        "UC1234567890123456789012",
    )];
    app.feed.home.subscriptions_rss_cache.insert(
        "UC1234567890123456789012".into(),
        oauth::ChannelRssCacheEntry::default(),
    );

    app.sign_out_and_reload_home(&ctx);

    assert!(!app.feed.home.subscriptions_cached_loaded);
    assert!(app.feed.home.subscriptions_cached_items.is_empty());
    assert!(app.feed.home.subscriptions_cached_at.is_none());
    assert!(app.feed.home.subscriptions_rss_cache.is_empty());
    assert!(!cache_path.exists());
    // Stop the background home fetch started by sign_out_and_reload_home.
    app.feed.home.batch_task.cancel();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn home_cards_show_explainability_badge_mute_actions_and_pinned_topics() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let mut sub_video = test_sub_result(
        "ddddddddddd",
        "Rust Async Deep Dive",
        "Canal Favorito",
        "UCfavorito123456789012",
    );
    sub_video.metadata.description = Some("Tutorial de Rust e tecnologia".into());
    app.feed.videos = vec![sub_video.video.clone()];
    app.video_metadata
        .insert(sub_video.video.url.clone(), sub_video.metadata);

    let _ = app.history.data.record(history::Entry {
        video_id: "eeeeeeeeeee".into(),
        title: "Rust Basics".into(),
        channel: "Canal Favorito".into(),
        position_ms: 400_000,
        duration_ms: Some(600_000),
        watched_at_ms: None,
        ..Default::default()
    });
    app.toggle_pinned_topic("Rust");
    assert!(app.is_topic_pinned("Rust"));

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1024.0, 720.0));
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.filter_chips_bar_ui(ui, &ctx);
                let _ = app.home_grid_ui(ui, &ctx);
            },
        );
        out.textures_delta.clear();
    }
    let rendered = collect_rendered_text_rects(&out.shapes)
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("Rust Async Deep Dive"),
        "missing home card title:\n{rendered}"
    );
    assert!(
        rendered.contains("Rust"),
        "missing pinned topic chip:\n{rendered}"
    );

    app.mute_channel_preference("Canal Favorito");
    app.mute_topic_preference("Rust");
    assert!(
        app.settings
            .prefs
            .value
            .muted_channels
            .iter()
            .any(|c| c == "canal favorito")
    );
    assert!(
        app.settings
            .prefs
            .value
            .muted_topics
            .iter()
            .any(|t| t == "rust")
    );
    app.ensure_feed_cache();
    let rank_entry = app
        .feed
        .cache
        .ranked
        .iter()
        .find(|s| s.video.url == sub_video.video.url);
    assert!(
        rank_entry.is_none_or(|s| s.score < 0.0),
        "muted channel + topic should downrank the video"
    );
}

#[test]
fn search_dropdown_supports_keyboard_nav_recent_delete_and_ignores_stale_suggest() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let palette = app.palette();
    app.feed.recent_searches = vec!["rust egui".into(), "fastube player".into()];
    app.feed.query = "rust".into();

    // Validate stale request_id filtering in poll_search_suggestions.
    let (tx, rx) = mpsc::channel();
    app.feed
        .search_suggest
        .task
        .attach_receiver(5, process::Cancellation::default(), rx);
    tx.send((
        4,
        app_state::SearchSuggestFetchMessage {
            request_id: 4,
            query: "rust".into(),
            result: Ok(vec!["stale suggestion".into()]),
        },
    ))
    .unwrap();
    app.poll_search_suggestions(&ctx);
    assert!(app.feed.search_suggest.remote_suggestions.is_empty());

    let (tx2, rx2) = mpsc::channel();
    app.feed
        .search_suggest
        .task
        .attach_receiver(6, process::Cancellation::default(), rx2);
    tx2.send((
        6,
        app_state::SearchSuggestFetchMessage {
            request_id: 6,
            query: "rust".into(),
            result: Ok(vec!["rust tutorial".into()]),
        },
    ))
    .unwrap();
    app.poll_search_suggestions(&ctx);
    assert_eq!(
        app.feed.search_suggest.remote_suggestions,
        vec!["rust tutorial".to_owned()]
    );

    // Focus search box via click and verify merged suggestions render.
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(960.0, 600.0));
    let mut pre_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            app.header_search_ui(ui, &ctx, palette, 420.0);
        },
    );
    pre_out.textures_delta.clear();
    let pre_rects = collect_rendered_text_rects(&pre_out.shapes);
    let click_input_pos = pre_rects
        .iter()
        .find(|(t, _)| t == "rust")
        .map(|(_, r)| egui::pos2(r.right() + 20.0, r.center().y))
        .expect("expected search input text rect");
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(click_input_pos),
                egui::Event::PointerButton {
                    pos: click_input_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_input_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            app.header_search_ui(ui, &ctx, palette, 420.0);
        },
    );
    click_out.textures_delta.clear();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            app.header_search_ui(ui, &ctx, palette, 420.0);
        },
    );
    out.textures_delta.clear();
    let rects = collect_rendered_text_rects(&out.shapes);
    let rendered = rects
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("rust egui") && rendered.contains("rust tutorial"),
        "dropdown should show merged recent + remote suggestions:\n{rendered}"
    );

    // Delete the recent search entry via its "×" button in the dropdown.
    let del_pos = rects
        .iter()
        .filter(|(t, r)| t == "\u{00d7}" && r.top() > click_input_pos.y + 18.0)
        .map(|(_, r)| r.center())
        .next()
        .expect("expected delete × button on recent search dropdown row");
    let mut del_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(del_pos),
                egui::Event::PointerButton {
                    pos: del_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: del_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            app.header_search_ui(ui, &ctx, palette, 420.0);
        },
    );
    del_out.textures_delta.clear();
    assert!(
        !app.feed.recent_searches.iter().any(|r| r == "rust egui"),
        "clicking × should remove the recent search entry"
    );

    // Re-focus input, let focus settle, then press ArrowDown + Enter to submit suggestion.
    let mut refocus_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(click_input_pos),
                egui::Event::PointerButton {
                    pos: click_input_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_input_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            app.header_search_ui(ui, &ctx, palette, 420.0);
        },
    );
    refocus_out.textures_delta.clear();
    let mut down_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| {
            app.header_search_ui(ui, &ctx, palette, 420.0);
        },
    );
    down_out.textures_delta.clear();
    assert_eq!(app.feed.search_suggest.highlighted_index, Some(0));

    let mut enter_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| {
            app.header_search_ui(ui, &ctx, palette, 420.0);
        },
    );
    enter_out.textures_delta.clear();
    assert_eq!(app.feed.query, "rust tutorial");
    assert!(app.ui.search_results_mode);
    app.feed.search.task.cancel();
    app.feed.search_page_task.cancel();
}

#[test]
fn shorts_watch_page_supports_jk_navigation_index_counter_and_loop_toggle() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let s1 = Video {
        title: "Short Um".into(),
        channel: "Canal Shorts".into(),
        url: "https://www.youtube.com/shorts/short000001".into(),
        thumbnail: String::new(),
        duration: "0:25".into(),
    };
    let s2 = Video {
        title: "Short Dois".into(),
        channel: "Canal Shorts".into(),
        url: "https://www.youtube.com/shorts/short000002".into(),
        thumbnail: String::new(),
        duration: "0:30".into(),
    };
    let s3 = Video {
        title: "Short Tres".into(),
        channel: "Canal Shorts".into(),
        url: "https://www.youtube.com/shorts/short000003".into(),
        thumbnail: String::new(),
        duration: "0:35".into(),
    };
    app.video_metadata.insert(
        s1.url.clone(),
        VideoMetadata {
            kind: search::SearchResultKind::Short,
            description: Some("Desc completa do Short Um".into()),
            view_count_text: Some("38 mil visualiza\u{00e7}\u{00f5}es".into()),
            published_time_text: Some("h\u{00e1} 3 horas".into()),
            ..Default::default()
        },
    );
    app.feed.videos = vec![s1.clone(), s2.clone(), s3.clone()];
    app.ui.browse_section = BrowseSection::Shorts;
    app.feed.filter = recommendations::FeedFilter::Shorts;
    app.selected = Some(s1.clone());
    app.watch.extras.sync_video(Some(&s1.url));
    app.watch.extras.like_count_text = Some("7,5 mil".into());

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
    let rects = collect_rendered_text_rects(&out.shapes);
    let rendered = rects
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("1/3"), "missing index pill:\n{rendered}");
    assert!(
        !rendered.contains("Remix"),
        "Remix must be omitted:\n{rendered}"
    );
    assert!(
        !rendered.contains("promo\u{00e7}\u{00e3}o paga"),
        "paid promotion must be omitted:\n{rendered}"
    );

    // Click Short title to open Description panel, then close it again.
    let title_pos = rects
        .iter()
        .find(|(t, _)| t.contains("Short Um"))
        .map(|(_, r)| r.center())
        .expect("Short title rect");
    let click_at = |ctx: &egui::Context, app: &mut FastTube, pos: egui::Pos2| {
        let mut click_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| {
                let _ = app.shorts_watch_page_ui(ui, ctx);
            },
        );
        click_out.textures_delta.clear();
        click_out
    };

    let _ = click_at(&ctx, &mut app, title_pos);
    assert_eq!(
        app.watch.shorts_side_panel,
        app_state::ShortsSidePanel::Description
    );
    let mut desc_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let _ = app.shorts_watch_page_ui(ui, &ctx);
        },
    );
    desc_out.textures_delta.clear();
    let desc_rendered = collect_rendered_text(&desc_out.shapes).join("\n");
    assert!(
        desc_rendered.contains("Marca\u{00e7}\u{00f5}es Gostei"),
        "{desc_rendered}"
    );
    assert!(
        desc_rendered.contains("Desc completa do Short Um"),
        "{desc_rendered}"
    );

    // Close Description panel so centered stage geometry returns cleanly.
    let rects_desc = collect_rendered_text_rects(&desc_out.shapes);
    let overlay_title_pos = rects_desc
        .iter()
        .filter(|(t, _)| t.contains("Short Um"))
        .min_by(|a, b| a.1.left().total_cmp(&b.1.left()))
        .map(|(_, r)| r.center())
        .expect("overlay title rect");
    let _ = click_at(&ctx, &mut app, overlay_title_pos);
    assert_eq!(
        app.watch.shorts_side_panel,
        app_state::ShortsSidePanel::None
    );

    // Find top-right ⋮ circular button (radius ~20.0 with largest center.x) and open menu to toggle Loop.
    let mut settle = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let _ = app.shorts_watch_page_ui(ui, &ctx);
        },
    );
    settle.textures_delta.clear();

    fn find_more_circle(shape: &egui::Shape, best: &mut Option<egui::Pos2>) {
        match shape {
            egui::Shape::Circle(c) if (c.radius - 20.0).abs() < 0.6 => {
                if best.is_none_or(|b| c.center.x > b.x) {
                    *best = Some(c.center);
                }
            }
            egui::Shape::Vec(items) => {
                for item in items {
                    find_more_circle(item, best);
                }
            }
            _ => {}
        }
    }
    let mut more_pos = None;
    for clipped in &settle.shapes {
        find_more_circle(&clipped.shape, &mut more_pos);
    }
    let more_pos = more_pos.expect("top-right more button circle");
    let _ = click_at(&ctx, &mut app, more_pos);
    let mut menu_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let _ = app.shorts_watch_page_ui(ui, &ctx);
        },
    );
    menu_out.textures_delta.clear();
    let loop_pos = collect_rendered_text_rects(&menu_out.shapes)
        .into_iter()
        .find(|(t, _)| t.contains("Repetir Short"))
        .map(|(_, r)| r.center())
        .expect("Repetir Short menu item");
    let _ = click_at(&ctx, &mut app, loop_pos);
    assert!(app.playback.loop_video);
    assert!(!app.toasts.is_empty());

    // Press J to advance to next Short.
    let mut next_short = None;
    let mut j_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::Key {
                key: egui::Key::J,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| {
            next_short = app.shorts_watch_page_ui(ui, &ctx);
        },
    );
    j_out.textures_delta.clear();
    assert_eq!(next_short.as_ref().map(|v| &v.url), Some(&s2.url));

    // Switch to s2, verify "2/3" and press K to return to s1.
    app.selected = Some(s2);
    let mut prev_short = None;
    let mut out2 = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::Key {
                key: egui::Key::K,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| {
            prev_short = app.shorts_watch_page_ui(ui, &ctx);
        },
    );
    out2.textures_delta.clear();
    let rendered2 = collect_rendered_text_rects(&out2.shapes)
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered2.contains("2/3"), "missing 2/3 pill:\n{rendered2}");
    assert_eq!(prev_short.as_ref().map(|v| &v.url), Some(&s1.url));
}

#[test]
fn shorts_tab_auto_selects_first_short_and_excludes_regular_videos() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let long_video = Video {
        title: "Documentario Longo 45min".into(),
        channel: "Canal Doc".into(),
        url: "https://www.youtube.com/watch?v=longdoc1111".into(),
        thumbnail: String::new(),
        duration: "45:10".into(),
    };
    let short_one = Video {
        title: "Primeiro Short Vertical".into(),
        channel: "Criador Short".into(),
        url: "https://www.youtube.com/shorts/short_first1".into(),
        thumbnail: String::new(),
        duration: "0:32".into(),
    };
    let short_two = Video {
        title: "Segundo Short Vertical".into(),
        channel: "Criador Short".into(),
        url: "https://www.youtube.com/watch?v=short_second2".into(),
        thumbnail: String::new(),
        duration: "0:48".into(),
    };
    app.video_metadata.insert(
        short_two.url.clone(),
        VideoMetadata {
            kind: search::SearchResultKind::Short,
            ..Default::default()
        },
    );
    app.feed.videos = vec![long_video.clone(), short_one.clone(), short_two.clone()];

    // Entering Shorts tab schedules the first genuine Short (skipping long_video) for immediate playback.
    app.open_shorts_tab(&ctx);
    assert_eq!(app.ui.browse_section, BrowseSection::Shorts);
    assert_eq!(
        app.playback.pending_open_video.as_ref().map(|v| &v.url),
        Some(&short_one.url)
    );

    // Candidate sequence only includes genuine Shorts even while BrowseSection::Shorts is active.
    let candidates: Vec<&str> = app
        .shorts_feed_candidates()
        .into_iter()
        .map(|v| v.url.as_str())
        .collect();
    assert_eq!(
        candidates,
        vec![short_one.url.as_str(), short_two.url.as_str()]
    );

    // Rendering browse_page_ui while on BrowseSection::Shorts with no active video returns the first Short directly.
    app.playback.pending_open_video = None;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 760.0));
    let mut chosen = None;
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            chosen = app.browse_page_ui(ui, &ctx);
        },
    );
    out.textures_delta.clear();
    assert_eq!(chosen.as_ref().map(|v| &v.url), Some(&short_one.url));
}

#[test]
fn subscriptions_page_renders_youtube_layout_and_global_shorts_mode() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.ui.browse_section = BrowseSection::Subscriptions;
    app.settings.prefs.value.hide_shorts_in_home = true;

    let mut videos = Vec::new();
    for i in 1..=4 {
        let v = Video {
            title: format!("Regular Sub Video {i}"),
            channel: format!("Channel {i}"),
            url: format!("https://www.youtube.com/watch?v=regsub0000{i}"),
            thumbnail: String::new(),
            duration: "18:20".into(),
        };
        app.video_metadata.insert(
            v.url.clone(),
            VideoMetadata {
                upload_date: Some(format!("2026111{}", 9 - i)),
                view_count_text: Some("21 mil visualiza\u{00e7}\u{00f5}es".into()),
                published_time_text: Some("h\u{00e1} 2 horas".into()),
                ..Default::default()
            },
        );
        videos.push(v);
    }
    let short_by_kind = Video {
        title: "Sub Short Kind".into(),
        channel: "Short Creator".into(),
        url: "https://www.youtube.com/watch?v=subshortk01".into(),
        thumbnail: String::new(),
        duration: String::new(),
    };
    app.video_metadata.insert(
        short_by_kind.url.clone(),
        VideoMetadata {
            kind: search::SearchResultKind::Short,
            upload_date: Some("20261118".into()),
            view_count: Some(12_000),
            ..Default::default()
        },
    );
    videos.push(short_by_kind.clone());
    app.feed.videos = videos;
    app.feed.generation += 1;

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 820.0));
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let palette = app.palette();
                    app.browse_subscriptions_header_ui(ui, &ctx, palette);
                    let _ = app.home_grid_ui(ui, &ctx);
                });
            },
        );
        out.textures_delta.clear();
    }
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    assert!(rendered.contains("Mais recentes"), "{rendered}");
    assert!(
        rendered.contains("Todas as inscri\u{00e7}\u{00f5}es"),
        "{rendered}"
    );
    assert!(rendered.contains("Mais relevantes"), "{rendered}");
    assert!(rendered.contains("Mostrar mais"), "{rendered}");
    assert!(rendered.contains("Shorts"), "{rendered}");
    assert!(rendered.contains("Ver tudo"), "{rendered}");
    assert!(rendered.contains("Sub Short Kind"), "{rendered}");
    // Default filter bar stays collapsed until toggled or active.
    assert!(!rendered.contains("N\u{00e3}o assistidos"), "{rendered}");

    // Opening a Short identified by SearchResultKind::Short uses global Shorts player even on Home.
    app.ui.browse_section = BrowseSection::Home;
    app.selected = Some(short_by_kind);
    assert!(app.is_watch_short_mode());
}

#[test]
fn header_search_bar_opens_direct_video_url_and_channel_handle() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let palette = app.palette();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 120.0));

    // Direct video URL with timestamp
    app.feed.query = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=75".into();
    let mut search_btn_rect = egui::Rect::NOTHING;
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_search_ui(ui, &c, palette, 520.0);
            search_btn_rect = ui.min_rect();
        },
    );
    out.textures_delta.clear();
    let click_pos = egui::pos2(search_btn_rect.right() - 24.0, search_btn_rect.center().y);
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_search_ui(ui, &c, palette, 520.0);
        },
    );
    click_out.textures_delta.clear();
    assert!(!app.ui.search_results_mode);
    assert_eq!(app.playback.pending_open_seek_ms, Some(75_000));
    assert_eq!(
        app.playback
            .pending_open_video
            .as_ref()
            .map(|v| v.url.as_str()),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );

    // Direct @handle opens channel directly
    app.playback.pending_open_video = None;
    app.feed.query = "@rustlang".into();
    let mut click_handle = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_search_ui(ui, &c, palette, 520.0);
        },
    );
    click_handle.textures_delta.clear();
    app.channels.videos.task.cancel();
    assert!(!app.ui.search_results_mode);
    assert!(app.channels.view_open);
    assert_eq!(
        app.channels.last_target,
        Some(ChannelTarget::Handle("@rustlang".to_owned()))
    );
}

#[test]
fn grid_card_hover_quick_actions_trigger_without_opening_video() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let scored = ScoredVideo {
        video: Video {
            title: "FastTube UI Walkthrough".into(),
            channel: "Rust Channel".into(),
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            thumbnail: String::new(),
            duration: "8:20".into(),
        },
        score: 100.0,
        metadata: VideoMetadata::default(),
        is_short: false,
        reasons: Vec::new(),
        matched_topic: None,
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 340.0));
    let hover_pos = egui::pos2(160.0, 60.0);

    // Frame 1: hover over thumbnail reveals quick actions.
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::PointerMoved(hover_pos)],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.youtube_grid_card_ui(ui, &c, &scored, 300.0);
        },
    );
    fn collect_quick_action_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Rect> {
        let mut quick = Vec::new();
        fn visit(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
            match shape {
                egui::Shape::Rect(r)
                    if (r.rect.width() - 28.0).abs() < 0.5
                        && (r.rect.height() - 28.0).abs() < 0.5 =>
                {
                    out.push(r.rect);
                }
                egui::Shape::Vec(items) => {
                    for item in items {
                        visit(item, out);
                    }
                }
                _ => {}
            }
        }
        for clipped in shapes {
            visit(&clipped.shape, &mut quick);
        }
        quick.sort_by(|a, b| a.top().total_cmp(&b.top()));
        quick
    }
    let quick_rects = collect_quick_action_rects(&out.shapes);
    out.textures_delta.clear();
    assert!(
        quick_rects.len() >= 2,
        "expected 2 hover quick action rects, found {}",
        quick_rects.len()
    );
    let wl_pos = quick_rects[0].center();
    let fila_pos = quick_rects[1].center();

    // Click "+ Fila"
    let mut fila_actions = HomeCardActions::default();
    let mut out_fila = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(fila_pos),
                egui::Event::PointerButton {
                    pos: fila_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: fila_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            fila_actions = app.youtube_grid_card_ui(ui, &c, &scored, 300.0);
        },
    );
    out_fila.textures_delta.clear();
    assert!(fila_actions.enqueue_clicked);
    assert!(!fila_actions.open_clicked);

    // Click "Assistir depois"
    let mut wl_actions = HomeCardActions::default();
    let mut out_wl = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(wl_pos),
                egui::Event::PointerButton {
                    pos: wl_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: wl_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            wl_actions = app.youtube_grid_card_ui(ui, &c, &scored, 300.0);
        },
    );
    out_wl.textures_delta.clear();
    assert!(wl_actions.toggle_watch_later);
    assert!(!wl_actions.open_clicked);
}

#[test]
fn filter_chips_bar_scrolls_horizontally_via_left_and_right_buttons() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.feed.videos = (0..12)
        .map(|i| Video {
            title: format!("Video de Musica Rock Ao Vivo {i}"),
            channel: format!("Canal Muito Longo Numero {i}"),
            url: format!("https://www.youtube.com/watch?v=vid000000{i:02}"),
            thumbnail: String::new(),
            duration: "10:00".into(),
        })
        .collect();
    app.invalidate_feed_cache();

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(260.0, 80.0));
    let mut bar_rect = egui::Rect::NOTHING;
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.filter_chips_bar_ui(ui, &c);
            bar_rect = ui.min_rect();
        },
    );
    out.textures_delta.clear();
    assert_eq!(app.ui.chips_scroll_offset, 0.0);

    // Click right chevron (last 32x32 button inside bar_rect)
    let right_pos = egui::pos2(bar_rect.right() - 16.0, bar_rect.center().y);
    let mut out_right = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(right_pos),
                egui::Event::PointerButton {
                    pos: right_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: right_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.filter_chips_bar_ui(ui, &c);
        },
    );
    out_right.textures_delta.clear();
    assert!(
        app.ui.chips_scroll_offset > 0.0,
        "expected chips_scroll_offset > 0 after clicking right arrow, got {}",
        app.ui.chips_scroll_offset
    );

    // Click left chevron (first 32x32 button inside bar_rect)
    let left_pos = egui::pos2(bar_rect.left() + 16.0, bar_rect.center().y);
    let mut out_left = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(left_pos),
                egui::Event::PointerButton {
                    pos: left_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: left_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.filter_chips_bar_ui(ui, &c);
        },
    );
    out_left.textures_delta.clear();
    assert_eq!(app.ui.chips_scroll_offset, 0.0);
}

#[test]
fn youtube_parity_removes_non_youtube_tabs_and_exposes_youtube_controls_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.feed.videos = sample_videos();

    // 1. Expanded sidebar keeps official YouTube sections and removes non-YouTube tabs.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 960.0));
    let mut nav_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.navigation_ui(ui, &c);
        },
    );
    let nav_text = collect_rendered_text(&nav_out.shapes).join("\n");
    nav_out.textures_delta.clear();
    for expected in [
        "In\u{00ed}cio",
        "Shorts",
        "Inscri\u{00e7}\u{00f5}es",
        "Voc\u{00ea}",
        "Hist\u{00f3}rico",
        "Playlists",
        "Seu canal",
        "Assistir mais tarde",
        "Downloads",
        "Explorar",
        "Em alta",
        "M\u{00fa}sica",
        "Ao vivo",
        "Jogos",
        "Podcasts",
        "Configura\u{00e7}\u{00f5}es",
    ] {
        assert!(
            nav_text.contains(expected),
            "expected official YouTube sidebar item '{expected}', got:\n{nav_text}"
        );
    }
    for removed in [
        "Insights",
        "Fila de reprodu\u{00e7}\u{00e3}o",
        "Mixes",
        "Novidades",
        "Assistindo",
        "Fechar v\u{00ed}deo",
    ] {
        assert!(
            !nav_text.contains(removed),
            "non-YouTube sidebar item '{removed}' should be removed, got:\n{nav_text}"
        );
    }

    // 2. Compact 72px rail only renders Home, Shorts, Subscriptions, You.
    let mut rail_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.compact_navigation_ui(ui, &c);
        },
    );
    let rail_text = collect_rendered_text(&rail_out.shapes).join("\n");
    rail_out.textures_delta.clear();
    assert!(rail_text.contains("In\u{00ed}cio"));
    assert!(rail_text.contains("Shorts"));
    assert!(rail_text.contains("Inscri\u{00e7}\u{00f5}es"));
    assert!(rail_text.contains("Voc\u{00ea}"));
    assert!(!rail_text.contains("Downloads"));
    assert!(!rail_text.contains("Insights"));

    // 3. Header omits non-YouTube Ctrl+K button.
    let mut hdr_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    let hdr_text = collect_rendered_text(&hdr_out.shapes).join("\n");
    hdr_out.textures_delta.clear();
    assert!(!hdr_text.contains("Ctrl+K"));

    // 4. Command palette exposes only YouTube sections (no Insights, Queue, or Explore hub).
    let palette_items = app.build_command_palette_items();
    assert!(!palette_items.iter().any(|i| matches!(
        i.action,
        command_palette::PaletteAction::Browse(
            BrowseSection::Insights | BrowseSection::Queue | BrowseSection::Explore
        )
    )));

    // 5. Watch page omits standalone "Fila" action button and "Transcrição" sidebar chip;
    //    expanded description reveals "Mostrar transcrição", and Transcript view hides chips.
    let video = app.feed.videos[0].clone();
    app.selected = Some(video.clone());
    app.video_metadata.insert(
        video.url.clone(),
        VideoMetadata {
            description: Some("Linha 1\nLinha 2\nLinha 3\nLinha 4".into()),
            ..Default::default()
        },
    );
    app.ensure_watch_metadata_cache(&video);
    app.watch.metadata_cache.description_expanded = true;
    app.watch.extras.comments_loaded = true;
    let mut watch_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.watch_metadata_card_ui(ui, &video, 820.0, None);
            let _ = app.watch_sidebar_results_ui(ui, &c, "parity_watch_sidebar");
        },
    );
    let watch_tokens = collect_rendered_text(&watch_out.shapes);
    watch_out.textures_delta.clear();
    assert!(
        watch_tokens
            .iter()
            .any(|t| t == "Mostrar transcri\u{00e7}\u{00e3}o"),
        "expanded watch description must show official YouTube 'Mostrar transcrição' button"
    );
    assert!(
        !watch_tokens.iter().any(|t| t == "Fila"),
        "watch action bar should move Queue into overflow menu instead of standalone 'Fila' pill"
    );
    assert!(
        !watch_tokens.iter().any(|t| t == "Transcri\u{00e7}\u{00e3}o"
            && watch_tokens.contains(&"Todos".to_owned())
            && app.watch.extras.sidebar_tab == watch_extras::SidebarTab::All)
            || watch_tokens
                .iter()
                .filter(|t| t.as_str() == "Transcri\u{00e7}\u{00e3}o")
                .count()
                == 1,
        "sidebar chips must not show 'Transcrição'; only expanded description heading shows it"
    );

    app.watch.extras.open_transcript();
    let mut tr_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_sidebar_results_ui(ui, &c, "parity_watch_transcript");
        },
    );
    let tr_tokens = collect_rendered_text(&tr_out.shapes);
    let close_pos =
        find_rendered_text_center(&tr_out.shapes, "\u{2715}").expect("Close transcript ×");
    tr_out.textures_delta.clear();
    assert!(tr_tokens.iter().any(|t| t == "Transcri\u{00e7}\u{00e3}o"));
    assert!(
        tr_tokens.iter().any(|t| t == "Todos") && tr_tokens.iter().any(|t| t == "Relacionados"),
        "Transcript panel should dock above sidebar chips on official YouTube"
    );
    let _ = headless_pointer_click(&ctx, screen.size(), close_pos, |ui| {
        let c = ui.ctx().clone();
        let _ = app.watch_sidebar_results_ui(ui, &c, "parity_watch_transcript");
    });
    assert!(!app.watch.extras.is_transcript_open());

    // 6. Empty "Você" (Library) omits non-YouTube empty Favorites and Bookmarks shelves.
    let palette = app.palette();
    let mut lib_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    let lib_text = collect_rendered_text(&lib_out.shapes).join("\n");
    lib_out.textures_delta.clear();
    assert!(lib_text.contains("Assistir mais tarde"));
    assert!(lib_text.contains("Playlists"));
    assert!(!lib_text.contains("Favoritos"));
    assert!(!lib_text.contains("Marcadores"));

    // 7. History page shows official YouTube title and Pause Watch History toggle.
    let mut hist_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "parity_history", false, None, None);
        },
    );
    let hist_text = collect_rendered_text(&hist_out.shapes).join("\n");
    hist_out.textures_delta.clear();
    assert!(hist_text.contains("Hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o"));
    assert!(hist_text.contains("Pausar o hist\u{00f3}rico de visualiza\u{00e7}\u{00f5}es"));
    let pause_pos = find_rendered_text_center(
        &hist_out.shapes,
        "Pausar o hist\u{00f3}rico de visualiza\u{00e7}\u{00f5}es",
    )
    .expect("pause btn");
    let _ = headless_pointer_click(&ctx, screen.size(), pause_pos, |ui| {
        let _ = app.history_section_ui(ui, "parity_history", false, None, None);
    });
    assert!(app.settings.prefs.value.pause_watch_history);
}

#[test]
fn sidebar_youtube_labs_explore_and_report_history_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 960.0));

    let mut collapsed_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.navigation_ui(ui, &c);
        },
    );
    let collapsed_text = collect_rendered_text(&collapsed_out.shapes).join("\n");
    collapsed_out.textures_delta.clear();
    for expected in [
        "Mais do YouTube",
        "YouTube Labs",
        "Filmes",
        "Mostrar mais",
        "Histórico de denúncias",
    ] {
        assert!(
            collapsed_text.contains(expected),
            "expected sidebar item '{expected}', got:\n{collapsed_text}"
        );
    }
    for forbidden in ["YouTube Premium", "YouTube Music", "YouTube Kids"] {
        assert!(
            !collapsed_text.contains(forbidden),
            "red-X item '{forbidden}' must not appear in sidebar, got:\n{collapsed_text}"
        );
    }

    app.ui.explore_expanded = true;
    let mut expanded_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.navigation_ui(ui, &c);
        },
    );
    let expanded_text = collect_rendered_text(&expanded_out.shapes).join("\n");
    expanded_out.textures_delta.clear();
    for expected in ["Sala de Jogos", "Assinaturas", "Mostrar menos"] {
        assert!(
            expanded_text.contains(expected),
            "expected expanded Explore item '{expected}', got:\n{expanded_text}"
        );
    }
}

#[test]
fn your_channel_empty_videos_tab_recovery_and_studio_buttons_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let my_target = search::ChannelTarget::ChannelId("UCx20-fNue2nSzLQtuWeFD9g".into());
    app.auth.my_channel = Some(my_target.clone());
    app.auth.my_channel_title = Some("Meu Canal Teste".into());

    let (req_id, valid_target) = app
        .begin_channel_request(my_target, &ctx)
        .expect("should initiate channel request");
    assert!(app.complete_channel_request(
        req_id,
        &valid_target,
        Err(
            "Não foi possível consultar o canal: ERROR: [youtube:tab] UCx20-fNue2nSzLQtuWeFD9g: This channel does not have a videos tab"
                .into()
        ),
        &ctx,
    ));
    assert!(app.channels.error.is_none());
    assert_eq!(
        app.channels.page.as_ref().and_then(|p| p.title.as_deref()),
        Some("Meu Canal Teste")
    );

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.channel_section_ui(ui, &c, "test_my_chan", false, None);
        },
    );
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();
    assert!(
        rendered.contains("Personalizar o canal") && rendered.contains("Gerenciar vídeos"),
        "expected Studio management buttons on own channel, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("Inscrever-se"),
        "own channel must not render subscribe button, got:\n{rendered}"
    );
}

#[test]
fn poll_local_followed_feed_rejects_stale_request_ids_and_applies_active() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.auth.authenticated = false;
    app.ui.browse_section = BrowseSection::Subscriptions;
    let (sender, receiver) = mpsc::channel();
    app.followed_channels
        .feed_task
        .attach_receiver(2, process::Cancellation::default(), receiver);

    let stale_item = SearchResult {
        video: Video {
            title: "Stale Upload".into(),
            channel: "Old Channel".into(),
            url: "https://www.youtube.com/watch?v=stale000001".into(),
            thumbnail: String::new(),
            duration: "4:00".into(),
        },
        metadata: VideoMetadata::default(),
    };
    let fresh_item = SearchResult {
        video: Video {
            title: "Fresh Upload".into(),
            channel: "Followed Channel".into(),
            url: "https://www.youtube.com/watch?v=fresh000001".into(),
            thumbnail: String::new(),
            duration: "6:15".into(),
        },
        metadata: VideoMetadata::default(),
    };

    sender
        .send((
            1,
            Ok(followed_channels::LocalFeedOutput {
                items: vec![stale_item],
                new_video_urls: HashSet::new(),
                latest_ids_by_channel: HashMap::new(),
                errors: 0,
            }),
        ))
        .unwrap();
    app.poll_local_followed_feed(&ctx);
    assert!(app.followed_channels.feed_items.is_empty());
    assert!(app.followed_channels.loaded_at.is_none());
    assert!(app.followed_channels.feed_task.is_running());
    assert_eq!(app.followed_channels.feed_task.active_request_id(), Some(2));

    sender
        .send((
            2,
            Ok(followed_channels::LocalFeedOutput {
                items: vec![fresh_item.clone()],
                new_video_urls: HashSet::from([fresh_item.video.url.clone()]),
                latest_ids_by_channel: HashMap::new(),
                errors: 0,
            }),
        ))
        .unwrap();
    app.poll_local_followed_feed(&ctx);
    assert_eq!(app.followed_channels.feed_items.len(), 1);
    assert_eq!(
        app.followed_channels.feed_items[0].video.title,
        "Fresh Upload"
    );
    assert!(app.followed_channels.loaded_at.is_some());
    assert!(!app.followed_channels.feed_task.is_running());
    assert_eq!(app.followed_channels.feed_task.active_request_id(), None);
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Fresh Upload");
}

#[test]
fn fast_home_query_plan_respects_muted_prefs_pause_and_followed_channels() {
    let mut app = FastTube::default();
    app.history
        .data
        .record(history::Entry {
            video_id: "hist0000001".into(),
            title: "Rust async tokio deep dive".into(),
            channel: "Muted Creator".into(),
            position_ms: 120_000,
            duration_ms: Some(300_000),
            watched_at_ms: Some(1_762_728_000_000),
            ..Default::default()
        })
        .unwrap();
    app.followed_channels
        .data
        .channels
        .push(followed_channels::FollowedChannel {
            channel_id: "UC1234567890123456789012".into(),
            title: "Followed Rust Lab".into(),
            handle: None,
            url: "https://www.youtube.com/channel/UC1234567890123456789012".into(),
            avatar_url: None,
            followed_at_ms: 1,
            last_seen_video_id: None,
        });
    app.followed_channels
        .data
        .channels
        .push(followed_channels::FollowedChannel {
            channel_id: "UC9999999999999999999999".into(),
            title: "Muted Creator".into(),
            handle: None,
            url: "https://www.youtube.com/channel/UC9999999999999999999999".into(),
            avatar_url: None,
            followed_at_ms: 1,
            last_seen_video_id: None,
        });
    app.settings.prefs.value.muted_topics = vec!["rust".into()];
    app.settings.prefs.value.muted_channels = vec!["Muted Creator".into()];

    let plan = app.build_fast_home_query_plan(0);
    assert!(
        plan.video_queries
            .iter()
            .all(|q| !q.to_lowercase().contains("rust")
                && !q.to_lowercase().contains("muted creator")),
        "expected muted topics/channels excluded from video_queries, got {:?}",
        plan.video_queries
    );
    assert!(
        plan.channel_targets
            .contains(&ChannelTarget::ChannelId("UC1234567890123456789012".into())),
        "expected unmuted followed channel in channel_targets, got {:?}",
        plan.channel_targets
    );
    assert!(
        !plan
            .channel_targets
            .contains(&ChannelTarget::ChannelId("UC9999999999999999999999".into())),
        "expected muted followed channel excluded from channel_targets, got {:?}",
        plan.channel_targets
    );

    // Pausing recommendations ignores history/queue topics and channels.
    app.settings.prefs.value.muted_topics.clear();
    app.settings.prefs.value.pause_recommendations = true;
    let paused_plan = app.build_fast_home_query_plan(0);
    assert!(
        paused_plan
            .video_queries
            .iter()
            .all(|q| !q.to_lowercase().contains("tokio")),
        "expected history topics ignored when recommendations are paused, got {:?}",
        paused_plan.video_queries
    );
}

#[test]
fn poll_home_handles_subscriptions_section_in_english_language() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.settings.prefs.value.language = i18n::UiLanguage::En;
    app.auth.authenticated = true;
    app.ui.browse_section = BrowseSection::Subscriptions;
    app.feed.home.title = Some("Subscriptions: recent uploads".into());

    let (req_id, _cancel, sender) = app.feed.home.stream_task.begin_manual();
    let channel = search::HomeChannel {
        title: "English Channel".into(),
        channel_id: "UCenglish0001".into(),
        avatar_url: None,
    };
    let item = SearchResult {
        video: Video {
            title: "English Upload".into(),
            channel: "English Channel".into(),
            url: "https://www.youtube.com/watch?v=engsub00001".into(),
            thumbnail: String::new(),
            duration: "9:30".into(),
        },
        metadata: VideoMetadata::default(),
    };
    sender
        .send((
            req_id,
            HomeFetchMessage::Progress(oauth::SubscriptionProgress {
                items: vec![item.clone()],
                channels: vec![channel.clone()],
                completed_channels: 1,
                total_channels: 1,
                reached_limit: false,
            }),
        ))
        .unwrap();
    sender
        .send((
            req_id,
            HomeFetchMessage::Finished(
                None,
                Ok(search::HomeFeed {
                    items: vec![item],
                    playlists: Vec::new(),
                    channels: vec![channel],
                }),
            ),
        ))
        .unwrap();
    app.poll_home(&ctx);

    assert!(app.feed.home.subscription_channels_loaded);
    assert_eq!(app.feed.home.subscription_channels.len(), 1);
    assert_eq!(
        app.feed.home.subscription_channels[0].title,
        "English Channel"
    );
    assert!(app.feed.home.subscriptions_cached_loaded);
    assert_eq!(app.feed.home.subscriptions_cached_items.len(), 1);

    let (err_req_id, _cancel, err_tx) = app.feed.home.stream_task.begin_manual();
    err_tx
        .send((
            err_req_id,
            HomeFetchMessage::Finished(None, Err("network error".into())),
        ))
        .unwrap();
    app.poll_home(&ctx);
    assert_eq!(
        app.feed.home.subscription_channels_error.as_deref(),
        Some("network error")
    );
}

#[test]
fn channel_without_videos_hides_videos_tab_auto_selects_playlists_and_renders_private_playlists() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.settings.prefs.value.language = i18n::UiLanguage::PtBr;

    let my_id = "UCnathanbzrr123456789012";
    let my_target = search::ChannelTarget::ChannelId(my_id.into());
    app.auth.authenticated = true;
    app.auth.my_channel = Some(my_target.clone());
    app.auth.my_channel_title = Some("nathan".into());

    let (req_id, valid_target) = app
        .begin_channel_request(my_target, &ctx)
        .expect("should initiate channel request");

    let musica = search::ChannelPlaylist {
        id: "PLmusica1234567890".into(),
        title: "musica".into(),
        url: "https://www.youtube.com/playlist?list=PLmusica1234567890".into(),
        thumbnail: "https://i.ytimg.com/vi/abc12345678/hqdefault.jpg".into(),
        item_count: Some(434),
        description: None,
        privacy: playlist_sync::PlaylistPrivacy::Public,
    };
    let pc = search::ChannelPlaylist {
        id: "PLpcbuild123456789".into(),
        title: "PC".into(),
        url: "https://www.youtube.com/playlist?list=PLpcbuild123456789".into(),
        thumbnail: "https://i.ytimg.com/vi/xyz12345678/hqdefault.jpg".into(),
        item_count: Some(4),
        description: None,
        privacy: playlist_sync::PlaylistPrivacy::Private,
    };

    let page = ChannelPage {
        target: valid_target.clone(),
        channel_id: Some(my_id.into()),
        title: Some("nathan".into()),
        handle: Some("@nathanbzrr".into()),
        canonical_url: valid_target.canonical_url(),
        subscriber_count: Some(3),
        subscriber_count_text: Some("3 inscritos".into()),
        has_home_tab: false,
        has_videos_tab: false,
        has_playlists_tab: true,
        videos: Vec::new(),
        initial_playlists: vec![musica, pc],
        tab_endpoints: vec![
            innertube::ChannelTabEndpoint {
                title: "Playlists".into(),
                browse_id: my_id.into(),
                params: Some("EglwbGF5bGlzdHM".into()),
                selected: true,
                kind: innertube::ChannelTabKind::Playlists,
            },
            innertube::ChannelTabEndpoint {
                title: "Posts".into(),
                browse_id: my_id.into(),
                params: Some("EgVwb3N0cw".into()),
                selected: false,
                kind: innertube::ChannelTabKind::Posts,
            },
        ],
        ..Default::default()
    };

    assert!(app.complete_channel_request(req_id, &valid_target, Ok(page), &ctx));
    assert_eq!(app.channels.tab, ChannelTab::Playlists);
    assert_eq!(app.channels.playlists.items.len(), 2);

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.channel_section_ui(
                    ui,
                    &c,
                    "test_no_videos_channel",
                    false,
                    Some(BROWSE_MAX_CONTENT_WIDTH),
                );
            });
        },
    );
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();

    assert!(
        !rendered.contains("Vídeos"),
        "Videos tab must be hidden when channel has no videos, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("Início"),
        "Home tab must be hidden when channel has no Home tab/videos, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("1 vídeo"),
        "header must not show bogus 1 vídeo count from playlists, got:\n{rendered}"
    );
    assert!(
        rendered.contains("Playlists") && rendered.contains("Posts"),
        "expected Playlists and Posts tabs, got:\n{rendered}"
    );
    assert!(
        rendered.contains("musica") && rendered.contains("PC"),
        "expected both public and private playlists to render, got:\n{rendered}"
    );
    assert!(
        rendered.contains("Particular"),
        "expected private playlist privacy label, got:\n{rendered}"
    );
}

#[test]
fn channel_with_videos_keeps_videos_tab_and_recovers_from_extra_tab_selection() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.settings.prefs.value.language = i18n::UiLanguage::PtBr;

    let cid = "UCcreator123456789012345";
    let target = search::ChannelTarget::ChannelId(cid.into());
    let (req_id, valid_target) = app
        .begin_channel_request(target, &ctx)
        .expect("should initiate channel request");

    let video = SearchResult {
        video: Video {
            title: "Uploaded Video".into(),
            channel: "Creator".into(),
            url: "https://www.youtube.com/watch?v=vid00000001".into(),
            thumbnail: String::new(),
            duration: "5:00".into(),
        },
        metadata: VideoMetadata::default(),
    };
    let page = ChannelPage {
        target: valid_target.clone(),
        channel_id: Some(cid.into()),
        title: Some("Creator".into()),
        canonical_url: valid_target.canonical_url(),
        has_home_tab: true,
        has_videos_tab: true,
        has_playlists_tab: true,
        videos: vec![video],
        tab_endpoints: vec![
            innertube::ChannelTabEndpoint {
                title: "Início".into(),
                browse_id: cid.into(),
                params: None,
                selected: true,
                kind: innertube::ChannelTabKind::Home,
            },
            innertube::ChannelTabEndpoint {
                title: "Vídeos".into(),
                browse_id: cid.into(),
                params: Some("EgZ2aWRlb3M".into()),
                selected: false,
                kind: innertube::ChannelTabKind::Videos,
            },
            innertube::ChannelTabEndpoint {
                title: "Playlists".into(),
                browse_id: cid.into(),
                params: Some("EglwbGF5bGlzdHM".into()),
                selected: false,
                kind: innertube::ChannelTabKind::Playlists,
            },
            innertube::ChannelTabEndpoint {
                title: "Posts".into(),
                browse_id: cid.into(),
                params: Some("EgVwb3N0cw".into()),
                selected: false,
                kind: innertube::ChannelTabKind::Posts,
            },
        ],
        ..Default::default()
    };

    assert!(app.complete_channel_request(req_id, &valid_target, Ok(page), &ctx));
    assert_eq!(app.channels.tab, ChannelTab::Home);

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.channel_section_ui(
                    ui,
                    &c,
                    "test_videos_channel",
                    false,
                    Some(BROWSE_MAX_CONTENT_WIDTH),
                );
            });
        },
    );
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();
    assert!(
        rendered.contains("Início")
            && rendered.contains("Vídeos")
            && rendered.contains("Playlists"),
        "expected Home, Videos, and Playlists tabs when channel has videos, got:\n{rendered}"
    );

    // Selecting Posts marks the extra endpoint selected; starting channel search must clear it.
    if let Some(p) = app.channels.page.as_mut() {
        p.tab_endpoints[3].selected = true;
    }
    assert!(app.start_channel_search("query", &ctx));
    assert_eq!(app.channels.tab, ChannelTab::Search);
    assert!(
        app.channels
            .page
            .as_ref()
            .unwrap()
            .tab_endpoints
            .iter()
            .all(|ep| !ep.selected)
    );
}
