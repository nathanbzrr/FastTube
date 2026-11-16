use super::*;

#[test]
fn old_session_without_comment_scope_is_discarded_on_restore() {
    let (session, discard, message) = restore_session_state(Ok(Some(
        oauth::TokenSession::test_session_without_comment_scope("old"),
    )));
    assert!(session.is_none());
    assert!(discard);
    assert!(message.unwrap().contains("Entre novamente"));

    let (session, discard, message) =
        restore_session_state(Ok(Some(oauth::TokenSession::test_session("new"))));
    assert_eq!(session.unwrap().access_token, "new");
    assert!(!discard);
    assert!(message.is_none());

    let (session, discard, message) = restore_session_state(Ok(None));
    assert!(session.is_none());
    assert!(!discard);
    assert!(message.is_none());

    let (session, discard, message) = restore_session_state(Err("falha".into()));
    assert!(session.is_none());
    assert!(!discard);
    assert_eq!(message.as_deref(), Some("falha"));
}

#[test]
fn cancel_login_discards_pending_result_and_enables_retry() {
    let mut app = FastTube::default();
    let cancel = process::Cancellation::default();
    let (sender, receiver) = mpsc::channel();
    app.feed.home.cancel = Some(cancel.clone());
    app.feed.home.rx = Some(receiver);
    app.feed.home.title = Some("Inscrições".into());
    app.cancel_login();
    assert!(cancel.cancelled());
    assert!(app.feed.home.rx.is_none());
    assert!(app.feed.home.title.is_none());
    assert!(!app.auth.authenticated);
    assert!(app.auth.session.is_none());
    assert!(app.message.contains("tentar novamente"));
    assert!(
        sender
            .send(HomeFetchMessage::Finished(
                Some(oauth::TokenSession::test_session("resultado-antigo")),
                Err("antigo".into()),
            ))
            .is_err()
    );
    app.poll_async_tasks(&egui::Context::default());
    assert!(!app.auth.authenticated);
    assert!(app.auth.session.is_none());
}

#[test]
fn oauth_home_preserves_session_when_feed_fails() {
    let mut app = FastTube::default();
    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);
    assert!(
        sender
            .send(HomeFetchMessage::Finished(
                Some(oauth::TokenSession::test_session("test-token")),
                Err("API unavailable".into()),
            ))
            .is_ok()
    );
    app.poll_async_tasks(&egui::Context::default());
    assert!(app.auth.authenticated);
    assert_eq!(
        app.auth
            .session
            .as_ref()
            .map(|session| session.access_token.as_str()),
        Some("test-token")
    );
    assert_eq!(app.message, "API unavailable");

    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);
    assert!(
        sender
            .send(HomeFetchMessage::Finished(
                None,
                Err("Login cancelado".into())
            ))
            .is_ok()
    );
    app.poll_async_tasks(&egui::Context::default());
    assert!(!app.auth.authenticated);
    assert!(app.auth.session.is_none());
}

#[test]
fn oauth_home_shows_partial_results_and_preserves_session_on_failure() {
    let mut app = FastTube::default();
    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);
    sender
        .send(HomeFetchMessage::Progress(oauth::SubscriptionProgress {
            items: vec![SearchResult {
                video: Video {
                    title: "Envio recebido".into(),
                    channel: "Canal".into(),
                    url: "https://www.youtube.com/watch?v=vid00000001".into(),
                    thumbnail: String::new(),
                    duration: String::new(),
                },
                metadata: VideoMetadata::default(),
            }],
            channels: Vec::new(),
            completed_channels: 5,
            total_channels: 80,
            reached_limit: false,
        }))
        .unwrap();
    sender
        .send(HomeFetchMessage::Progress(oauth::SubscriptionProgress {
            items: vec![SearchResult {
                video: Video {
                    title: "Lote mais recente".into(),
                    channel: "Canal".into(),
                    url: "https://www.youtube.com/watch?v=vid00000002".into(),
                    thumbnail: String::new(),
                    duration: String::new(),
                },
                metadata: VideoMetadata::default(),
            }],
            channels: Vec::new(),
            completed_channels: 10,
            total_channels: 80,
            reached_limit: false,
        }))
        .unwrap();
    sender
        .send(HomeFetchMessage::Finished(
            Some(oauth::TokenSession::test_session("test-token")),
            Err("API unavailable".into()),
        ))
        .unwrap();

    app.poll_async_tasks(&egui::Context::default());
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Lote mais recente");
    assert_eq!(app.message, "API unavailable");
    assert_eq!(
        app.auth
            .session
            .as_ref()
            .map(|session| session.access_token.as_str()),
        Some("test-token")
    );
}

#[test]
fn oauth_home_progress_waits_to_replace_videos_until_cancelled() {
    let mut app = FastTube::default();
    app.feed.videos = vec![Video {
        title: "Resultado atual".into(),
        channel: "Canal atual".into(),
        url: "https://www.youtube.com/watch?v=current00001".into(),
        thumbnail: String::new(),
        duration: String::new(),
    }];
    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);

    let first_progress = SearchResult {
        video: Video {
            title: "Primeiro lote".into(),
            channel: "Canal".into(),
            url: "https://www.youtube.com/watch?v=progress0001".into(),
            thumbnail: String::new(),
            duration: String::new(),
        },
        metadata: VideoMetadata::default(),
    };
    sender
        .send(HomeFetchMessage::Progress(oauth::SubscriptionProgress {
            items: vec![first_progress],
            channels: Vec::new(),
            completed_channels: 5,
            total_channels: 80,
            reached_limit: false,
        }))
        .unwrap();
    app.poll_async_tasks(&egui::Context::default());

    assert!(app.message.contains("5 de 80"));
    assert_eq!(app.feed.home.pending_items.len(), 1);
    assert_eq!(app.feed.home.pending_items[0].video.title, "Primeiro lote");
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Resultado atual");

    sender
        .send(HomeFetchMessage::Progress(oauth::SubscriptionProgress {
            items: vec![SearchResult {
                video: Video {
                    title: "Segundo lote".into(),
                    channel: "Canal".into(),
                    url: "https://www.youtube.com/watch?v=progress0002".into(),
                    thumbnail: String::new(),
                    duration: String::new(),
                },
                metadata: VideoMetadata::default(),
            }],
            channels: Vec::new(),
            completed_channels: 10,
            total_channels: 80,
            reached_limit: false,
        }))
        .unwrap();
    app.poll_async_tasks(&egui::Context::default());

    assert!(app.message.contains("10 de 80"));
    assert_eq!(app.feed.home.pending_items.len(), 1);
    assert_eq!(app.feed.home.pending_items[0].video.title, "Segundo lote");
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Resultado atual");

    app.cancel_home_feed(&egui::Context::default());

    assert!(app.feed.home.pending_items.is_empty());
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Segundo lote");
}

#[test]
fn oauth_home_keeps_channel_limit_notice_after_final_result() {
    let mut app = FastTube::default();
    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);
    sender
        .send(HomeFetchMessage::Progress(oauth::SubscriptionProgress {
            items: Vec::new(),
            channels: Vec::new(),
            completed_channels: 100,
            total_channels: 100,
            reached_limit: true,
        }))
        .unwrap();
    sender
        .send(HomeFetchMessage::Finished(
            Some(oauth::TokenSession::test_session("test-token")),
            Ok(search::HomeFeed {
                items: Vec::new(),
                playlists: Vec::new(),
                channels: Vec::new(),
            }),
        ))
        .unwrap();

    app.poll_async_tasks(&egui::Context::default());
    assert!(app.message.contains("Limite de 100 canais"));
}

#[test]
fn sidebar_subscription_catalog_only_contains_oauth_subscriptions() {
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.feed.home.title = Some("Inscri\u{00e7}\u{00f5}es: envios recentes".into());
    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);
    sender
        .send(HomeFetchMessage::Progress(oauth::SubscriptionProgress {
            items: Vec::new(),
            channels: vec![search::HomeChannel {
                title: "Canal inscrito".into(),
                channel_id: "UC000000001".into(),
                avatar_url: None,
            }],
            completed_channels: 0,
            total_channels: 1,
            reached_limit: false,
        }))
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

fn sample_videos() -> Vec<Video> {
    vec![
        Video {
            title: "Primeiro Vídeo de Teste".into(),
            channel: "Canal Um".into(),
            url: "https://www.youtube.com/watch?v=vid1".into(),
            thumbnail: String::new(),
            duration: "10:15".into(),
        },
        Video {
            title: "Segundo Vídeo Existente na Pesquisa".into(),
            channel: "Canal Dois".into(),
            url: "https://www.youtube.com/watch?v=vid2".into(),
            thumbnail: String::new(),
            duration: "4:30".into(),
        },
    ]
}

#[test]
fn watch_columns_switch_between_side_by_side_and_stacked_by_width() {
    let wide = compute_watch_columns(940.0);
    assert_eq!(wide.mode, WatchLayoutMode::SideBySide);
    assert!(wide.primary_width >= 460.0);
    assert!(wide.secondary_width >= 320.0);
    assert!((wide.primary_width + wide.secondary_width + wide.gap - 940.0).abs() < 0.01);

    let boundary_stacked = compute_watch_columns(919.9);
    assert_eq!(boundary_stacked.mode, WatchLayoutMode::Stacked);

    let boundary_side_by_side = compute_watch_columns(920.0);
    assert_eq!(boundary_side_by_side.mode, WatchLayoutMode::SideBySide);

    let narrow = compute_watch_columns(720.0);
    assert_eq!(narrow.mode, WatchLayoutMode::Stacked);
    assert_eq!(narrow.primary_width, 720.0);
    assert_eq!(narrow.secondary_width, 720.0);
    assert_eq!(narrow.gap, 0.0);
}

#[test]
fn watch_controls_switch_to_compact_at_narrow_player_widths() {
    assert!(!watch_controls_are_compact(760.0));
    assert!(watch_controls_are_compact(759.9));
    assert!(!watch_controls_are_compact(1100.0));
}

#[test]
fn player_size_preserves_16_by_9_and_leaves_vertical_room() {
    let side = compute_player_size(640.0, 680.0, false, WatchLayoutMode::SideBySide);
    assert!((side.x / side.y - 16.0 / 9.0).abs() < 0.01);
    assert!(side.x <= 640.0);
    assert!(side.y <= 680.0 - 185.0 + 0.01);

    let stacked = compute_player_size(640.0, 500.0, false, WatchLayoutMode::Stacked);
    assert!((stacked.x / stacked.y - 16.0 / 9.0).abs() < 0.01);
    assert!(stacked.y <= 500.0 * 0.72 + 0.01);

    let full = compute_player_size(1920.0, 1080.0, true, WatchLayoutMode::SideBySide);
    assert!((full.x / full.y - 16.0 / 9.0).abs() < 0.01);
    assert!(full.y <= 1080.0 - 110.0 + 0.01);
}

#[test]
fn short_player_uses_portrait_ratio_and_respects_available_width() {
    let short = compute_player_size(640.0, 800.0, false, WatchLayoutMode::Short);
    assert!((short.x / short.y - 9.0 / 16.0).abs() < 0.01);
    assert!(short.x <= 640.0);
    assert!(short.y <= 720.0 + 0.01);

    let narrow = compute_player_size(220.0, 800.0, false, WatchLayoutMode::Short);
    assert!((narrow.x / narrow.y - 9.0 / 16.0).abs() < 0.01);
    assert!(narrow.x <= 220.0);

    let theater =
        compute_player_size_with_theater(640.0, 800.0, false, true, WatchLayoutMode::Short);
    assert!((theater.x / theater.y - 16.0 / 9.0).abs() < 0.01);
}

#[test]
fn header_and_navigation_dimensions_adapt_to_viewport_width() {
    let (lead_wide, input_wide) = compute_header_search_layout(1000.0, 200.0);
    let (lead_narrow, input_narrow) = compute_header_search_layout(420.0, 0.0);
    assert!(input_wide >= input_narrow);
    assert!(lead_wide >= lead_narrow);
    assert!(input_narrow >= 80.0);

    assert_eq!(compute_navigation_width(640.0), 132.0);
    assert_eq!(compute_navigation_width(1100.0), 160.0);

    assert_eq!(compute_header_layout_mode(480.0), HeaderLayoutMode::TwoRows);
    assert_eq!(compute_header_layout_mode(759.9), HeaderLayoutMode::TwoRows);
    assert_eq!(
        compute_header_layout_mode(760.0),
        HeaderLayoutMode::SingleRow
    );
    assert_eq!(compute_header_height(640.0, false), 96.0);
    assert_eq!(compute_header_height(1100.0, false), 56.0);
    assert_eq!(compute_header_height(480.0, true), 0.0);

    assert!(!should_show_compact_navigation(480.0, false, false, false));
    assert!(!should_show_compact_navigation(759.9, false, false, false));
    assert!(should_show_compact_navigation(760.0, false, false, false));
    assert!(should_show_compact_navigation(1100.0, false, false, false));
    assert!(!should_show_compact_navigation(1100.0, true, false, false));
    assert!(!should_show_compact_navigation(1100.0, false, true, false));

    assert_eq!(compute_home_grid_columns(480.0), 1);
    assert_eq!(compute_home_grid_columns(640.0), 1);
    assert_eq!(compute_home_grid_columns(1028.0), 3);
    assert_eq!(compute_continue_watching_columns(640.0), 1);
}

#[test]
fn format_time_handles_minutes_and_hours() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(65.9), "1:05");
    assert_eq!(format_time(3661.0), "1:01:01");
}

#[test]
fn buffer_settings_persistence_pending_restart_and_storage_errors_work() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-settings-test-{}",
        std::process::id()
    ));
    let path = temp_dir.join("buffer-settings.txt");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let mut app = FastTube::default();
    app.settings.buffer.path = Some(path.clone());
    app.settings.buffer.error = None;
    app.settings.buffer.profile = BufferProfile::Default;
    assert_eq!(app.settings.buffer.profile, BufferProfile::Default);
    assert!(!app.has_pending_buffer_restart());

    app.playback.active_buffer = Some(BufferProfile::Default);
    app.apply_buffer_profile(BufferProfile::Large);
    assert_eq!(app.settings.buffer.profile, BufferProfile::Large);
    assert!(app.has_pending_buffer_restart());
    assert!(app.settings.buffer.error.is_none());
    assert!(
        app.settings
            .buffer
            .notice
            .as_deref()
            .unwrap_or("")
            .contains("reiniciar o player")
    );
    assert_eq!(settings::load(&path).unwrap(), BufferProfile::Large);

    app.settings.buffer.path = Some(PathBuf::from(""));
    app.apply_buffer_profile(BufferProfile::Small);
    assert_eq!(app.settings.buffer.profile, BufferProfile::Small);
    assert!(app.settings.buffer.error.is_some());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn playback_preferences_isolated_path_injection_persistence_and_real_path_protection_work() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-playback-settings-test-{}",
        std::process::id()
    ));
    let playback_file = temp_dir.join("playback-settings.txt");
    let buffer_file = temp_dir.join("buffer-settings.txt");
    let history_file = temp_dir.join("history.json");
    let _ = std::fs::remove_dir_all(&temp_dir);

    // Unconfigured path updates session and surfaces an error without writing to real playback_path().
    let mut unconfigured_app = FastTube::default();
    assert!(unconfigured_app.settings.playback.path.is_none());
    assert_eq!(
        unconfigured_app.settings.playback.value,
        PlaybackPreferences::default()
    );
    let custom_prefs = PlaybackPreferences {
        speed_quarters: 6,
        max_height: Some(720),
    };
    unconfigured_app.apply_playback_preferences(custom_prefs);
    assert_eq!(unconfigured_app.settings.playback.value, custom_prefs);
    assert!(unconfigured_app.settings.playback.error.is_some());
    assert!(unconfigured_app.settings.playback.path.is_none());

    let mut app = FastTube::default();
    app.settings.playback.path = Some(playback_file.clone());
    app.settings.buffer.path = Some(buffer_file.clone());
    app.history.path = Some(history_file.clone());
    app.history.loaded_path = Some(history_file.clone());
    app.apply_playback_preferences(custom_prefs);
    assert_eq!(app.settings.playback.value, custom_prefs);
    assert!(app.settings.playback.error.is_none());
    assert_eq!(
        settings::load_playback(&playback_file).unwrap(),
        custom_prefs
    );
    assert!(!buffer_file.exists());
    assert!(!history_file.exists());

    let mut reloaded_app = FastTube::default();
    reloaded_app.settings.playback.path = Some(playback_file.clone());
    reloaded_app.ensure_playback_settings_synchronized();
    assert_eq!(reloaded_app.settings.playback.value, custom_prefs);
    assert!(reloaded_app.settings.playback.error.is_none());

    let corrupt_file = temp_dir.join("corrupt-playback.txt");
    std::fs::write(&corrupt_file, b"invalid-playback-data").unwrap();
    let mut corrupt_app = FastTube::default();
    corrupt_app.settings.playback.path = Some(corrupt_file.clone());
    corrupt_app.ui.settings_open = true;
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
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
            corrupt_app.header_ui(ui, &ctx);
            corrupt_app.navigation_ui(ui, &ctx);
            let _ = corrupt_app.browse_ui(ui, &ctx);
        });
        corrupt_app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();
    assert_eq!(
        corrupt_app.settings.playback.value,
        PlaybackPreferences::default()
    );
    assert!(corrupt_app.settings.playback.error.is_some());
    assert_eq!(
        std::fs::read(&corrupt_file).unwrap(),
        b"invalid-playback-data"
    );

    corrupt_app.settings.playback.path = Some(PathBuf::from(""));
    corrupt_app.settings.playback.loaded_path = Some(PathBuf::from(""));
    let next_prefs = PlaybackPreferences {
        speed_quarters: 8,
        max_height: Some(1440),
    };
    corrupt_app.apply_playback_preferences(next_prefs);
    assert_eq!(corrupt_app.settings.playback.value, next_prefs);
    assert!(corrupt_app.settings.playback.error.is_some());

    let _ = std::fs::remove_dir_all(&temp_dir);
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
fn settings_window_hides_and_restores_video_window_across_watch_fullscreen_and_close() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let videos = sample_videos();
    let mut app = FastTube::default();
    app.feed.videos = videos.clone();
    app.selected = Some(videos[0].clone());
    app.playback.active_buffer = Some(BufferProfile::Default);
    app.ui.settings_open = false;
    app.sync_player_window_visibility();
    assert!(app.playback.window_visible);

    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 760.0),
        )),
        ..Default::default()
    };

    app.ui.settings_open = true;
    let mut output = ctx.run_ui(raw_input.clone(), |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            app.watch_primary_ui(ui, &ctx, 700.0, 640.0, WatchLayoutMode::SideBySide);
        });
        app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();
    assert!(app.ui.settings_open);
    assert!(!app.playback.window_visible);
    assert!(app.selected.is_some());
    assert_eq!(app.playback.active_buffer, Some(BufferProfile::Default));

    app.toggle_fullscreen(&ctx);
    assert!(app.playback.fullscreen);
    assert!(!app.playback.window_visible);
    let mut output = ctx.run_ui(raw_input.clone(), |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            app.watch_primary_ui(ui, &ctx, 1100.0, 760.0, WatchLayoutMode::SideBySide);
        });
        app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();
    assert!(app.ui.settings_open);
    assert!(!app.playback.window_visible);

    app.ui.settings_open = false;
    let mut output = ctx.run_ui(raw_input, |ui| {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            app.watch_primary_ui(ui, &ctx, 1100.0, 760.0, WatchLayoutMode::SideBySide);
        });
        app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();
    assert!(!app.ui.settings_open);
    assert!(app.playback.window_visible);
    assert!(app.selected.is_some());

    app.close_video(&ctx);
    assert!(app.selected.is_none());
    assert!(!app.playback.fullscreen);
    assert!(!app.playback.window_visible);
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

fn valid_youtube_videos() -> Vec<Video> {
    vec![
        Video {
            title: "Vídeo YouTube Um".into(),
            channel: "Canal Alfa".into(),
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            thumbnail: String::new(),
            duration: "3:20".into(),
        },
        Video {
            title: "Vídeo YouTube Dois".into(),
            channel: "Canal Beta".into(),
            url: "https://youtu.be/0123456789A".into(),
            thumbnail: String::new(),
            duration: "2:00".into(),
        },
    ]
}

fn render_history_section(
    ctx: &egui::Context,
    app: &mut FastTube,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(720.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "history_queue_test", true, None, None);
        },
    );
    output.textures_delta.clear();
    output
}

fn history_queue_button_center(output: &egui::FullOutput) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return None;
            };
            (text.galley.text() == "+ Fila").then(|| {
                let text_rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                let center = text_rect.center();
                assert!(
                    clipped.clip_rect.contains(center),
                    "O texto do botão + Fila ficou fora da área visível no layout compacto."
                );
                center
            })
        })
        .expect("O botão + Fila deve aparecer no cartão compacto do histórico.")
}

fn click_history_queue_button(ctx: &egui::Context, app: &mut FastTube, position: egui::Pos2) {
    let pointer_move = egui::Event::PointerMoved(position);
    let _ = render_history_section(
        ctx,
        app,
        vec![
            pointer_move,
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    let _ = render_history_section(
        ctx,
        app,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
}

#[test]
fn compact_history_queue_button_enqueues_the_matching_video_data() {
    let videos = valid_youtube_videos();
    let mut app = FastTube::default();
    app.feed.videos = videos.clone();
    app.queue.data = Queue::new(3);
    app.history.data = History::default();
    app.history
        .data
        .record(history::Entry {
            video_id: "dQw4w9WgXcQ".into(),
            title: videos[0].title.clone(),
            channel: videos[0].channel.clone(),
            position_ms: 42_000,
            duration_ms: Some(200_000),
        })
        .unwrap();
    let ctx = egui::Context::default();
    let output = render_history_section(&ctx, &mut app, Vec::new());
    let button_center = history_queue_button_center(&output);
    click_history_queue_button(&ctx, &mut app, button_center);

    assert_eq!(app.queue.data.len(), 1);
    let queued = &app.queue.data.videos()[0];
    assert_eq!(queued.title, videos[0].title);
    assert_eq!(queued.channel, videos[0].channel);
    assert_eq!(queued.url, videos[0].url);
    assert_eq!(queued.thumbnail, videos[0].thumbnail);
    assert_eq!(queued.duration, videos[0].duration);
    assert!(app.queue.error.is_none());
}

#[test]
fn compact_history_queue_button_preserves_full_queue_and_reports_rejected_video() {
    let videos = valid_youtube_videos();
    let mut app = FastTube::default();
    app.feed.videos = videos.clone();
    app.queue.data = Queue::new(1);
    app.history.data = History::default();
    app.enqueue_video(videos[1].clone()).unwrap();
    app.history
        .data
        .record(history::Entry {
            video_id: "dQw4w9WgXcQ".into(),
            title: videos[0].title.clone(),
            channel: videos[0].channel.clone(),
            position_ms: 0,
            duration_ms: None,
        })
        .unwrap();
    let ctx = egui::Context::default();
    let output = render_history_section(&ctx, &mut app, Vec::new());
    let button_center = history_queue_button_center(&output);
    click_history_queue_button(&ctx, &mut app, button_center);

    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.queue.data.videos()[0].url, videos[1].url);
    let rejected = app.queue.last_rejected_video.as_ref().unwrap();
    assert_eq!(rejected.title, videos[0].title);
    assert_eq!(rejected.channel, videos[0].channel);
    assert_eq!(rejected.url, videos[0].url);
    assert!(app.queue.error.is_some());
}

#[test]
fn unconfirmed_playback_and_missing_path_do_not_replace_saved_position() {
    let videos = valid_youtube_videos();
    let now = Instant::now();
    let mut app = FastTube::default();
    app.history.data = History::default();
    app.history.path = None;
    app.history.loaded_path = None;
    app.history.read_failed = true;
    app.record_video_in_history(&videos[0], 197_000, Some(200_000))
        .unwrap();
    app.activate_video_playback(&videos[0], None, Some(62_000), now);
    app.process_player_telemetry(&videos[0], None, Some(62.0), Some(200.0), false, now);
    app.process_player_telemetry(&videos[0], Some(&videos[0].url), None, None, false, now);
    app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(62.1),
        Some(200.0),
        false,
        now,
    );
    app.flush_playback_history_at(now);
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        197_000
    );
    app.close_video(&egui::Context::default());
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        197_000
    );
}

#[test]
fn corrupt_file_discovered_before_save_is_preserved() {
    let dir = std::env::temp_dir().join(format!("fasttube-late-corruption-{}", std::process::id()));
    let path = dir.join("history.json");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, b"corrupt").unwrap();
    let mut app = FastTube::default();
    app.history.data = History::default();
    app.history.path = Some(path.clone());
    app.history.loaded_path = Some(path.clone());
    app.history.read_failed = false;
    app.record_video_in_history(&valid_youtube_videos()[0], 10_000, None)
        .unwrap();
    assert!(!app.save_history_to_disk(Instant::now()));
    assert!(app.history.read_failed);
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn history_tracks_playback_throttles_disk_saves_and_prevents_old_position_leak_on_switch() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-history-switch-test-{}",
        std::process::id()
    ));
    let path = temp_dir.join("history.json");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let videos = valid_youtube_videos();
    let t0 = Instant::now();
    let mut app = FastTube::default();
    app.history.data = History::default();
    app.history.path = Some(path.clone());
    app.history.loaded_path = Some(path.clone());
    app.history.read_failed = false;
    app.history.error = None;

    app.selected = Some(videos[0].clone());
    app.activate_video_playback(&videos[0], None, None, t0);
    app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(0.0),
        Some(200.0),
        false,
        t0,
    );
    assert_eq!(app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms, 0);
    let initial_disk = history::load(&path).unwrap();
    assert_eq!(initial_disk.get("dQw4w9WgXcQ").unwrap().position_ms, 0);

    let t1 = t0 + Duration::from_millis(500);
    let (pos, dur, seek, clear_start) = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(12.4),
        Some(200.0),
        false,
        t1,
    );
    assert!((pos - 12.4).abs() < 0.01);
    assert_eq!(dur, Some(200.0));
    assert!(seek.is_none());
    assert!(!clear_start);
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        12_400
    );
    assert_eq!(
        history::load(&path)
            .unwrap()
            .get("dQw4w9WgXcQ")
            .unwrap()
            .position_ms,
        0
    );

    let t2 = t0 + Duration::from_secs(2);
    let _ = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(28.0),
        Some(200.0),
        false,
        t2,
    );
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        28_000
    );
    assert_eq!(
        history::load(&path)
            .unwrap()
            .get("dQw4w9WgXcQ")
            .unwrap()
            .position_ms,
        0
    );

    let t3 = t0 + Duration::from_secs(6);
    let _ = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(45.0),
        Some(200.0),
        false,
        t3,
    );
    assert_eq!(
        history::load(&path)
            .unwrap()
            .get("dQw4w9WgXcQ")
            .unwrap()
            .position_ms,
        45_000
    );

    let t4 = t3 + Duration::from_secs(1);
    let _ = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(62.0),
        Some(200.0),
        false,
        t4,
    );
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        62_000
    );

    let prev_live_pos = app.current_player_live_pos_ms();
    assert_eq!(prev_live_pos, Some(62_000));
    let t_switch = t4 + Duration::from_millis(200);
    app.flush_playback_history_at(t_switch);
    app.playback.active = None;
    let resume_v2 = app.planned_resume_ms(&videos[1]);
    assert_eq!(resume_v2, None);
    app.activate_video_playback(&videos[1], resume_v2, prev_live_pos, t_switch);
    app.selected = Some(videos[1].clone());

    let disk_after_switch = history::load(&path).unwrap();
    assert_eq!(
        disk_after_switch.get("dQw4w9WgXcQ").unwrap().position_ms,
        62_000
    );
    assert!(disk_after_switch.get("0123456789A").is_none());

    // Ignore stale path/time-pos samples emitted right after `loadfile` for the previous video.
    let _ = app.process_player_telemetry(
        &videos[1],
        Some(&videos[0].url),
        Some(62.1),
        Some(200.0),
        false,
        t_switch + Duration::from_millis(50),
    );
    assert!(app.history.data.get("0123456789A").is_none());

    let _ = app.process_player_telemetry(
        &videos[1],
        None,
        Some(62.2),
        Some(200.0),
        false,
        t_switch + Duration::from_millis(100),
    );
    assert!(app.history.data.get("0123456789A").is_none());

    let _ = app.process_player_telemetry(
        &videos[1],
        Some(&videos[1].url),
        Some(1.5),
        Some(120.0),
        false,
        t_switch + Duration::from_millis(400),
    );
    let _ = app.process_player_telemetry(
        &videos[1],
        Some(&videos[1].url),
        Some(18.0),
        Some(120.0),
        false,
        t_switch + Duration::from_millis(900),
    );
    let ctx = egui::Context::default();
    app.close_video(&ctx);

    let final_disk = history::load(&path).unwrap();
    assert_eq!(final_disk.entries()[0].video_id, "0123456789A");
    assert_eq!(final_disk.entries()[0].position_ms, 18_000);
    assert_eq!(final_disk.entries()[1].video_id, "dQw4w9WgXcQ");
    assert_eq!(final_disk.entries()[1].position_ms, 62_000);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn resume_restores_saved_position_and_avoids_resuming_near_end_or_non_youtube_urls() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-history-resume-test-{}",
        std::process::id()
    ));
    let path = temp_dir.join("history.json");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let videos = valid_youtube_videos();
    let now = Instant::now();
    let mut app = FastTube::default();
    app.history.data = History::default();
    app.history.path = Some(path.clone());
    app.history.loaded_path = Some(path.clone());
    app.history.read_failed = false;
    app.history.error = None;

    let external_video = Video {
        title: "Vídeo Externo".into(),
        channel: "Outro Site".into(),
        url: "https://example.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
        duration: "5:00".into(),
    };
    assert!(
        app.record_video_in_history(&external_video, 30_000, Some(300_000))
            .is_err()
    );
    assert!(app.planned_resume_ms(&external_video).is_none());
    app.selected = Some(external_video.clone());
    app.activate_video_playback(&external_video, None, None, now);
    let _ = app.process_player_telemetry(
        &external_video,
        Some(&external_video.url),
        Some(25.0),
        Some(300.0),
        false,
        now + Duration::from_secs(6),
    );
    assert!(app.history.data.entries().is_empty());

    app.record_video_in_history(&videos[0], 45_000, Some(200_000))
        .unwrap();
    let resume_ms = app.planned_resume_ms(&videos[0]);
    assert_eq!(resume_ms, Some(45_000));
    app.selected = Some(videos[0].clone());
    app.activate_video_playback(&videos[0], resume_ms, None, now);

    // Keep the 45s resume position intact while the player is still loading (`time-pos == None`).
    let (pos_loading, _, seek_loading, _) = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        None,
        None,
        false,
        now + Duration::from_millis(200),
    );
    assert!((pos_loading - 45.0).abs() < 0.01);
    assert!(seek_loading.is_none());
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        45_000
    );

    let (pos_started, _, pending_seek, _) = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(0.2),
        Some(200.0),
        false,
        now + Duration::from_millis(500),
    );
    assert!((pos_started - 45.0).abs() < 0.01);
    assert_eq!(pending_seek, Some(45.0));
    app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(0.4),
        Some(200.0),
        false,
        now + Duration::from_millis(600),
    );
    app.flush_playback_history_at(now + Duration::from_millis(700));
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        45_000
    );
    app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(45.2),
        Some(200.0),
        false,
        now + Duration::from_millis(800),
    );
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        45_200
    );

    app.record_video_in_history(&videos[0], 197_000, Some(200_000))
        .unwrap();
    assert_eq!(app.planned_resume_ms(&videos[0]), None);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn corrupt_history_file_reports_error_and_is_never_overwritten_and_clear_works() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-history-corrupt-test-{}",
        std::process::id()
    ));
    let corrupt_path = temp_dir.join("history.json");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let corrupt_bytes = b"{corrupted-json-history".to_vec();
    std::fs::write(&corrupt_path, &corrupt_bytes).unwrap();

    let videos = valid_youtube_videos();
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);

    let mut app = FastTube::default();
    app.history.path = Some(corrupt_path.clone());
    app.ensure_history_synchronized();
    assert!(app.history.read_failed);
    assert!(app.history.error.is_some());

    let now = Instant::now();
    app.selected = Some(videos[0].clone());
    app.activate_video_playback(&videos[0], None, None, now);
    let _ = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(30.0),
        Some(200.0),
        false,
        now + Duration::from_secs(10),
    );
    app.close_video(&ctx);
    app.clear_history();
    assert_eq!(std::fs::read(&corrupt_path).unwrap(), corrupt_bytes);

    let valid_path = temp_dir.join("valid-history.json");
    let mut valid_app = FastTube::default();
    valid_app.feed.videos = videos.clone();
    valid_app.history.data = History::default();
    valid_app.history.path = Some(valid_path.clone());
    valid_app.history.loaded_path = Some(valid_path.clone());
    valid_app.history.read_failed = false;
    valid_app.history.error = None;
    valid_app
        .record_video_in_history(&videos[0], 42_000, Some(200_000))
        .unwrap();
    valid_app.save_history_to_disk(now);
    assert_eq!(history::load(&valid_path).unwrap().entries().len(), 1);

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
            valid_app.navigation_ui(ui, &ctx);
            let _ = valid_app.browse_ui(ui, &ctx);
        });
    });
    output.textures_delta.clear();

    valid_app.clear_history();
    assert!(valid_app.history.data.entries().is_empty());
    assert!(history::load(&valid_path).unwrap().entries().is_empty());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn decoding_diagnostics_polls_only_when_settings_visible_and_throttles_to_one_second() {
    use diagnostics::{
        EffectiveDecoding, PROP_HEIGHT, PROP_HWDEC, PROP_HWDEC_CURRENT, PROP_VIDEO_CODEC,
        PROP_WIDTH,
    };

    let videos = valid_youtube_videos();
    let t0 = Instant::now();
    let mut app = FastTube::default();
    app.feed.videos = videos.clone();
    app.selected = None;
    app.ui.settings_open = false;

    let mut ffi_calls = 0usize;

    assert!(!app.refresh_decoding_diagnostics_with(t0, false, |_| {
        ffi_calls += 1;
        Some("d3d11va".into())
    }));
    assert_eq!(ffi_calls, 0);
    assert!(app.playback.diagnostics.effective_decoding.is_unknown());

    app.selected = Some(videos[0].clone());
    assert!(!app.refresh_decoding_diagnostics_with(t0, true, |_| {
        ffi_calls += 1;
        Some("d3d11va".into())
    }));
    assert_eq!(ffi_calls, 0);

    app.ui.settings_open = true;
    assert!(!app.refresh_decoding_diagnostics_with(t0, false, |_| {
        ffi_calls += 1;
        Some("d3d11va".into())
    }));
    assert_eq!(ffi_calls, 0);

    let polled = app.refresh_decoding_diagnostics_with(t0, true, |name| {
        ffi_calls += 1;
        match name {
            PROP_HWDEC => Some("auto-safe".into()),
            PROP_HWDEC_CURRENT => Some("d3d11va".into()),
            PROP_VIDEO_CODEC => Some("vp9".into()),
            PROP_WIDTH => Some("1920".into()),
            PROP_HEIGHT => Some("1080".into()),
            _ => None,
        }
    });
    assert!(polled);
    assert_eq!(ffi_calls, 5);
    assert_eq!(
        app.playback.diagnostics.effective_decoding,
        EffectiveDecoding::HardwareActive("d3d11va".into())
    );
    assert_eq!(app.playback.diagnostics.codec.as_deref(), Some("vp9"));
    assert_eq!(
        app.playback.diagnostics.size,
        Some(VideoSize {
            width: 1920,
            height: 1080
        })
    );

    assert!(
        !app.refresh_decoding_diagnostics_with(t0 + Duration::from_millis(250), true, |_| {
            ffi_calls += 1;
            None
        })
    );
    assert!(
        !app.refresh_decoding_diagnostics_with(t0 + Duration::from_millis(999), true, |_| {
            ffi_calls += 1;
            None
        })
    );
    assert_eq!(ffi_calls, 5);

    let polled_after_1s =
        app.refresh_decoding_diagnostics_with(t0 + Duration::from_secs(1), true, |name| {
            ffi_calls += 1;
            match name {
                PROP_HWDEC => Some("auto-safe".into()),
                PROP_HWDEC_CURRENT => Some("no".into()),
                PROP_VIDEO_CODEC => Some("av1".into()),
                PROP_WIDTH => Some("1280".into()),
                PROP_HEIGHT => Some("720".into()),
                _ => None,
            }
        });
    assert!(polled_after_1s);
    assert_eq!(ffi_calls, 10);
    assert_eq!(
        app.playback.diagnostics.effective_decoding,
        EffectiveDecoding::Software
    );
    assert_eq!(app.playback.diagnostics.codec.as_deref(), Some("av1"));

    let ctx = egui::Context::default();
    app.close_video(&ctx);
    assert!(app.selected.is_none());
    assert!(app.playback.diagnostics.effective_decoding.is_unknown());
    assert!(app.playback.diagnostics.codec.is_none());
    assert!(app.playback.diagnostics.size.is_none());
    assert!(app.playback.last_diagnostics_poll_at.is_none());
}

#[test]
fn settings_diagnostics_display_distinguishes_requested_from_effective_and_renders_headlessly() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let videos = valid_youtube_videos();

    let inactive_diag = DecodingDiagnostics::unknown(Some("auto-safe"));
    let inactive_display = format_diagnostics_display(&inactive_diag, false);
    assert!(inactive_display.status_line.contains("inativo"));
    assert!(inactive_display.configured_line.contains("auto-safe"));
    assert!(
        inactive_display
            .configured_line
            .contains("não confirma aceleração ativa")
    );
    assert!(
        inactive_display
            .effective_line
            .contains("desconhecido / inativo")
    );
    assert!(inactive_display.metadata_line.is_none());

    let unconfirmed_diag =
        DecodingDiagnostics::from_raw(Some("auto-safe"), Some("auto-safe"), None, None, None);
    let unconfirmed_display = format_diagnostics_display(&unconfirmed_diag, true);
    assert!(unconfirmed_display.status_line.contains("desconhecida"));
    assert!(unconfirmed_display.effective_line.contains("desconhecido"));
    assert!(unconfirmed_display.metadata_line.is_none());

    let hw_diag = DecodingDiagnostics::from_raw(
        Some("auto-safe"),
        Some("d3d11va"),
        Some("vp9"),
        Some("1920"),
        Some("1080"),
    );
    let hw_display = format_diagnostics_display(&hw_diag, true);
    assert!(
        hw_display
            .status_line
            .contains("decodificação por hardware ativa confirmada (d3d11va)")
    );
    assert_eq!(
        hw_display.metadata_line.as_deref(),
        Some("Codec: vp9 • Tamanho: 1920 × 1080")
    );

    let sw_diag = DecodingDiagnostics::from_raw(
        Some("auto-safe"),
        Some("no"),
        Some("h264"),
        Some("1280"),
        Some("720"),
    );
    let sw_display = format_diagnostics_display(&sw_diag, true);
    assert!(
        sw_display
            .status_line
            .contains("decodificação por software confirmada")
    );
    assert!(sw_display.effective_line.contains("software (no)"));

    let mut app = FastTube::default();
    app.feed.videos = videos.clone();
    app.selected = Some(videos[0].clone());
    app.ui.settings_open = true;
    app.playback.diagnostics = hw_diag;
    app.playback.last_diagnostics_poll_at = Some(Instant::now());
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1024.0, 720.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw_input.clone(), |ui| {
        let ctx = ui.ctx().clone();
        app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();

    app.selected = None;
    let mut output = ctx.run_ui(raw_input, |ui| {
        let ctx = ui.ctx().clone();
        app.settings_window_ui(&ctx);
    });
    output.textures_delta.clear();
    assert!(app.playback.diagnostics.effective_decoding.is_unknown());
}

#[test]
fn theater_mode_geometry_enlarges_video_and_stacks_results_across_widths() {
    let wide_viewport = 1100.0;
    let nav_w = compute_navigation_width(wide_viewport);
    assert!(should_show_navigation_panel(true, false, true, false));
    assert!(!should_show_navigation_panel(true, false, true, true));
    assert!(should_show_navigation_panel(true, false, false, true));
    assert!(!should_show_navigation_panel(true, true, true, false));

    let normal_avail_w = wide_viewport - nav_w;
    let normal_cols = compute_watch_columns_for_mode(normal_avail_w, false);
    assert_eq!(normal_cols.mode, WatchLayoutMode::SideBySide);

    let theater_cols = compute_watch_columns_for_mode(wide_viewport, true);
    assert_eq!(theater_cols.mode, WatchLayoutMode::Stacked);
    assert_eq!(theater_cols.primary_width, wide_viewport);
    assert_eq!(theater_cols.secondary_width, wide_viewport);
    assert_eq!(theater_cols.gap, 0.0);

    let avail_h = 700.0;
    let normal_size = compute_player_size_with_theater(
        normal_cols.primary_width,
        avail_h,
        false,
        false,
        normal_cols.mode,
    );
    let theater_size = compute_player_size_with_theater(
        theater_cols.primary_width,
        avail_h,
        false,
        true,
        theater_cols.mode,
    );
    let fullscreen_size = compute_player_size_with_theater(
        wide_viewport,
        avail_h,
        true,
        true,
        WatchLayoutMode::SideBySide,
    );

    assert!((normal_size.x / normal_size.y - 16.0 / 9.0).abs() < 0.01);
    assert!((theater_size.x / theater_size.y - 16.0 / 9.0).abs() < 0.01);
    assert!(theater_size.x > normal_size.x);
    assert!(theater_size.y > normal_size.y);
    assert!(fullscreen_size.y > theater_size.y);

    let narrow_w = 700.0;
    let narrow_h = 540.0;
    let normal_narrow = compute_player_size_with_theater(
        narrow_w,
        narrow_h,
        false,
        false,
        WatchLayoutMode::Stacked,
    );
    let theater_narrow =
        compute_player_size_with_theater(narrow_w, narrow_h, false, true, WatchLayoutMode::Stacked);
    assert!(theater_narrow.x > normal_narrow.x);
    assert!(theater_narrow.y > normal_narrow.y);
}

#[test]
fn theater_mode_toggles_via_button_and_shortcut_respecting_focus_fullscreen_and_state() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let videos = valid_youtube_videos();
    let mut app = FastTube::default();
    app.feed.videos = videos.clone();
    app.selected = Some(videos[0].clone());
    app.playback.active_buffer = Some(BufferProfile::Large);
    app.settings.buffer.profile = BufferProfile::Large;
    app.ui.navigation_open = true;
    app.record_video_in_history(&videos[0], 33_000, Some(200_000))
        .unwrap();
    app.sync_player_window_visibility();
    assert!(app.playback.window_visible);
    assert!(!app.playback.theater);
    assert!(!app.playback.fullscreen);

    app.toggle_theater_mode();
    assert!(app.playback.theater);
    assert!(!app.playback.fullscreen);
    assert!(app.playback.window_visible);
    assert_eq!(app.settings.buffer.profile, BufferProfile::Large);
    assert_eq!(app.playback.active_buffer, Some(BufferProfile::Large));
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        33_000
    );

    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 760.0));
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
            if should_show_navigation_panel(
                app.ui.navigation_open,
                app.playback.fullscreen,
                app.selected.is_some(),
                app.playback.theater,
            ) {
                egui::Panel::left("navigation")
                    .exact_size(compute_navigation_width(1100.0))
                    .show(ui, |ui| {
                        app.navigation_ui(ui, &ctx);
                    });
            }
            egui::CentralPanel::default().show(ui, |ui| {
                let cols =
                    compute_watch_columns_for_mode(ui.available_width(), app.playback.theater);
                assert_eq!(cols.mode, WatchLayoutMode::Stacked);
                assert!((cols.primary_width - ui.available_width()).abs() < 0.01);
                app.watch_primary_ui(
                    ui,
                    &ctx,
                    cols.primary_width,
                    ui.available_height(),
                    cols.mode,
                );
                let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_theater_results");
            });
        },
    );
    output.textures_delta.clear();

    let key_t_event = egui::Event::Key {
        key: egui::Key::T,
        physical_key: Some(egui::Key::T),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(base_rect),
            events: vec![key_t_event.clone()],
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            if app.selected.is_some() && !ctx.egui_wants_keyboard_input() {
                let theater = ctx.input(|input| input.key_pressed(egui::Key::T));
                if theater && !app.playback.fullscreen {
                    app.toggle_theater_mode();
                }
            }
        },
    );
    output.textures_delta.clear();
    assert!(!app.playback.theater);

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(base_rect),
            events: vec![key_t_event.clone()],
            ..Default::default()
        },
        |ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut app.feed.query));
            edit.request_focus();
        },
    );
    output.textures_delta.clear();

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(base_rect),
            events: vec![key_t_event],
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            let _ = ui.add(egui::TextEdit::singleline(&mut app.feed.query));
            assert!(ctx.egui_wants_keyboard_input());
            if app.selected.is_some() && !ctx.egui_wants_keyboard_input() {
                app.toggle_theater_mode();
            }
        },
    );
    output.textures_delta.clear();
    assert!(!app.playback.theater);

    app.toggle_theater_mode();
    assert!(app.playback.theater);
    app.ui.settings_open = true;
    app.sync_player_window_visibility();
    assert!(!app.playback.window_visible);
    app.ui.settings_open = false;
    app.sync_player_window_visibility();
    assert!(app.playback.window_visible);
    assert!(app.playback.theater);
    assert_eq!(
        app.history.data.get("dQw4w9WgXcQ").unwrap().position_ms,
        33_000
    );
}

#[test]
fn queue_integration_preserves_duplicates_rejected_videos_errors_and_reorders_safely() {
    let videos = valid_youtube_videos();
    let extra = Video {
        title: "Vídeo YouTube Três".into(),
        channel: "Canal Gama".into(),
        url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
        thumbnail: String::new(),
        duration: "5:10".into(),
    };
    let mut app = FastTube::default();
    app.feed.videos = vec![videos[0].clone(), videos[1].clone(), extra.clone()];
    app.queue.data = Queue::new(3);
    app.settings.buffer.error = Some("erro-config".into());
    app.history.error = Some("erro-historico".into());
    app.message = "erro-pesquisa".into();

    assert_eq!(app.queue.data.limit(), 3);
    assert!(app.enqueue_video(videos[0].clone()).is_ok());
    assert!(app.enqueue_video(videos[1].clone()).is_ok());
    assert!(app.enqueue_video(videos[0].clone()).is_ok());
    assert_eq!(app.queue.data.len(), 3);
    assert_eq!(app.queue.data.videos()[0].url, videos[0].url);
    assert_eq!(app.queue.data.videos()[1].url, videos[1].url);
    assert_eq!(app.queue.data.videos()[2].url, videos[0].url);

    let rejected = app.enqueue_video(extra.clone()).unwrap_err();
    assert_eq!(rejected.title, extra.title);
    assert_eq!(rejected.url, extra.url);
    assert_eq!(
        app.queue
            .last_rejected_video
            .as_ref()
            .map(|v| v.url.as_str()),
        Some(extra.url.as_str())
    );
    assert!(app.queue.error.is_some());
    assert_eq!(app.queue.data.len(), 3);

    assert!(!app.reorder_queue(0, 3));
    assert!(!app.reorder_queue(3, 0));
    assert!(app.remove_from_queue(3).is_none());
    assert!(app.queue.error.is_some());
    assert_eq!(app.queue.data.len(), 3);

    assert!(app.reorder_queue(0, 2));
    assert!(app.queue.error.is_none());
    assert_eq!(app.queue.data.videos()[0].url, videos[1].url);
    assert_eq!(app.queue.data.videos()[1].url, videos[0].url);
    assert_eq!(app.queue.data.videos()[2].url, videos[0].url);

    let removed = app.remove_from_queue(1).unwrap();
    assert_eq!(removed.url, videos[0].url);
    assert_eq!(app.queue.data.len(), 2);

    assert!(app.enqueue_video(rejected).is_ok());
    assert!(app.queue.error.is_none());
    assert!(app.queue.last_rejected_video.is_none());
    assert_eq!(app.queue.data.len(), 3);
    assert_eq!(app.queue.data.videos()[2].url, extra.url);

    assert_eq!(app.settings.buffer.error.as_deref(), Some("erro-config"));
    assert_eq!(app.history.error.as_deref(), Some("erro-historico"));
    assert_eq!(app.message, "erro-pesquisa");

    let mut zero_app = FastTube::default();
    zero_app.queue.data = Queue::new(0);
    let zero_rejected = zero_app.enqueue_video(videos[0].clone()).unwrap_err();
    assert_eq!(zero_rejected.url, videos[0].url);
    assert!(zero_app.advance_queue_at(Instant::now()).is_none());
    assert!(zero_app.queue.error.is_some());
}

#[test]
fn queue_load_failure_keeps_pending_duplicate_and_persisted_item() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-queue-load-failure-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let path = temp_dir.join("queue.json");
    let video = valid_youtube_videos()[0].clone();
    let mut app = FastTube::default();
    app.queue.data = Queue::new(4);
    app.queue.path = Some(path.clone());
    app.queue.read_failed = false;
    app.enqueue_video(video.clone()).unwrap();
    app.enqueue_video(video.clone()).unwrap();
    app.queue.pending_video = Some(video.clone());

    app.report_pending_queue_load_error();

    assert!(
        app.queue
            .error
            .as_deref()
            .unwrap()
            .contains("permanece na fila")
    );
    assert_eq!(app.queue.data.len(), 2);
    assert_eq!(Queue::load(&path, 4).unwrap().len(), 2);
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn delayed_signals_for_equal_urls_never_remove_queue_items() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-queue-load-success-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let path = temp_dir.join("queue.json");
    let videos = valid_youtube_videos();
    let mut app = FastTube::default();
    app.queue.data = Queue::new(4);
    app.queue.path = Some(path.clone());
    app.queue.read_failed = false;
    app.enqueue_video(videos[0].clone()).unwrap();
    app.enqueue_video(videos[1].clone()).unwrap();
    app.queue.pending_video = Some(videos[0].clone());
    assert!(app.reorder_queue(0, 1));

    app.report_pending_queue_playback_observed();
    app.report_pending_queue_playback_observed();

    assert_eq!(app.queue.data.len(), 2);
    assert_eq!(Queue::load(&path, 4).unwrap().len(), 2);
    assert_eq!(app.queue.pending_video, Some(videos[0].clone()));
    assert!(
        app.queue
            .notice
            .as_deref()
            .unwrap()
            .contains("aguardando confirma\u{00e7}\u{00e3}o")
    );
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn sequential_attempts_with_equal_urls_remain_unresolved_until_manual_removal() {
    let videos = valid_youtube_videos();
    let mut app = FastTube::default();
    app.queue.data = Queue::new(4);
    app.enqueue_video(videos[0].clone()).unwrap();
    app.enqueue_video(videos[0].clone()).unwrap();
    app.queue.pending_video = Some(videos[0].clone());

    app.report_pending_queue_playback_observed();
    app.queue.pending_video = Some(videos[0].clone());
    app.report_pending_queue_playback_observed();

    assert_eq!(app.queue.data.len(), 2);
    assert!(app.queue.pending_video.is_some());
    assert!(
        app.queue
            .notice
            .as_deref()
            .unwrap()
            .contains("aguardando confirma\u{00e7}\u{00e3}o")
    );
}

#[test]
fn manual_removal_after_reordering_is_explicit_and_cancels_pending_notice() {
    let videos = valid_youtube_videos();
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.queue.data = Queue::new(4);
    app.enqueue_video(videos[0].clone()).unwrap();
    app.enqueue_video(videos[1].clone()).unwrap();
    app.queue.pending_video = Some(videos[0].clone());
    assert!(app.reorder_queue(0, 1));
    app.report_pending_queue_playback_observed();
    assert_eq!(app.queue.data.len(), 2);

    app.remove_from_queue(1).unwrap();
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.queue.data.videos()[0], videos[1]);
    assert!(app.queue.pending_video.is_none());

    app.queue.pending_video = Some(videos[0].clone());
    app.close_video(&ctx);
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.queue.data.videos()[0], videos[1]);
}

#[test]
fn normal_matching_playback_observation_warns_without_consuming_queue_item() {
    let video = valid_youtube_videos()[0].clone();
    let mut app = FastTube::default();
    app.queue.data = Queue::new(2);
    app.enqueue_video(video.clone()).unwrap();
    app.queue.pending_video = Some(video.clone());

    app.report_pending_queue_playback_observed();

    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.queue.data.videos()[0], video);
    assert!(
        app.queue
            .notice
            .as_deref()
            .unwrap()
            .contains("aguardando confirma\u{00e7}\u{00e3}o")
    );
}

#[test]
fn queue_load_pending_or_unconfirmed_eof_cannot_trigger_another_advance() {
    let video = valid_youtube_videos()[0].clone();
    let mut app = FastTube::default();
    app.queue.data = Queue::new(2);
    app.selected = Some(video.clone());
    app.enqueue_video(video.clone()).unwrap();
    app.activate_video_playback(&video, None, None, Instant::now());
    app.playback.active.as_mut().unwrap().has_live_sample = true;
    assert!(app.queue_auto_advance_ready(true));

    app.queue.pending_video = Some(video.clone());
    assert!(!app.queue_auto_advance_ready(true));
    app.queue.pending_video = None;
    app.playback.active.as_mut().unwrap().has_live_sample = false;
    assert!(!app.queue_auto_advance_ready(true));
}

#[test]
fn queue_manual_advance_preserves_history_theater_mode_buffer_and_metadata_without_auto_advance() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-queue-headless-test-{}",
        std::process::id()
    ));
    let path = temp_dir.join("history.json");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let videos = valid_youtube_videos();
    let meta_v2 = VideoMetadata {
        view_count: Some(98_765),
        upload_date: Some("20261106".into()),
        description: Some("Descrição preservada na fila.".into()),
        ..Default::default()
    };

    let t0 = Instant::now();
    let mut app = FastTube::default();
    app.queue.data = Queue::new(4);
    app.history.data = History::default();
    app.history.path = Some(path.clone());
    app.history.loaded_path = Some(path.clone());
    app.history.read_failed = false;
    app.history.error = None;
    app.settings.buffer.profile = BufferProfile::Large;
    app.playback.active_buffer = Some(BufferProfile::Large);
    app.playback.theater = true;

    app.apply_search_results(
        &ctx,
        vec![
            SearchResult {
                video: videos[0].clone(),
                metadata: VideoMetadata {
                    view_count: None,
                    upload_date: None,
                    description: None,
                    ..Default::default()
                },
            },
            SearchResult {
                video: videos[1].clone(),
                metadata: meta_v2.clone(),
            },
        ],
    );

    app.record_video_in_history(&videos[1], 42_000, Some(120_000))
        .unwrap();
    app.save_history_to_disk(t0);

    assert!(app.enqueue_video(videos[1].clone()).is_ok());
    assert!(app.enqueue_video(videos[0].clone()).is_ok());
    assert_eq!(app.queue.data.len(), 2);

    app.apply_search_results(&ctx, Vec::new());
    assert_eq!(
        app.metadata_for(&videos[1].url).and_then(|m| m.view_count),
        Some(98_765)
    );

    app.selected = Some(videos[0].clone());
    app.activate_video_playback(&videos[0], None, None, t0);
    app.sync_player_window_visibility();
    let _ = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(64.0),
        Some(200.0),
        true,
        t0 + Duration::from_secs(6),
    );
    assert_eq!(app.queue.data.len(), 2);
    assert_eq!(
        app.selected.as_ref().map(|v| v.url.as_str()),
        Some(videos[0].url.as_str())
    );

    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 760.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(base_rect),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                app.navigation_ui(ui, &ctx);
                app.watch_primary_ui(ui, &ctx, 1100.0, 700.0, WatchLayoutMode::Stacked);
                let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_queue_watch_scroll");
            });
        },
    );
    output.textures_delta.clear();

    let advanced = app
        .advance_queue_at(t0 + Duration::from_secs(7))
        .expect("should advance to queued video");
    assert_eq!(advanced.url, videos[1].url);
    assert_eq!(app.queue.data.len(), 2);
    app.handle_queue_player_event(PlaybackEvent::StartFile {
        playlist_entry_id: 101,
    });
    app.handle_queue_player_event(PlaybackEvent::FileLoaded);
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(
        app.selected.as_ref().map(|v| v.url.as_str()),
        Some(videos[1].url.as_str())
    );
    assert_eq!(
        app.selected_metadata.as_ref().and_then(|m| m.view_count),
        Some(98_765)
    );
    assert!(app.playback.theater);
    assert_eq!(app.settings.buffer.profile, BufferProfile::Large);
    assert_eq!(app.playback.active_buffer, Some(BufferProfile::Large));
    assert!(app.playback.window_visible);

    let saved_disk = history::load(&path).unwrap();
    assert_eq!(saved_disk.get("dQw4w9WgXcQ").unwrap().position_ms, 200_000);
    assert_eq!(
        app.playback
            .active
            .as_ref()
            .and_then(|a| a.resume_target_ms),
        Some(42_000)
    );
    assert_eq!(
        app.playback
            .active
            .as_ref()
            .and_then(|a| a.stale_guard_pos_ms),
        Some(200_000)
    );

    app.request_queue_advance();
    assert_eq!(app.queue.data.len(), 1);
    app.handle_queue_player_event(PlaybackEvent::StartFile {
        playlist_entry_id: 102,
    });
    app.handle_queue_player_event(PlaybackEvent::FileLoaded);
    assert!(app.queue.data.is_empty());
    assert_eq!(
        app.selected.as_ref().map(|v| v.url.as_str()),
        Some(videos[0].url.as_str())
    );

    app.close_video(&ctx);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(base_rect),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                let _ = app.browse_ui(ui, &ctx);
            });
        },
    );
    output.textures_delta.clear();

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn theme_selection_persists_isolated_and_preserves_buffer_playback_queue_and_history() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);

    // Unconfigured path updates session and surfaces an error without writing to real theme_path().
    let mut unconfigured_app = FastTube::default();
    assert_eq!(unconfigured_app.settings.theme.value, Theme::Dark);
    assert!(unconfigured_app.settings.theme.path.is_none());
    assert!(unconfigured_app.settings.theme.error.is_none());
    unconfigured_app.apply_theme(Theme::Light);
    assert_eq!(unconfigured_app.settings.theme.value, Theme::Light);
    assert!(unconfigured_app.settings.theme.error.is_some());

    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-main-theme-isolated-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let theme_path = temp_dir.join("theme-settings.txt");
    let buffer_path = temp_dir.join("buffer-settings.txt");
    let playback_path = temp_dir.join("playback-settings.txt");
    let history_path = temp_dir.join("history.json");

    settings::save(&buffer_path, BufferProfile::Large).unwrap();
    let initial_prefs = PlaybackPreferences {
        speed_quarters: 5,
        max_height: Some(720),
    };
    settings::save_playback(&playback_path, initial_prefs).unwrap();

    let mut app = FastTube::default();
    app.settings.theme.path = Some(theme_path.clone());
    app.settings.theme.loaded_path = None;
    app.settings.theme.error = None;
    app.settings.buffer.path = Some(buffer_path.clone());
    app.settings.buffer.profile = settings::load(&buffer_path).unwrap();
    app.settings.buffer.error = None;
    app.settings.playback.path = Some(playback_path.clone());
    app.settings.playback.loaded_path = None;
    app.settings.playback.error = None;
    app.history.data = History::default();
    app.history.path = Some(history_path.clone());
    app.history.loaded_path = Some(history_path.clone());
    app.history.read_failed = false;
    app.history.error = None;

    app.ensure_theme_settings_synchronized();
    app.ensure_playback_settings_synchronized();
    app.sync_theme_visuals(&ctx);

    assert_eq!(app.settings.theme.value, Theme::Dark);
    assert!(ctx.global_style().visuals.dark_mode);
    assert!(!theme_path.exists());
    assert_eq!(app.settings.buffer.profile, BufferProfile::Large);
    assert_eq!(app.settings.playback.value, initial_prefs);

    let videos = valid_youtube_videos();
    app.apply_search_results(
        &ctx,
        videos
            .iter()
            .cloned()
            .map(|video| SearchResult {
                video,
                metadata: VideoMetadata {
                    view_count: Some(12_345),
                    upload_date: Some("20261106".into()),
                    description: Some("Trecho de descrição legível.".into()),
                    ..Default::default()
                },
            })
            .collect(),
    );
    assert!(app.enqueue_video(videos[1].clone()).is_ok());
    app.record_video_in_history(&videos[0], 45_000, Some(180_000))
        .unwrap();
    app.save_history_to_disk(Instant::now());
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.history.data.entries().len(), 1);

    app.apply_theme(Theme::Light);
    app.sync_theme_visuals(&ctx);
    assert_eq!(app.settings.theme.value, Theme::Light);
    assert_eq!(app.settings.applied_theme, Some(Theme::Light));
    assert!(!ctx.global_style().visuals.dark_mode);
    assert!(app.settings.theme.error.is_none());
    assert_eq!(std::fs::read_to_string(&theme_path).unwrap(), "1 light\n");
    assert_eq!(settings::load_theme(&theme_path).unwrap(), Theme::Light);

    // Verify buffer, playback preferences, queue, and history remain untouched.
    assert_eq!(app.settings.buffer.profile, BufferProfile::Large);
    assert_eq!(settings::load(&buffer_path).unwrap(), BufferProfile::Large);
    assert_eq!(app.settings.playback.value, initial_prefs);
    assert_eq!(
        settings::load_playback(&playback_path).unwrap(),
        initial_prefs
    );
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.queue.data.videos()[0].url, videos[1].url);
    assert_eq!(app.history.data.entries().len(), 1);
    assert_eq!(
        history::load(&history_path)
            .unwrap()
            .get("dQw4w9WgXcQ")
            .unwrap()
            .position_ms,
        45_000
    );

    // Render browse, watch, and settings panels in both Light and Dark themes.
    app.ui.settings_open = true;
    app.selected = Some(videos[0].clone());
    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1180.0, 780.0));
    for theme in [Theme::Light, Theme::Dark] {
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
                    app.navigation_ui(ui, &ctx);
                    app.settings_panel_ui(ui);
                    app.watch_primary_ui(ui, &ctx, 760.0, 520.0, WatchLayoutMode::SideBySide);
                    let _ = app.watch_sidebar_results_ui(ui, &ctx, "test_theme_watch_scroll");
                    let _ = app.browse_ui(ui, &ctx);
                });
            },
        );
        output.textures_delta.clear();
        assert_eq!(ctx.global_style().visuals.dark_mode, theme == Theme::Dark);
    }

    // Persist Light again and verify reload in a fresh FastTube instance.
    app.apply_theme(Theme::Light);
    app.sync_theme_visuals(&ctx);
    let mut reloaded_app = FastTube::default();
    reloaded_app.settings.theme.path = Some(theme_path.clone());
    reloaded_app.settings.theme.loaded_path = None;
    reloaded_app.ensure_theme_settings_synchronized();
    reloaded_app.sync_theme_visuals(&ctx);
    assert_eq!(reloaded_app.settings.theme.value, Theme::Light);
    assert!(!ctx.global_style().visuals.dark_mode);

    // Verify corrupt theme file falls back to Dark and is preserved on disk.
    std::fs::write(&theme_path, "1 neon\n").unwrap();
    let mut corrupt_app = FastTube::default();
    corrupt_app.settings.theme.path = Some(theme_path.clone());
    corrupt_app.settings.theme.loaded_path = None;
    corrupt_app.ensure_theme_settings_synchronized();
    corrupt_app.sync_theme_visuals(&ctx);
    assert_eq!(corrupt_app.settings.theme.value, Theme::Dark);
    assert!(ctx.global_style().visuals.dark_mode);
    assert!(corrupt_app.settings.theme.error.is_some());
    assert_eq!(std::fs::read_to_string(&theme_path).unwrap(), "1 neon\n");

    let _ = std::fs::remove_dir_all(&temp_dir);
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
    app.feed.search.rx = Some(rx_s1);

    app.feed.query = "segunda busca".into();
    let (search_req_2, q2) = app.begin_search_request().unwrap();
    assert_eq!(q2, "segunda busca");
    assert!(search_req_2 > search_req_1);
    let (tx_s2, rx_s2) = mpsc::channel();
    app.feed.search.rx = Some(rx_s2);

    // Stale search 1 result arrives via complete_search_request -> rejected.
    let _ = tx_s1.send(SearchFetchMessage {
        request_id: search_req_1,
        result: Ok(vec![SearchResult {
            video: videos[0].clone(),
            metadata: VideoMetadata::default(),
        }]),
    });
    assert!(!app.complete_search_request(
        search_req_1,
        Ok(vec![SearchResult {
            video: videos[0].clone(),
            metadata: VideoMetadata::default(),
        }]),
        &ctx,
    ));
    assert!(app.feed.videos.is_empty());
    assert_eq!(app.feed.search.active_request_id, Some(search_req_2));

    // Active search 2 arrives via channel polling -> accepted.
    tx_s2
        .send(SearchFetchMessage {
            request_id: search_req_2,
            result: Ok(vec![SearchResult {
                video: videos[1].clone(),
                metadata: VideoMetadata {
                    channel_id: Some("UC38IQsAvIsxxjztdMZQtwHA".into()),
                    view_count: Some(500),
                    ..Default::default()
                },
            }]),
        })
        .unwrap();
    app.poll_async_tasks(&ctx);
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].url, videos[1].url);
    assert!(app.feed.search.rx.is_none());
    assert!(app.feed.search.active_request_id.is_none());

    // 2. Concurrent channel fetches: start channel A, then channel B before A finishes.
    let target_a = ChannelTarget::ChannelId("UC38IQsAvIsxxjztdMZQtwHA".into());
    let target_b = ChannelTarget::Handle("@RustLang".into());

    let (chan_req_a, valid_a) = app.begin_channel_request(target_a.clone(), &ctx).unwrap();
    let (tx_ca, rx_ca) = mpsc::channel();
    app.channels.videos.rx = Some(rx_ca);

    let (chan_req_b, valid_b) = app.begin_channel_request(target_b.clone(), &ctx).unwrap();
    assert!(chan_req_b > chan_req_a);
    let (tx_cb, rx_cb) = mpsc::channel();
    app.channels.videos.rx = Some(rx_cb);

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
    let _ = tx_ca.send(ChannelFetchMessage {
        request_id: chan_req_a,
        target: valid_a.clone(),
        result: Ok(search::ChannelFetchOutcome {
            page: page_a.clone(),
            from_cache: false,
        }),
    });
    assert!(!app.complete_channel_request(chan_req_a, &valid_a, Ok(page_a), &ctx));
    assert!(app.channels.page.is_none());
    assert!(app.is_channel_loading());

    // Active response B arrives via poll_async_tasks -> applied.
    tx_cb
        .send(ChannelFetchMessage {
            request_id: chan_req_b,
            target: valid_b.clone(),
            result: Ok(search::ChannelFetchOutcome {
                page: page_b,
                from_cache: false,
            }),
        })
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
    let (_chan_req_d, _valid_d) = app.begin_channel_request(target_a, &ctx).unwrap();
    assert!(app.channels.page.is_none()); // Different target clears old channel_page
    let (tx_drop, rx_drop) = mpsc::channel::<ChannelFetchMessage>();
    drop(tx_drop);
    app.channels.videos.rx = Some(rx_drop);
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
    let (tx_channel, rx_channel) = mpsc::channel::<ChannelFetchMessage>();
    app.channels.videos.rx = Some(rx_channel);
    assert!(app.channels.view_open);
    assert!(app.is_channel_loading());

    let (search_request, query) = app
        .begin_search_request()
        .expect("non-empty query should start a search");
    assert_eq!(query, "rust");
    assert!(search_request > 0);
    assert!(!app.channels.view_open);
    assert!(!app.is_channel_loading());
    assert!(app.channels.videos.rx.is_none());
    assert!(app.channels.videos.active_request_id.is_none());

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
    let _ = tx_channel.send(ChannelFetchMessage {
        request_id: channel_request,
        target: valid_target.clone(),
        result: Ok(search::ChannelFetchOutcome {
            page: stale_page.clone(),
            from_cache: false,
        }),
    });
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
    let active_sort_req = app.channels.videos.active_sorted_request_id.unwrap();
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
    let search_req_id = app.channels.search.active_request_id.unwrap();
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
fn comments_reauthorization_replaces_session_and_resets_comment_state() {
    let mut app = FastTube::default();
    app.auth.session = Some(oauth::TokenSession::test_session_without_comment_scope(
        "old-token",
    ));
    app.auth.authenticated = true;
    app.watch.extras.comments = vec![oauth::Comment {
        id: "comment-old".into(),
        author: "Ana".into(),
        author_avatar: None,
        text: "antigo".into(),
        published: String::new(),
        likes: 0,
        replies: 0,
    }];
    app.watch.extras.comments_error = Some("erro antigo".into());
    app.watch.extras.comments_loaded = true;
    let (sender, receiver) = mpsc::channel();
    app.auth.reauth_rx = Some(receiver);
    sender
        .send(Ok(oauth::TokenSession::test_session("novo-token")))
        .unwrap();

    app.poll_async_tasks(&egui::Context::default());

    assert!(app.auth.authenticated);
    assert_eq!(
        app.auth.session.as_ref().unwrap().access_token,
        "novo-token"
    );
    assert!(app.auth.session.as_ref().unwrap().has_comment_scope());
    assert!(app.watch.extras.comments.is_empty());
    assert!(app.watch.extras.comments_error.is_none());
    assert!(!app.watch.extras.comments_loaded);
}

#[test]
fn import_preview_renders_modes_without_applying_data() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.ui.settings_open = true;
    app.backup.confirm_import = true;
    app.backup.preview = Some(BackupPreview {
        path: PathBuf::from("backup.json"),
        library_items: 3,
        favorites: 1,
        watch_later: 1,
        collections: 1,
        bookmarks: 0,
        queue_items: 2,
        history_entries: 4,
        merge_added: 2,
        merge_conflicts: 1,
        merge_skipped: 0,
        history_progress_updates: 1,
    });
    let initial_library = app.library.data.clone();
    let initial_history = app.history.data.clone();
    let initial_queue_len = app.queue.data.len();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1024.0, 800.0),
            )),
            ..Default::default()
        },
        |ui| app.settings_window_ui(ui.ctx()),
    );
    let rendered_shapes = output.shapes.len();
    output.textures_delta.clear();
    assert!(rendered_shapes > 0);
    assert!(app.backup.confirm_import);
    assert!(app.backup.pending_import.is_none());
    assert_eq!(app.backup.import_mode, BackupImportMode::Merge);
    assert_eq!(app.library.data, initial_library);
    assert_eq!(app.history.data, initial_history);
    assert_eq!(app.queue.data.len(), initial_queue_len);
}

#[test]
fn shortcuts_help_renders_and_f1_escape_toggle_state_headlessly() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(800.0, 600.0),
    ));
    let key_input = |key| egui::RawInput {
        screen_rect,
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    };

    let mut output = ctx.run_ui(key_input(egui::Key::F1), |ui| {
        app.handle_help_shortcuts(ui.ctx());
    });
    assert!(app.ui.shortcuts_open);
    output.textures_delta.clear();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect,
            ..Default::default()
        },
        |ui| app.shortcuts_window_ui(ui.ctx()),
    );
    let rendered_shapes = output.shapes.len();
    output.textures_delta.clear();
    assert!(rendered_shapes > 0);

    let mut output = ctx.run_ui(key_input(egui::Key::Escape), |ui| {
        app.handle_help_shortcuts(ui.ctx());
    });
    assert!(!app.ui.shortcuts_open);
    output.textures_delta.clear();
}

#[test]
fn transcript_controls_render_headlessly_without_panicking() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.watch.extras.transcript = vec![transcript::Cue {
        start_ms: 0,
        end_ms: 2_000,
        text: "fala de teste".into(),
    }];
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            app.subtitles_ui(ui, &ctx, "test_transcript");
        },
    );
    let rendered_shapes = output.shapes.len();
    output.textures_delta.clear();
    assert!(rendered_shapes > 0);
}

#[test]
fn encode_utf16_clipboard_words_appends_null_terminator_for_ascii_and_unicode() {
    let ascii = encode_utf16_clipboard_words("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    assert_eq!(ascii.last(), Some(&0));
    assert_eq!(
        String::from_utf16(&ascii[..ascii.len() - 1]).unwrap(),
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
    );

    let unicode = encode_utf16_clipboard_words("Vídeo com ação 👍");
    assert_eq!(unicode.last(), Some(&0));
    assert_eq!(
        String::from_utf16(&unicode[..unicode.len() - 1]).unwrap(),
        "Vídeo com ação 👍"
    );
}

#[test]
fn poll_rating_updates_ratings_refreshed_session_and_status_messages() {
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("old-token"));

    let (tx, rx) = mpsc::channel();
    app.auth.rating_rx = Some(rx);
    tx.send((
        Some(oauth::TokenSession::test_session("refreshed-token")),
        Ok(("dQw4w9WgXcQ".into(), Some(oauth::VideoRating::Like))),
    ))
    .unwrap();
    app.poll_rating();
    assert_eq!(
        app.auth
            .session
            .as_ref()
            .map(|session| session.access_token.as_str()),
        Some("refreshed-token")
    );
    assert_eq!(
        app.auth.ratings.get("dQw4w9WgXcQ").copied(),
        Some(oauth::VideoRating::Like)
    );
    assert_eq!(app.message, "Você gostou deste vídeo.");
    assert!(app.auth.rating_rx.is_none());

    let (tx, rx) = mpsc::channel();
    app.auth.rating_rx = Some(rx);
    tx.send((
        None,
        Ok(("dQw4w9WgXcQ".into(), Some(oauth::VideoRating::Dislike))),
    ))
    .unwrap();
    app.poll_rating();
    assert_eq!(
        app.auth.ratings.get("dQw4w9WgXcQ").copied(),
        Some(oauth::VideoRating::Dislike)
    );
    assert_eq!(app.message, "Você não gostou deste vídeo.");

    let (tx, rx) = mpsc::channel();
    app.auth.rating_rx = Some(rx);
    tx.send((None, Ok(("dQw4w9WgXcQ".into(), None)))).unwrap();
    app.poll_rating();
    assert!(!app.auth.ratings.contains_key("dQw4w9WgXcQ"));
    assert_eq!(app.message, "Avaliação removida.");

    let (tx, rx) = mpsc::channel();
    app.auth.rating_rx = Some(rx);
    tx.send((
        None,
        Err("Sua sessão Google expirou ou foi revogada. Entre novamente.".into()),
    ))
    .unwrap();
    app.poll_rating();
    assert!(app.auth.session.is_some());
    assert!(app.message.contains("sessão Google expirou"));
}

#[test]
fn watch_share_copied_state_renders_headlessly_and_clears_on_close() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let video = sample_videos().remove(0);
    app.selected = Some(video.clone());
    app.watch.share_copied_at = Some(Instant::now());

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 720.0),
            )),
            ..Default::default()
        },
        |ui| {
            app.watch_metadata_card_ui(ui, &video, 680.0, None);
        },
    );
    let rendered_shapes = output.shapes.len();
    output.textures_delta.clear();
    assert!(rendered_shapes > 0);
    assert!(app.watch.share_copied_at.is_some());

    app.close_video(&ctx);
    assert!(app.watch.share_copied_at.is_none());
}

#[test]
fn integrated_overlay_actions_update_menu_theater_fullscreen_miniplayer_and_app_settings() {
    let temp_dir = std::env::temp_dir().join(format!(
        "fasttube-overlay-actions-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let lib_path = temp_dir.join("library.json");

    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.library.path = Some(lib_path.clone());

    let mut video = sample_videos().remove(0);
    video.url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into();
    app.selected = Some(video.clone());
    app.playback.last_subtitle_id = Some("mpv:1".into());

    let overlay_ctx = OverlayPlaybackContext {
        video: video.clone(),
        paused: false,
        ended: false,
        muted: false,
        position: 42.0,
        duration: Some(180.0),
        buffered_secs: None,
        raw_volume: 85.0,
        speed: 1.0,
        current_height: Some(1080),
        selected_quality: None,
        qualities: vec![1080, 720, 480],
        qualities_loading: false,
        has_queued: false,
        has_active_subtitle: false,
        subtitles: Vec::new(),
        subtitles_loading: false,
        chapters: vec![overlay::OverlayChapter {
            title: "Intro".into(),
            time_secs: 0.0,
        }],
        theater: false,
        fullscreen: false,
        mini_player: false,
        autoplay: true,
        loop_video: false,
        is_live: false,
        is_live_dvr: false,
        at_live_edge: false,
        live_behind_secs: 0.0,
        can_bookmark: true,
        raw_path: Some(video.url.clone()),
        raw_position: Some(42.0),
        now: Instant::now(),
    };

    app.handle_player_overlay_action(
        overlay::OverlayAction::SetMenu(Some(overlay::OverlayMenu::Root)),
        &overlay_ctx,
        &ctx,
    );
    assert_eq!(app.playback.player_menu, Some(overlay::OverlayMenu::Root));

    app.handle_player_overlay_action(
        overlay::OverlayAction::SetMenu(Some(overlay::OverlayMenu::Speed)),
        &overlay_ctx,
        &ctx,
    );
    assert_eq!(app.playback.player_menu, Some(overlay::OverlayMenu::Speed));

    app.handle_player_overlay_action(overlay::OverlayAction::SetSpeed(1.5), &overlay_ctx, &ctx);
    assert_eq!(app.playback.player_menu, Some(overlay::OverlayMenu::Speed));

    app.handle_player_overlay_action(
        overlay::OverlayAction::SetMenu(Some(overlay::OverlayMenu::Quality)),
        &overlay_ctx,
        &ctx,
    );
    app.handle_player_overlay_action(
        overlay::OverlayAction::SelectQuality(Some(720)),
        &overlay_ctx,
        &ctx,
    );
    assert_eq!(
        app.playback.player_menu,
        Some(overlay::OverlayMenu::Quality)
    );

    app.handle_player_overlay_action(
        overlay::OverlayAction::SetMenu(Some(overlay::OverlayMenu::Subtitles)),
        &overlay_ctx,
        &ctx,
    );
    app.handle_player_overlay_action(
        overlay::OverlayAction::SelectSubtitle(None),
        &overlay_ctx,
        &ctx,
    );
    assert_eq!(
        app.playback.player_menu,
        Some(overlay::OverlayMenu::Subtitles)
    );

    app.handle_player_overlay_action(
        overlay::OverlayAction::SetMenu(Some(overlay::OverlayMenu::Chapters)),
        &overlay_ctx,
        &ctx,
    );
    app.handle_player_overlay_action(overlay::OverlayAction::SeekChapter(0.0), &overlay_ctx, &ctx);
    assert_eq!(
        app.playback.player_menu,
        Some(overlay::OverlayMenu::Chapters)
    );

    app.handle_player_overlay_action(
        overlay::OverlayAction::SetMenu(Some(overlay::OverlayMenu::Root)),
        &overlay_ctx,
        &ctx,
    );
    app.handle_player_overlay_action(overlay::OverlayAction::AddBookmark, &overlay_ctx, &ctx);
    assert_eq!(app.library.data.bookmarks.len(), 1);
    assert_eq!(app.library.data.bookmarks[0].time_ms, 42_000);
    assert!(app.library.error.is_none());
    assert_eq!(app.playback.player_menu, Some(overlay::OverlayMenu::Root));

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleAutoplay, &overlay_ctx, &ctx);
    assert!(!app.playback.autoplay);
    assert_eq!(app.playback.player_menu, Some(overlay::OverlayMenu::Root));

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleLoop, &overlay_ctx, &ctx);
    assert!(app.playback.loop_video);
    assert_eq!(app.playback.player_menu, Some(overlay::OverlayMenu::Root));

    app.handle_player_overlay_action(overlay::OverlayAction::OpenAppSettings, &overlay_ctx, &ctx);
    assert_eq!(app.playback.player_menu, None);
    assert!(app.ui.settings_open);
    assert!(!app.playback.window_visible);

    app.ui.settings_open = false;
    app.handle_player_overlay_action(overlay::OverlayAction::ToggleTheater, &overlay_ctx, &ctx);
    assert!(app.playback.theater);

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleFullscreen, &overlay_ctx, &ctx);
    assert!(app.playback.fullscreen);

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleMiniPlayer, &overlay_ctx, &ctx);
    assert!(app.playback.mini_player);
    assert!(!app.playback.fullscreen);

    app.playback.player_menu = Some(overlay::OverlayMenu::Quality);
    app.close_video(&ctx);
    assert_eq!(app.playback.player_menu, None);
    assert_eq!(app.playback.last_subtitle_id, None);
    assert!(!app.playback.mini_player);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn overlay_style_maintains_high_contrast_in_both_themes() {
    let mut settings = app_state::SettingsState::default();
    for theme in [Theme::Dark, Theme::Light] {
        settings.theme.value = theme;
        let style = settings.overlay_style();
        assert!(style.panel.a() >= 220);
        assert!(style.icon.r() >= 240 && style.icon.g() >= 240 && style.icon.b() >= 240);
        assert!(style.text.r() >= 240 && style.text.g() >= 240 && style.text.b() >= 240);
        assert_eq!(style.accent, Color32::from_rgb(255, 0, 51));
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
fn collection_toggle_and_saved_status_round_trip() {
    let mut app = FastTube::default();
    let mut video = sample_videos().remove(0);
    video.url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into();

    assert!(!app.video_saved_in_library(&video));
    app.toggle_collection_video("Estudos Rust", &video);
    assert!(app.video_saved_in_library(&video));
    assert_eq!(app.library.data.collections.len(), 1);
    assert_eq!(app.library.data.collections[0].videos.len(), 1);

    app.toggle_collection_video("estudos rust", &video);
    assert!(!app.video_saved_in_library(&video));
    assert!(app.library.data.collections[0].videos.is_empty());
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

fn collect_rendered_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    let mut texts = Vec::new();
    fn visit(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(items) => {
                for item in items {
                    visit(item, out);
                }
            }
            _ => {}
        }
    }
    for clipped in shapes {
        visit(&clipped.shape, &mut texts);
    }
    texts
}

#[test]
fn watch_metadata_card_omits_transcript_and_markers_section() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let video = Video {
        title: "Video de Teste".into(),
        channel: "Canal Teste".into(),
        duration: "3:00".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
    };
    app.selected = Some(video.clone());
    app.watch.extras.comments_loaded = true;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1024.0, 900.0),
            )),
            ..Default::default()
        },
        |ui| {
            app.watch_metadata_card_ui(ui, &video, 820.0, None);
        },
    );
    let rendered = collect_rendered_text(&output.shapes).join("\n");
    output.textures_delta.clear();
    assert!(!rendered.contains("Transcri\u{00e7}\u{00e3}o / Legendas"));
    assert!(!rendered.contains("Ocultar transcri\u{00e7}\u{00e3}o"));
    assert!(!rendered.contains("Marcadores"));
}

#[test]
fn comments_ui_renders_composer_emoji_picker_avatars_and_actions_headlessly() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("token-comments"));
    app.auth.my_channel = Some(ChannelTarget::ChannelId("UC1234567890".into()));
    app.auth.my_channel_title = Some("Meu Canal".into());
    app.auth.my_channel_fetch_attempted = true;
    app.watch.extras.comments_loaded = true;
    app.watch.extras.comments_total = Some(1);
    app.watch.extras.comment_composer_active = true;
    app.watch.extras.comment_emoji_picker_open = true;
    app.watch.extras.comments = vec![oauth::Comment {
        id: "c-1".into(),
        author: "@usuario".into(),
        author_avatar: None,
        text: "Hello world".into(),
        published: "2026-11-13T10:00:00Z".into(),
        likes: 66,
        replies: 1,
    }];
    app.watch
        .extras
        .comment_ratings
        .insert("c-1".into(), watch_extras::CommentRating::Like);
    app.watch
        .extras
        .comment_rating_errors
        .insert("c-1".into(), "Falha remota simulada".into());
    assert_eq!(app.watch.extras.comment_displayed_likes("c-1", 66), 67);

    let input_id = egui::Id::new("test_emoji_insert");
    let mut draft = "Oi".to_owned();
    FastTube::insert_emoji_into_draft(&ctx, input_id, &mut draft, "\u{1f525}");
    assert_eq!(draft, "Oi\u{1f525}");

    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 900.0),
            )),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.comments_ui(ui, &c, "test_watch_comments");
        },
    );
    let rendered = collect_rendered_text(&output.shapes).join("\n");
    output.textures_delta.clear();
    assert!(rendered.contains("1 coment\u{00e1}rio"));
    assert!(rendered.contains("Cancelar"));
    assert!(rendered.contains("Comentar"));
    assert!(rendered.contains("Traduzir para o portugu\u{00ea}s"));
    assert!(rendered.contains("67"));
    assert!(rendered.contains("Falha remota simulada"));
}

#[test]
fn poll_my_channel_stores_avatar_and_only_navigates_when_requested() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("token"));
    app.auth.my_channel_open_when_ready = false;
    let (tx, rx) = mpsc::channel();
    app.auth.my_channel_rx = Some(rx);
    tx.send((
        Some(oauth::TokenSession::test_session("token-2")),
        Ok(oauth::MyChannelProfile {
            target: ChannelTarget::ChannelId("UCabc123".into()),
            title: Some("Canal Teste".into()),
            avatar_url: Some("https://yt3.ggpht.com/avatar=s88".into()),
            subscriber_count: Some(3),
        }),
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
fn youtube_sidebar_and_responsive_watch_resize_adapt_across_viewports() {
    // 1. Browse page: persistent sidebar at >= 1312px, compact rail + overlay drawer at 760..1312px, overlay drawer only at < 760px.
    assert!(should_show_persistent_navigation_panel(
        1440.0, true, false, false, false
    ));
    assert!(!should_show_compact_navigation_for_page(
        1440.0, true, false, false, false
    ));
    assert!(!should_show_navigation_drawer(1440.0, true, false, false));

    assert!(!should_show_persistent_navigation_panel(
        1440.0, false, false, false, false
    ));
    assert!(should_show_compact_navigation_for_page(
        1440.0, false, false, false, false
    ));

    assert!(!should_show_persistent_navigation_panel(
        1100.0, true, false, false, false
    ));
    assert!(should_show_compact_navigation_for_page(
        1100.0, true, false, false, false
    ));
    assert!(should_show_navigation_drawer(1100.0, true, false, false));

    assert!(!should_show_compact_navigation_for_page(
        640.0, false, false, false, false
    ));
    assert!(should_show_navigation_drawer(640.0, true, false, false));

    // 2. Watch page (video open): never shows persistent panel or compact rail; shows overlay drawer when navigation_open (including in theater mode).
    assert!(!should_show_persistent_navigation_panel(
        1600.0, true, false, true, false
    ));
    assert!(!should_show_compact_navigation_for_page(
        1600.0, false, false, true, false
    ));
    assert!(should_show_navigation_drawer(1600.0, true, false, true));
    assert!(!should_show_navigation_drawer(1600.0, false, false, true));

    // Watch pagecolumns adapt between wide side-by-side and narrow stacked on window resize.
    let wide_watch = compute_watch_page_columns(1440.0, 900.0, false);
    assert_eq!(wide_watch.mode, WatchLayoutMode::SideBySide);
    let wide_margin = watch_page_side_margin(1440.0, &wide_watch);
    assert!(wide_margin >= 24.0);

    let narrow_watch = compute_watch_page_columns(820.0, 700.0, false);
    assert_eq!(narrow_watch.mode, WatchLayoutMode::Stacked);
    assert!(narrow_watch.primary_width <= 820.0 - 24.0);

    let theater_watch = compute_watch_page_columns(1440.0, 900.0, true);
    assert_eq!(theater_watch.mode, WatchLayoutMode::Stacked);
    assert_eq!(theater_watch.primary_width, 1440.0);

    // 3. Headless UI pass verifying YouTube sidebar sections and overlay drawer in both default and theater watch modes.
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.feed.videos = sample_videos();
    app.selected = Some(app.feed.videos[0].clone());
    app.ui.navigation_open = true;
    app.playback.theater = true;

    for size in [
        egui::vec2(1600.0, 900.0),
        egui::vec2(1100.0, 760.0),
        egui::vec2(720.0, 640.0),
    ] {
        let mut rendered = String::new();
        for _pass in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| {
                    let c = ui.ctx().clone();
                    egui::Panel::top("header").show(ui, |ui| {
                        app.header_ui(ui, &c);
                    });
                    egui::CentralPanel::default().show(ui, |ui| {
                        let _ = app.watch_page_ui(ui, &c);
                    });
                    app.watch_navigation_drawer_ui(&c);
                },
            );
            rendered = collect_rendered_text(&output.shapes).join("\n");
            output.textures_delta.clear();
        }
        assert!(
            rendered.contains("In\u{00ed}cio"),
            "rendered was:\n{rendered}"
        );
        assert!(rendered.contains("Shorts"));
        assert!(rendered.contains("Inscri\u{00e7}\u{00f5}es"));
        assert!(rendered.contains("Voc\u{00ea}"));
        assert!(rendered.contains("Explorar"));
        assert!(rendered.contains("M\u{00fa}sica"));
        assert!(rendered.contains("Jogos"));
        assert!(rendered.contains("Configura\u{00e7}\u{00f5}es"));
    }
}

#[test]
fn home_feed_cache_restores_without_refetch_on_inicio() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    let (sender, receiver) = mpsc::channel();
    app.feed.home.batch_rx = Some(receiver);
    app.feed.home.batch_active_request_id = Some(1);
    let mix_url = "https://www.youtube.com/playlist?list=RDmix01".to_owned();
    sender
        .send(HomeBatchFetchMessage {
            request_id: 1,
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
        })
        .unwrap();

    app.poll_async_tasks(&ctx);
    assert!(app.feed.home.cached_loaded);
    assert_eq!(app.feed.home.cached_items.len(), 2);
    assert_eq!(app.feed.videos.len(), 2);

    // Switch away to a playlist/subscriptions view, then call start_public_home:
    // it must restore cached Home instantly without spawning a new batch_rx fetch.
    app.feed.home.title = Some("Inscri\u{00e7}\u{00f5}es: envios recentes".into());
    app.feed.videos.clear();
    app.start_public_home(&ctx);

    assert!(app.feed.home.title.is_none());
    assert!(app.feed.home.batch_rx.is_none());
    assert_eq!(app.feed.videos.len(), 2);
    assert!(app.feed.home.playlist_urls.contains(&mix_url));
}

#[test]
fn home_feed_batch_pagination_accumulates_past_100_items() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();

    for page in 0..2usize {
        let request_id = (page + 1) as u64;
        let (sender, receiver) = mpsc::channel();
        app.feed.home.batch_rx = Some(receiver);
        app.feed.home.batch_active_request_id = Some(request_id);
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
            .send(HomeBatchFetchMessage {
                request_id,
                page,
                result: Ok(search::HomeFeed {
                    items,
                    playlists: Vec::new(),
                    channels: Vec::new(),
                }),
            })
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
fn mini_player_viewport_settles_preserves_aspect_and_exits_on_maximize() {
    let ctx = egui::Context::default();
    let mut app = FastTube {
        selected: Some(sample_videos().remove(0)),
        ..Default::default()
    };

    assert_eq!(
        FastTube::compute_mini_player_size(480.0, 270.0),
        egui::vec2(480.0, 270.0)
    );
    let min_size = FastTube::compute_mini_player_size(260.0, 146.0);
    assert!(min_size.x <= 260.0 && min_size.y <= 146.0);
    let tall_size = FastTube::compute_mini_player_size(500.0, 400.0);
    assert!((tall_size.x - 500.0).abs() < 0.01);
    assert!((tall_size.y - 281.25).abs() < 0.01);

    app.playback.fullscreen = true;
    app.toggle_mini_player(&ctx);
    assert!(app.playback.mini_player);
    assert!(!app.playback.fullscreen);
    assert!(app.playback.mini_player_pending_enter.is_some());
    assert!(!app.playback.mini_player_settled);

    // Frame with normal (unmaximized, unfullscreen) viewport settles PiP without exiting.
    let mut raw_normal = egui::RawInput::default();
    if let Some(vp) = raw_normal.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(false);
        vp.inner_rect = Some(egui::Rect::from_min_size(
            egui::pos2(800.0, 400.0),
            egui::vec2(520.0, 292.5),
        ));
        vp.outer_rect = Some(egui::Rect::from_min_size(
            egui::pos2(792.0, 368.0),
            egui::vec2(536.0, 332.5),
        ));
    }
    let mut out1 = ctx.run_ui(raw_normal, |ui| {
        app.sync_mini_player_viewport(ui.ctx());
    });
    out1.textures_delta.clear();
    assert!(app.playback.mini_player);
    assert!(app.playback.mini_player_pending_enter.is_none());
    assert!(app.playback.mini_player_settled);

    // Next frame records user move/resize bounds without snapping back.
    let mut raw_moved = egui::RawInput::default();
    if let Some(vp) = raw_moved.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(false);
        vp.inner_rect = Some(egui::Rect::from_min_size(
            egui::pos2(120.0, 140.0),
            egui::vec2(640.0, 360.0),
        ));
        vp.outer_rect = Some(egui::Rect::from_min_size(
            egui::pos2(112.0, 108.0),
            egui::vec2(656.0, 400.0),
        ));
    }
    let mut out2 = ctx.run_ui(raw_moved, |ui| {
        app.sync_mini_player_viewport(ui.ctx());
    });
    out2.textures_delta.clear();
    assert_eq!(
        app.playback.mini_player_saved_size,
        Some(egui::vec2(640.0, 360.0))
    );
    assert_eq!(
        app.playback.mini_player_saved_pos,
        Some(egui::pos2(112.0, 108.0))
    );

    // Maximizing (or Aero-snapping) after settling exits PiP cleanly.
    let mut raw_max = egui::RawInput::default();
    if let Some(vp) = raw_max.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(true);
    }
    let mut out3 = ctx.run_ui(raw_max, |ui| {
        app.sync_mini_player_viewport(ui.ctx());
    });
    out3.textures_delta.clear();
    assert!(!app.playback.mini_player);
    assert!(!app.playback.mini_player_settled);
}

#[test]
fn poll_quality_populates_subtitles_from_playback_metadata() {
    let mut app = FastTube::default();
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    app.watch.extras.sync_video(Some(url));
    assert!(!app.watch.extras.subtitles_loaded);

    let (tx, rx) = mpsc::channel();
    app.playback.quality_rx = Some(rx);
    tx.send(Ok(search::VideoPlaybackMetadata {
        heights: vec![1080, 720, 480],
        subtitles: vec![search::SubtitleTrack {
            language: "pt".into(),
            url: "https://www.youtube.com/api/timedtext?v=dQw4w9WgXcQ&lang=pt&fmt=vtt".into(),
            automatic: false,
        }],
        ..Default::default()
    }))
    .unwrap();

    app.poll_quality();
    assert!(app.playback.quality_rx.is_none());
    assert_eq!(app.playback.qualities, vec![1080, 720, 480]);
    assert!(app.watch.extras.subtitles_loaded);
    assert_eq!(app.watch.extras.subtitles.len(), 1);
    assert_eq!(app.watch.extras.subtitles[0].language, "pt");
}

#[test]
fn record_active_playback_ticks_keep_feed_signal_revision_stable_until_completion() {
    let mut app = FastTube::default();
    app.feed.cache_dirty = false;
    let initial_rev = app.feed.signal_revision;

    app.playback.active = Some(ActivePlayback {
        video_id: Some("dQw4w9WgXcQ".into()),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        title: "Sample".into(),
        channel: "Channel".into(),
        position_ms: 5_000,
        duration_ms: Some(120_000),
        resume_target_ms: None,
        resume_seek_pending: false,
        start_option_cleared: true,
        has_live_sample: true,
        stale_guard_pos_ms: None,
        is_live: false,
        live_dvr: false,
        at_live_edge: false,
        live_paused_since: None,
        live_paused_accumulated_secs: 0.0,
        live_behind_secs: 0.0,
    });

    // First sample inserts history entry -> invalidates once.
    assert!(app.record_active_playback_in_memory());
    let after_first = app.feed.signal_revision;
    assert_ne!(after_first, initial_rev);

    // Clear dirty flag and simulate normal playback progression.
    app.feed.cache_dirty = false;
    if let Some(active) = &mut app.playback.active {
        active.position_ms = 25_000;
    }
    assert!(app.record_active_playback_in_memory());
    assert_eq!(app.feed.signal_revision, after_first);
    assert!(!app.feed.cache_dirty);

    // Reaching near-end completion updates history ranking -> invalidates.
    if let Some(active) = &mut app.playback.active {
        active.position_ms = 116_000;
    }
    assert!(app.record_active_playback_in_memory());
    assert_ne!(app.feed.signal_revision, after_first);
}

#[test]
fn watch_sidebar_cache_reuses_cards_and_detects_tab_and_feed_mutations() {
    let mut app = FastTube::default();
    let v1 = Video {
        title: "Current Video".into(),
        channel: "Canal A".into(),
        duration: "10:00".into(),
        url: "https://www.youtube.com/watch?v=aaaaaaaaaaa".into(),
        thumbnail: String::new(),
    };
    let v2 = Video {
        title: "Next From Canal A".into(),
        channel: "Canal A".into(),
        duration: "08:00".into(),
        url: "https://www.youtube.com/watch?v=bbbbbbbbbbb".into(),
        thumbnail: String::new(),
    };
    let v3 = Video {
        title: "Other Channel Video".into(),
        channel: "Canal B".into(),
        duration: "12:00".into(),
        url: "https://www.youtube.com/watch?v=ccccccccccc".into(),
        thumbnail: String::new(),
    };
    app.selected = Some(v1.clone());
    app.feed.videos = vec![v1, v2.clone(), v3.clone()];

    app.ensure_watch_sidebar_cache();
    assert!(app.watch.sidebar_cache.valid);
    assert_eq!(app.watch.sidebar_cache.cards.len(), 2);

    // Record measured height on cached card and ensure subsequent unchanged checks preserve it.
    app.watch.sidebar_cache.cards[0].measured_height = Some((380, 96.0));
    app.ensure_watch_sidebar_cache();
    assert_eq!(
        app.watch.sidebar_cache.cards[0].measured_height,
        Some((380, 96.0))
    );

    // Switching SidebarTab rebuilds filtered list cleanly.
    app.watch.extras.sidebar_tab = watch_extras::SidebarTab::FromChannel;
    app.ensure_watch_sidebar_cache();
    assert_eq!(app.watch.sidebar_cache.cards.len(), 1);
    assert_eq!(app.watch.sidebar_cache.cards[0].video.url, v2.url);

    // Direct mutation of `feed.videos` (like tests or unhooked writers) is auto-detected.
    app.watch.extras.sidebar_tab = watch_extras::SidebarTab::All;
    app.ensure_watch_sidebar_cache();
    assert_eq!(app.watch.sidebar_cache.cards.len(), 2);
    app.feed.videos.push(Video {
        title: "Newly Pushed".into(),
        channel: "Canal C".into(),
        duration: "05:00".into(),
        url: "https://www.youtube.com/watch?v=ddddddddddd".into(),
        thumbnail: String::new(),
    });
    app.ensure_watch_sidebar_cache();
    assert_eq!(app.watch.sidebar_cache.cards.len(), 3);
}

fn collect_rendered_text_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
    let mut entries = Vec::new();
    fn visit(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                out.push((text.galley.job.text.clone(), rect));
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
        visit(&clipped.shape, &mut entries);
    }
    entries
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
        "TIPO",
        "DURAÇÃO",
        "DATA DO UPLOAD",
        "CARACTERÍSTICAS",
        "PRIORIDADE",
        "Vídeos",
        "Shorts",
        "Canais",
        "Playlists",
        "Filmes",
        "Menos de 3 minutos",
        "De 3 a 20 minutos",
        "Com mais de 20 minutos",
        "Hoje",
        "Esta semana",
        "Este mês",
        "Este ano",
        "Ao vivo",
        "4K",
        "Alta Definição",
        "Legendas/CC",
        "Relevância",
        "Popularidade",
    ] {
        assert!(
            rendered.contains(expected),
            "missing '{expected}' in filter modal:\n{rendered}"
        );
    }

    let pop_pos = text_rects
        .iter()
        .find(|(t, _)| t == "Popularidade")
        .expect("Popularidade option")
        .1
        .center();

    let click_events = vec![
        egui::Event::PointerMoved(pop_pos),
        egui::Event::PointerButton {
            pos: pop_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: pop_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_events,
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.search_filters_modal_ui(&c);
        },
    );
    click_out.textures_delta.clear();

    assert_eq!(app.feed.search_filters.sort, search::SearchSort::Views);
    assert!(!app.feed.search_filters_open);
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
fn share_modal_opens_from_watch_and_updates_timestamp_and_embed() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let mut video = sample_videos().remove(0);
    video.url = "https://www.youtube.com/watch?v=_wCXFKpfcgg".into();
    app.selected = Some(video.clone());
    app.auth.my_channel_subscribers = Some(3);

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 760.0));
    let mut watch_out = egui::FullOutput::default();
    for _ in 0..2 {
        watch_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.watch_metadata_card_ui(ui, &video, 680.0, None);
            },
        );
        watch_out.textures_delta.clear();
    }
    let watch_rects = collect_rendered_text_rects(&watch_out.shapes);
    let share_btn_pos = watch_rects
        .iter()
        .find(|(t, _)| t == "Compartilhar")
        .expect("Compartilhar watch button")
        .1
        .center();

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

    let mut open_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_at(share_btn_pos),
            ..Default::default()
        },
        |ui| {
            app.watch_metadata_card_ui(ui, &video, 680.0, None);
        },
    );
    open_out.textures_delta.clear();
    assert_eq!(app.feed.share_modal_video.as_ref(), Some(&video));

    let mut share_out = egui::FullOutput::default();
    for _ in 0..2 {
        share_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.share_modal_ui(&c);
            },
        );
        share_out.textures_delta.clear();
    }
    let share_rects = collect_rendered_text_rects(&share_out.shapes);
    let share_text = share_rects
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "Compartilhar em um post",
        "Criar post",
        "3 inscritos",
        "Compartilhar",
        "Incorporar",
        "WhatsApp",
        "Mensagens",
        "Facebook",
        "https://youtu.be/_wCXFKpfcgg",
        "Copiar",
        "Começar em",
    ] {
        assert!(
            share_text.contains(expected),
            "missing '{expected}' in share modal:\n{share_text}"
        );
    }

    // Enable "Começar em" with 1:23 -> URL updates with ?t=83
    app.feed.share_start_at_text = "1:23".into();
    let start_at_pos = share_rects
        .iter()
        .find(|(t, _)| t == "Começar em")
        .expect("Começar em label")
        .1
        .center();
    let mut toggle_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_at(start_at_pos),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.share_modal_ui(&c);
        },
    );
    toggle_out.textures_delta.clear();
    assert!(app.feed.share_start_at_enabled);
    toggle_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.share_modal_ui(&c);
        },
    );
    toggle_out.textures_delta.clear();
    let toggled_text = collect_rendered_text(&toggle_out.shapes).join("\n");
    assert!(
        toggled_text.contains("https://youtu.be/_wCXFKpfcgg?t=83"),
        "expected ?t=83 in:\n{toggled_text}"
    );

    // Click "Incorporar" -> switches to iframe embed snippet with ?start=83
    let embed_pos = share_rects
        .iter()
        .find(|(t, _)| t == "Incorporar")
        .expect("Incorporar button")
        .1
        .center();
    let mut embed_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_at(embed_pos),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.share_modal_ui(&c);
        },
    );
    embed_out.textures_delta.clear();
    assert!(app.feed.share_embed_open);
    embed_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.share_modal_ui(&c);
        },
    );
    embed_out.textures_delta.clear();
    let embed_text = collect_rendered_text(&embed_out.shapes).join("\n");
    assert!(
        embed_text.contains("https://www.youtube.com/embed/_wCXFKpfcgg?start=83"),
        "expected embed iframe URL in:\n{embed_text}"
    );
}

#[test]
fn poll_report_updates_session_and_status_message() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("tok-1"));
    let (tx, rx) = mpsc::channel();
    app.auth.report_rx = Some(rx);
    tx.send((
        Some(oauth::TokenSession::test_session("tok-2")),
        Ok("Spam ou enganoso".into()),
    ))
    .unwrap();

    app.poll_report(&ctx);

    assert_eq!(
        app.auth.session.as_ref().map(|s| s.access_token.as_str()),
        Some("tok-2")
    );
    assert!(app.auth.report_rx.is_none());
    assert!(app.message.contains("Denúncia enviada ao YouTube"));
    assert!(app.message.contains("Spam ou enganoso"));
}

#[test]
fn live_stream_skips_resume_and_records_zero_position_without_duration() {
    let mut app = FastTube::default();
    let live_video = Video {
        title: "GEN 1x1 BLG | Worlds 2026".into(),
        channel: "CBLOL".into(),
        url: "https://www.youtube.com/watch?v=_UaGVka97no".into(),
        thumbnail: String::new(),
        duration: "Ao vivo".into(),
    };

    app.history
        .data
        .record_active_tick(HistoryEntry {
            video_id: "_UaGVka97no".into(),
            title: live_video.title.clone(),
            channel: live_video.channel.clone(),
            position_ms: 480_000,
            duration_ms: Some(3_600_000),
        })
        .unwrap();

    assert_eq!(app.planned_resume_ms(&live_video), None);

    app.activate_video_playback(&live_video, Some(480_000), None, Instant::now());
    let active = app.playback.active.as_mut().expect("active playback");
    assert!(active.is_live);
    assert_eq!(active.resume_target_ms, None);
    assert!(!active.resume_seek_pending);
    active.has_live_sample = true;
    active.position_ms = 6_361_000;
    active.duration_ms = Some(6_400_000);

    assert!(app.record_active_playback_in_memory());
    let saved = app.history.data.get("_UaGVka97no").expect("history entry");
    assert_eq!(saved.position_ms, 0);
    assert_eq!(saved.duration_ms, None);
}

#[test]
fn watch_metadata_prefers_real_channel_name_over_channel_id_and_formats_live_stats() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = Video {
        title: "GEN 1x1 BLG | Worlds 2026 | Fase Eliminatória - Final (Md5)".into(),
        channel: "UC48rkTIXjRd6pnqqBkdV0Mw".into(),
        url: "https://www.youtube.com/watch?v=_UaGVka97no".into(),
        thumbnail: String::new(),
        duration: "1:46:03".into(),
    };
    app.selected = Some(video.clone());
    app.watch.live.video_url = Some(video.url.clone());
    app.watch.live.info_revision = 1;
    app.watch.live.info = Some(live_chat::WatchLiveInfo {
        video_id: "_UaGVka97no".into(),
        is_live: true,
        is_live_dvr: true,
        title: Some(video.title.clone()),
        channel: Some("CBLOL".into()),
        channel_id: Some("UC48rkTIXjRd6pnqqBkdV0Mw".into()),
        channel_url: Some("https://www.youtube.com/@CBLOL".into()),
        subscriber_count_text: Some("1,6 mi de inscritos".into()),
        concurrent_view_count: Some(61_285),
        started_text: Some("Transmissão iniciada há 5 horas".into()),
        ..Default::default()
    });
    app.apply_live_metadata_to_video(
        &video.url,
        true,
        Some(true),
        Some(&video.title),
        Some("CBLOL"),
        Some("UC48rkTIXjRd6pnqqBkdV0Mw"),
        Some("https://www.youtube.com/@CBLOL"),
        None,
        Some("1,6 mi de inscritos"),
        None,
        Some(61_285),
        None,
    );

    let selected = app.selected.clone().unwrap();
    assert_eq!(selected.channel, "CBLOL");
    assert_eq!(selected.duration, "Ao vivo");

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(960.0, 600.0));
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.watch_metadata_card_ui(ui, &selected, 800.0, None);
            },
        );
        out.textures_delta.clear();
    }
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    assert!(rendered.contains("CBLOL"), "missing CBLOL in:\n{rendered}");
    assert!(
        !rendered.contains("UC48rkTIXjRd6pnqqBkdV0Mw"),
        "raw channel ID leaked into metadata UI:\n{rendered}"
    );
    assert!(
        rendered.contains("1,6 mi de inscritos"),
        "missing subscriber count in:\n{rendered}"
    );
    assert!(
        rendered.contains("61.285 assistindo agora"),
        "missing concurrent viewer count in:\n{rendered}"
    );
    assert!(
        rendered.contains("Transmissão iniciada há 5 horas"),
        "missing started text in:\n{rendered}"
    );
    assert!(
        !rendered.contains("Duração"),
        "live metadata should not show VOD duration label:\n{rendered}"
    );
}

#[test]
fn live_playback_state_tracks_pause_lag_dvr_lag_and_go_live_edge() {
    let mut app = FastTube::default();
    let video = Video {
        title: "Live Stream".into(),
        channel: "Canal".into(),
        url: "https://www.youtube.com/watch?v=_UaGVka97no".into(),
        thumbnail: String::new(),
        duration: "Ao vivo".into(),
    };
    app.selected = Some(video.clone());
    app.watch.live.video_url = Some(video.url.clone());
    app.watch.live.info = Some(live_chat::WatchLiveInfo {
        video_id: "_UaGVka97no".into(),
        is_live: true,
        is_live_dvr: false,
        ..Default::default()
    });
    app.activate_video_playback(&video, None, None, Instant::now());

    let t0 = Instant::now();
    app.update_live_playback_state(false, Some(10.0), None, Some(false), t0);
    {
        let active = app.playback.active.as_ref().unwrap();
        assert!(active.is_live);
        assert!(!active.live_dvr);
        assert!(active.at_live_edge);
        assert_eq!(active.live_behind_secs, 0.0);
    }

    // Pausing for >= 2s puts a non-DVR stream behind the live edge.
    app.update_live_playback_state(true, Some(10.0), None, Some(false), t0);
    app.update_live_playback_state(
        true,
        Some(10.0),
        None,
        Some(false),
        t0 + Duration::from_secs(5),
    );
    {
        let active = app.playback.active.as_ref().unwrap();
        assert!(!active.at_live_edge);
        assert!(active.live_behind_secs >= 4.9);
    }

    app.go_live_edge(&video);
    {
        let active = app.playback.active.as_ref().unwrap();
        assert!(active.at_live_edge);
        assert_eq!(active.live_behind_secs, 0.0);
    }

    // Enable DVR and simulate rewinding 120s behind a 600s DVR window.
    app.watch.live.info.as_mut().unwrap().is_live_dvr = true;
    let t1 = t0 + Duration::from_secs(10);
    app.update_live_playback_state(false, Some(480.0), Some(600.0), Some(true), t1);
    {
        let active = app.playback.active.as_ref().unwrap();
        assert!(active.live_dvr);
        assert!(!active.at_live_edge);
        assert!((active.live_behind_secs - 120.0).abs() < 0.01);
    }

    // Seeking back near the DVR head snaps back onto live edge.
    app.update_live_playback_state(false, Some(597.0), Some(600.0), Some(true), t1);
    {
        let active = app.playback.active.as_ref().unwrap();
        assert!(active.at_live_edge);
        assert_eq!(active.live_behind_secs, 0.0);
    }
}

#[test]
fn watch_live_chat_panel_ui_renders_modes_messages_and_toggles_visibility() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = Video {
        title: "Live".into(),
        channel: "CBLOL".into(),
        url: "https://www.youtube.com/watch?v=_UaGVka97no".into(),
        thumbnail: String::new(),
        duration: "Ao vivo".into(),
    };
    app.selected = Some(video.clone());
    app.watch.live.video_url = Some(video.url.clone());
    app.watch.live.video_id = Some("_UaGVka97no".into());
    app.watch.live.chat_visible = true;
    app.watch.live.auto_scroll = true;
    app.watch.live.send_params = Some("send-token".into());
    app.watch.live.chat_modes = vec![
        live_chat::LiveChatMode {
            label: "Principais mensagens".into(),
            continuation: "cont-top".into(),
            selected: true,
        },
        live_chat::LiveChatMode {
            label: "Chat ao vivo".into(),
            continuation: "cont-all".into(),
            selected: false,
        },
    ];
    app.watch.live.active_continuation = Some("cont-top".into());
    app.watch.live.info = Some(live_chat::WatchLiveInfo {
        video_id: "_UaGVka97no".into(),
        is_live: true,
        modes: app.watch.live.chat_modes.clone(),
        initial_continuation: Some("cont-top".into()),
        ..Default::default()
    });
    app.watch.live.push_message(live_chat::LiveChatMessage {
        id: "m1".into(),
        author_name: "@tiago.menezes075".into(),
        author_channel_id: None,
        author_avatar_url: None,
        badges: vec![live_chat::LiveChatBadgeKind::Member],
        text: "chovy pipoca".into(),
        timestamp_text: None,
        kind: live_chat::LiveChatMessageKind::Text,
    });
    app.watch.live.push_message(live_chat::LiveChatMessage {
        id: "m2".into(),
        author_name: "@apoiador".into(),
        author_channel_id: None,
        author_avatar_url: None,
        badges: vec![],
        text: "Bora GEN.G!".into(),
        timestamp_text: None,
        kind: live_chat::LiveChatMessageKind::Paid {
            amount: "R$ 20,00".into(),
            header_bg: 0xFF15_65C0,
            body_bg: 0xFF1E_88E5,
        },
    });

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(420.0, 620.0));
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.watch_live_chat_panel_ui(ui, &ctx, 380.0, 520.0);
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
    for expected in [
        "Principais mensagens",
        "Ocultar chat",
        "@tiago.menezes075",
        "Membro",
        "chovy pipoca",
        "R$ 20,00",
        "Bora GEN.G!",
        "Enviar",
    ] {
        assert!(
            rendered.contains(expected),
            "missing '{expected}' in live chat panel:\n{rendered}"
        );
    }

    // Collapse chat panel by clicking "Ocultar chat".
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
    let close_pos = rects
        .iter()
        .find(|(t, _)| t == "Ocultar chat")
        .expect("Ocultar chat button")
        .1
        .center();
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_at(close_pos),
            ..Default::default()
        },
        |ui| {
            app.watch_live_chat_panel_ui(ui, &ctx, 380.0, 520.0);
        },
    );
    click_out.textures_delta.clear();
    assert!(!app.watch.live.chat_visible);
    assert!(app.watch.live.chat_user_toggled);

    let mut collapsed_out = egui::FullOutput::default();
    for _ in 0..2 {
        collapsed_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.watch_live_chat_panel_ui(ui, &ctx, 380.0, 520.0);
            },
        );
        collapsed_out.textures_delta.clear();
    }
    let collapsed_text = collect_rendered_text(&collapsed_out.shapes).join("\n");
    assert!(
        collapsed_text.contains("Mostrar chat ao vivo"),
        "expected collapsed toggle button in:\n{collapsed_text}"
    );
}

#[test]
fn handle_player_overlay_action_live_non_dvr_disables_loop_autoplay_and_restores_live_edge() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = Video {
        title: "Live".into(),
        channel: "CBLOL".into(),
        url: "https://www.youtube.com/watch?v=_UaGVka97no".into(),
        thumbnail: String::new(),
        duration: "Ao vivo".into(),
    };
    app.selected = Some(video.clone());
    app.playback.autoplay = true;
    app.playback.loop_video = false;
    app.activate_video_playback(&video, None, None, Instant::now());
    {
        let active = app.playback.active.as_mut().unwrap();
        active.is_live = true;
        active.live_dvr = false;
        active.at_live_edge = false;
        active.live_behind_secs = 28.0;
    }

    let overlay_ctx = OverlayPlaybackContext {
        video: video.clone(),
        paused: false,
        ended: false,
        muted: false,
        position: 100.0,
        duration: None,
        buffered_secs: None,
        raw_volume: 80.0,
        speed: 1.0,
        current_height: Some(1080),
        selected_quality: None,
        qualities: vec![1080, 720],
        qualities_loading: false,
        has_queued: false,
        has_active_subtitle: false,
        subtitles: Vec::new(),
        subtitles_loading: false,
        chapters: Vec::new(),
        theater: false,
        fullscreen: false,
        mini_player: false,
        autoplay: true,
        loop_video: false,
        is_live: true,
        is_live_dvr: false,
        at_live_edge: false,
        live_behind_secs: 28.0,
        can_bookmark: true,
        raw_path: Some(video.url.clone()),
        raw_position: Some(100.0),
        now: Instant::now(),
    };

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleAutoplay, &overlay_ctx, &ctx);
    assert!(app.playback.autoplay);
    app.handle_player_overlay_action(overlay::OverlayAction::ToggleLoop, &overlay_ctx, &ctx);
    assert!(!app.playback.loop_video);
    app.handle_player_overlay_action(overlay::OverlayAction::SeekBack10, &overlay_ctx, &ctx);
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 0);

    app.handle_player_overlay_action(overlay::OverlayAction::GoLive, &overlay_ctx, &ctx);
    let active = app.playback.active.as_ref().unwrap();
    assert!(active.at_live_edge);
    assert_eq!(active.live_behind_secs, 0.0);
}

#[test]
fn settings_page_renders_all_six_tabs_and_compact_layout_headlessly() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.ui.settings_open = true;
    app.selected = Some(Video {
        title: "Video em segundo plano".into(),
        channel: "Canal Teste".into(),
        duration: "5:00".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
    });

    let expectations = [
        (app_state::SettingsTab::General, "Aparência e feed"),
        (
            app_state::SettingsTab::Playback,
            "Decodificação por hardware (libmpv)",
        ),
        (
            app_state::SettingsTab::SponsorBlock,
            "Integração SponsorBlock",
        ),
        (
            app_state::SettingsTab::Privacy,
            "Histórico e recomendações locais",
        ),
        (app_state::SettingsTab::Storage, "Local da pasta de cache"),
        (
            app_state::SettingsTab::BackupDiagnostics,
            "Backup de dados locais",
        ),
    ];

    for (tab, expected_text) in expectations {
        app.settings.active_tab = tab;
        let mut rendered = String::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1120.0, 2400.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let c = ui.ctx().clone();
                    app.settings_page_ui(ui, &c);
                },
            );
            rendered = collect_rendered_text(&output.shapes).join("\n");
            output.textures_delta.clear();
        }
        assert!(
            rendered.contains(expected_text),
            "tab {tab:?} missing `{expected_text}` in:\n{rendered}"
        );
        assert!(rendered.contains("O vídeo continua em reprodução em segundo plano"));
    }

    // Narrow viewport collapses left rail into top pills and still renders cleanly.
    app.settings.active_tab = app_state::SettingsTab::SponsorBlock;
    let mut narrow_rendered = String::new();
    for _ in 0..2 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 1200.0),
                )),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.settings_page_ui(ui, &c);
            },
        );
        narrow_rendered = collect_rendered_text(&output.shapes).join("\n");
        output.textures_delta.clear();
    }
    assert!(narrow_rendered.contains("← Voltar"));
    assert!(narrow_rendered.contains("Categorias de segmentos"));
}

#[test]
fn app_preferences_resume_privacy_and_feed_gates_work_as_configured() {
    let mut app = FastTube::default();
    let regular = Video {
        title: "Rust Tutorial".into(),
        channel: "Ferris".into(),
        duration: "5:00".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
    };
    let short_video = Video {
        title: "Quick Short".into(),
        channel: "Ferris".into(),
        duration: "0:30".into(),
        url: "https://www.youtube.com/shorts/short000001".into(),
        thumbnail: String::new(),
    };
    app.feed.videos = vec![regular.clone(), short_video.clone()];

    // Resume gate: enabled returns saved timestamp, disabled returns None.
    app.record_video_in_history(&regular, 120_000, Some(300_000))
        .unwrap();
    assert_eq!(app.planned_resume_ms(&regular), Some(120_000));
    app.settings.prefs.value.resume_enabled = false;
    assert_eq!(app.planned_resume_ms(&regular), None);

    // Pause watch history prevents new history entries.
    app.settings.prefs.value.pause_watch_history = true;
    let another = Video {
        title: "Paused Video".into(),
        channel: "Ferris".into(),
        duration: "4:00".into(),
        url: "https://www.youtube.com/watch?v=oHg5SJYRHA0".into(),
        thumbnail: String::new(),
    };
    app.record_video_in_history(&another, 45_000, Some(240_000))
        .unwrap();
    assert!(app.history.data.get("oHg5SJYRHA0").is_none());

    // Pause recommendations resets derived UserProfile to empty default.
    app.settings.prefs.value.pause_recommendations = false;
    app.invalidate_feed_cache();
    app.ensure_feed_cache();
    assert!(
        app.feed
            .cache
            .profile
            .channel_weights
            .contains_key("ferris")
    );
    assert!(!app.feed.cache.profile.token_weights.is_empty());

    app.settings.prefs.value.pause_recommendations = true;
    app.ensure_feed_cache();
    assert!(app.feed.cache.profile.channel_weights.is_empty());
    assert!(app.feed.cache.profile.token_weights.is_empty());

    // Hide Shorts in Home removes Shorts from Home ranked list, but preserves BrowseSection::Shorts.
    app.settings.prefs.value.hide_shorts_in_home = true;
    app.ui.browse_section = BrowseSection::Home;
    app.ensure_feed_cache();
    assert!(
        app.feed
            .cache
            .ranked
            .iter()
            .all(|item| item.video.url != short_video.url)
    );
    app.ui.browse_section = BrowseSection::Shorts;
    app.ensure_feed_cache();
    assert!(
        app.feed
            .cache
            .ranked
            .iter()
            .any(|item| item.video.url == short_video.url)
    );

    // Compact card density increases grid columns on wide viewports.
    app.settings.prefs.value.compact_card_density = false;
    let (_, normal_cols) = app.home_grid_metrics(1200.0);
    app.settings.prefs.value.compact_card_density = true;
    let (_, compact_cols) = app.home_grid_metrics(1200.0);
    assert!(compact_cols > normal_cols);
}

#[test]
fn sponsorblock_auto_skip_manual_skip_and_toast_respect_preferences() {
    let mut app = FastTube::default();
    let now = Instant::now();
    app.watch.sponsorblock.segments = vec![
        sponsorblock::SponsorSegment {
            uuid: "seg-sponsor-1".into(),
            start_secs: 10.0,
            end_secs: 25.0,
            category: sponsorblock::SponsorCategory::Sponsor,
        },
        sponsorblock::SponsorSegment {
            uuid: "seg-short-intro".into(),
            start_secs: 30.0,
            end_secs: 30.5,
            category: sponsorblock::SponsorCategory::Intro,
        },
    ];
    app.settings.prefs.value.sponsorblock.enabled = true;
    app.settings.prefs.value.sponsorblock.skip_mode = sponsorblock::SponsorSkipMode::AutoSkip;
    app.settings.prefs.value.sponsorblock.min_duration_secs = 1.0;
    app.settings.prefs.value.sponsorblock.show_skip_toast = true;

    // Auto-skip matches valid segment once and records toast.
    assert_eq!(
        app.try_auto_skip_sponsor_segment(12.0, Some(120.0), false, now),
        Some(25.0)
    );
    assert_eq!(
        app.watch.sponsorblock.active_skip_toast(now),
        Some("Pulado: Patrocínio")
    );
    // Same UUID is not auto-skipped a second time in the same playback.
    assert_eq!(
        app.try_auto_skip_sponsor_segment(12.5, Some(120.0), false, now),
        None
    );
    // Segment shorter than min_duration_secs (0.5s < 1.0s) is ignored.
    assert_eq!(
        app.try_auto_skip_sponsor_segment(30.2, Some(120.0), false, now),
        None
    );
    // Live streams never auto-skip.
    assert_eq!(
        app.try_auto_skip_sponsor_segment(12.0, Some(120.0), true, now),
        None
    );

    // Manual button mode disables auto-skip, while manual skip marks UUID and toast.
    app.watch.sponsorblock.auto_skipped_uuids.clear();
    app.watch.sponsorblock.last_skip_notice = None;
    app.settings.prefs.value.sponsorblock.skip_mode = sponsorblock::SponsorSkipMode::ManualButton;
    assert_eq!(
        app.try_auto_skip_sponsor_segment(12.0, Some(120.0), false, now),
        None
    );
    app.record_manual_sponsor_skip(12.0, 25.0, now);
    assert!(
        app.watch
            .sponsorblock
            .auto_skipped_uuids
            .contains("seg-sponsor-1")
    );
    assert_eq!(
        app.watch.sponsorblock.active_skip_toast(now),
        Some("Pulado: Patrocínio")
    );
}

#[test]
fn backup_round_trip_preserves_and_restores_app_preferences() {
    let temp_dir =
        std::env::temp_dir().join(format!("fasttube-prefs-backup-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let backup_file = temp_dir.join("backup.json");

    let configure_paths = |app: &mut FastTube| {
        app.library.path = Some(temp_dir.join("library.json"));
        app.queue.path = Some(temp_dir.join("queue.json"));
        app.history.path = Some(temp_dir.join("history.json"));
        app.settings.playback.path = Some(temp_dir.join("playback-settings.txt"));
        app.settings.theme.path = Some(temp_dir.join("theme-settings.txt"));
        app.feed_preferences.path = Some(temp_dir.join("feed-prefs.json"));
        app.settings.buffer.path = Some(temp_dir.join("buffer-settings.txt"));
        app.settings.prefs.path = Some(temp_dir.join("app-preferences.json"));
    };

    let mut exporter = FastTube::default();
    configure_paths(&mut exporter);
    exporter.settings.prefs.value.seek_step_secs = 15;
    exporter.settings.prefs.value.hwdec_mode = settings::HwdecMode::Software;
    exporter.settings.prefs.value.sponsorblock.skip_mode =
        sponsorblock::SponsorSkipMode::ManualButton;
    exporter.settings.prefs.value.pause_watch_history = true;
    exporter.settings.prefs.value.compact_card_density = true;
    exporter.backup.path_text = backup_file.display().to_string();
    exporter.export_user_data();
    assert!(backup_file.is_file());

    let mut importer = FastTube::default();
    configure_paths(&mut importer);
    importer.backup.path_text = backup_file.display().to_string();
    importer.analyze_import();
    assert!(importer.backup.confirm_import);
    importer.backup.import_mode = BackupImportMode::Replace;
    importer.import_user_data();

    assert_eq!(importer.settings.prefs.value, exporter.settings.prefs.value);
    let persisted = settings::load_app_preferences(&temp_dir.join("app-preferences.json")).unwrap();
    assert_eq!(persisted, exporter.settings.prefs.value);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn settings_search_custom_buffer_and_backup_modal_render_headlessly() {
    let mut app = FastTube::default();
    app.ui.settings_open = true;
    app.settings.buffer.profile = BufferProfile::Custom(settings::BufferSettings {
        max_mib: 96,
        back_mib: 24,
        cache_seconds: 45,
    });
    app.settings.custom_buffer_max_mib = 96;
    app.settings.custom_buffer_back_mib = 24;
    app.settings.custom_buffer_cache_secs = 45;

    let ctx = egui::Context::default();
    let run_settings = |app: &mut FastTube| -> String {
        let mut rendered = String::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1120.0, 3600.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let c = ui.ctx().clone();
                    app.settings_page_ui(ui, &c);
                },
            );
            rendered = collect_rendered_text(&output.shapes).join("\n");
            output.textures_delta.clear();
        }
        rendered
    };

    // Playback tab renders custom buffer sliders and Audio & Subtitles card.
    app.settings.active_tab = app_state::SettingsTab::Playback;
    let playback_text = run_settings(&mut app);
    assert!(playback_text.contains("Áudio e legendas"));
    assert!(playback_text.contains("Normalização de áudio (loudnorm)"));
    assert!(playback_text.contains("Personalizado — 96 MiB máx / 24 MiB retorno / 45s"));
    assert!(playback_text.contains("Aplicar buffer personalizado"));

    // Cross-tab search matches entries across tabs and handles empty results.
    app.settings.search_query = "loudnorm".into();
    let search_hit = run_settings(&mut app);
    assert!(search_hit.contains("Resultados para \"loudnorm\""));
    assert!(search_hit.contains("Áudio e legendas"));
    assert!(search_hit.contains("normalização (loudnorm)"));
    assert!(search_hit.contains("Abrir aba →"));

    app.settings.search_query = "zzzz_inexistente_12345".into();
    let search_miss = run_settings(&mut app);
    assert!(search_miss.contains("Nenhuma configuração encontrada"));

    // Backup confirmation modal renders cleanly when confirm_import is active.
    app.settings.search_query.clear();
    app.settings.active_tab = app_state::SettingsTab::BackupDiagnostics;
    app.backup.confirm_import = true;
    app.backup.preview = Some(BackupPreview {
        path: std::path::PathBuf::from("fasttube-backup.json"),
        library_items: 4,
        favorites: 2,
        watch_later: 1,
        collections: 1,
        bookmarks: 3,
        queue_items: 2,
        history_entries: 5,
        merge_added: 3,
        merge_conflicts: 1,
        merge_skipped: 0,
        history_progress_updates: 2,
    });
    let modal_text = run_settings(&mut app);
    assert!(modal_text.contains("Prévia da importação"));
    assert!(modal_text.contains("fasttube-backup.json"));
    assert!(modal_text.contains("Aplicar backup"));
}

#[test]
fn settings_tab_defaults_reset_and_muted_topics_affect_feed_profile() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();

    app.settings.theme.value = settings::Theme::Light;
    app.settings.prefs.value.compact_card_density = true;
    app.settings.prefs.value.seek_step_secs = 30;
    app.settings.prefs.value.audio_normalization = true;
    app.settings.prefs.value.sponsorblock.enabled = false;
    app.settings.prefs.value.sponsorblock.min_duration_secs = 6.0;

    app.reset_settings_tab_defaults(app_state::SettingsTab::General, &ctx);
    assert_eq!(app.settings.theme.value, settings::Theme::Dark);
    assert!(!app.settings.prefs.value.compact_card_density);
    assert_eq!(app.settings.prefs.value.seek_step_secs, 30);

    app.reset_settings_tab_defaults(app_state::SettingsTab::Playback, &ctx);
    assert_eq!(app.settings.prefs.value.seek_step_secs, 10);
    assert!(!app.settings.prefs.value.audio_normalization);
    assert!(!app.settings.prefs.value.sponsorblock.enabled);

    app.reset_settings_tab_defaults(app_state::SettingsTab::SponsorBlock, &ctx);
    assert!(app.settings.prefs.value.sponsorblock.enabled);
    assert_eq!(app.settings.prefs.value.sponsorblock.min_duration_secs, 1.0);

    // Muted topics suppress learned topic weights in derived feed profile.
    let video = Video {
        title: "Rust Async Streams Tutorial".into(),
        channel: "Ferris".into(),
        duration: "10:00".into(),
        url: "https://www.youtube.com/watch?v=abc123def45".into(),
        thumbnail: String::new(),
    };
    app.record_video_in_history(&video, 60_000, Some(600_000))
        .unwrap();
    app.invalidate_feed_cache();
    app.ensure_feed_cache();
    assert!(app.feed.cache.profile.token_weights.contains_key("rust"));
    assert!(app.feed.cache.profile.token_weights.contains_key("streams"));

    app.settings.prefs.value.muted_topics = vec!["streams".into()];
    app.ensure_feed_cache();
    assert!(app.feed.cache.profile.token_weights.contains_key("rust"));
    assert!(!app.feed.cache.profile.token_weights.contains_key("streams"));
}

fn test_sub_result(video_id: &str, title: &str, channel: &str, channel_id: &str) -> SearchResult {
    SearchResult {
        video: Video {
            title: title.into(),
            channel: channel.into(),
            url: format!("https://www.youtube.com/watch?v={video_id}"),
            thumbnail: format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"),
            duration: "10:00".into(),
        },
        metadata: VideoMetadata {
            channel_id: Some(channel_id.into()),
            upload_date: Some("2026-11-14T12:00:00+00:00".into()),
            ..VideoMetadata::default()
        },
    }
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
    assert!(app.feed.home.rx.is_none());
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
    assert!(app.feed.home.rx.is_some());
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
    app.feed.home.subscriptions_active_request_id = Some(42);
    app.feed.home.subscriptions_progressive_streaming = true;
    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);

    // Stale progress from an older request_id must be discarded.
    sender
        .send(HomeFetchMessage::SubscriptionProgress {
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
        })
        .unwrap();
    // Active first batch should render immediately while still fetching.
    sender
        .send(HomeFetchMessage::SubscriptionProgress {
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
        })
        .unwrap();
    app.poll_async_tasks(&ctx);

    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Lote Inicial");
    assert!(app.feed.home.rx.is_some());

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
        .send(HomeFetchMessage::SubscriptionFinished {
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
        })
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
    app.feed.home.subscriptions_active_request_id = Some(7);
    app.feed.home.subscriptions_progressive_streaming = false;
    app.feed.home.subscriptions_refreshing_in_background = true;
    let (sender, receiver) = mpsc::channel();
    app.feed.home.rx = Some(receiver);

    // Partial batch during background refresh must not jump cards on screen.
    sender
        .send(HomeFetchMessage::SubscriptionProgress {
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
        })
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
        .send(HomeFetchMessage::SubscriptionFinished {
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
        })
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
    if let Some(cancel) = app.feed.home.batch_cancel.take() {
        cancel.cancel();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compute_repaint_interval_throttles_idle_and_prioritizes_active_states() {
    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput::default()),
        None
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            minimized: true,
            user_interacting: true,
            video_actively_playing: true,
            thumbnails_loading: true,
            async_jobs_active: true,
            ..Default::default()
        }),
        None
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            user_interacting: true,
            video_actively_playing: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(16))
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            video_actively_playing: true,
            toast_or_animation_active: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(100))
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            toast_or_animation_active: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(80))
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            thumbnails_loading: true,
            async_jobs_active: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(80))
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            async_jobs_active: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(150))
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            native_hwnd_fallback_active: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(150))
    );
}

#[test]
fn disk_image_cache_for_override_ignores_default_in_tests_and_uses_explicit_override() {
    assert!(FastTube::disk_image_cache_for_override(None).is_none());

    let dir = std::env::temp_dir().join(format!("fasttube-main-disk-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let override_str = dir.to_string_lossy().into_owned();
    let cache = FastTube::disk_image_cache_for_override(Some(&override_str))
        .expect("explicit override should produce a DiskImageCache");
    let expected_path = cache.entry_path_for_uri("https://i.ytimg.com/vi/abc/hqdefault.jpg");
    assert!(expected_path.starts_with(dir.join("thumbnails")));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ab_loop_overlay_actions_and_auto_seek_enforcer() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    let mut video = sample_videos().remove(0);
    video.url = "https://www.youtube.com/watch?v=abLoopTest1".into();
    let now = Instant::now();
    app.selected = Some(video.clone());
    app.activate_video_playback(&video, Some(0), None, now);

    let mut overlay_ctx = OverlayPlaybackContext {
        video: video.clone(),
        paused: false,
        ended: false,
        muted: false,
        position: 20.0,
        duration: Some(180.0),
        buffered_secs: Some(30.0),
        raw_volume: 100.0,
        speed: 1.0,
        current_height: Some(1080),
        selected_quality: None,
        qualities: vec![1080, 720],
        qualities_loading: false,
        has_queued: false,
        has_active_subtitle: false,
        subtitles: Vec::new(),
        subtitles_loading: false,
        chapters: Vec::new(),
        theater: false,
        fullscreen: false,
        mini_player: false,
        autoplay: false,
        loop_video: false,
        can_bookmark: true,
        is_live: false,
        is_live_dvr: false,
        at_live_edge: false,
        live_behind_secs: 0.0,
        raw_path: Some(video.url.clone()),
        raw_position: Some(20.0),
        now,
    };

    app.handle_player_overlay_action(overlay::OverlayAction::SetLoopA, &overlay_ctx, &ctx);
    assert_eq!(app.playback.loop_a_secs, Some(20.0));
    assert_eq!(app.playback.ab_loop_active(), None);

    // Setting B <= A clears A so an invalid inverted range cannot stay active.
    overlay_ctx.position = 10.0;
    app.handle_player_overlay_action(overlay::OverlayAction::SetLoopB, &overlay_ctx, &ctx);
    assert_eq!(app.playback.loop_a_secs, None);
    assert_eq!(app.playback.loop_b_secs, Some(10.0));

    // Proper A < B activates the loop and auto-seeks back to A when reaching B.
    overlay_ctx.position = 15.0;
    app.handle_player_overlay_action(overlay::OverlayAction::SetLoopA, &overlay_ctx, &ctx);
    overlay_ctx.position = 45.0;
    app.handle_player_overlay_action(overlay::OverlayAction::SetLoopB, &overlay_ctx, &ctx);
    assert_eq!(app.playback.ab_loop_active(), Some((15.0, 45.0)));

    assert!(!app.enforce_ab_loop_seek(30.0, Some(180.0), false, now));
    assert!(!app.enforce_ab_loop_seek(45.0, Some(180.0), true, now));
    assert!(app.enforce_ab_loop_seek(45.2, Some(180.0), false, now));
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 15_000);
    // Debounce prevents immediate re-trigger within 350ms.
    assert!(!app.enforce_ab_loop_seek(
        45.2,
        Some(180.0),
        false,
        now + std::time::Duration::from_millis(100)
    ));
    // Re-triggers once cooldown elapses.
    assert!(app.enforce_ab_loop_seek(
        46.0,
        Some(180.0),
        false,
        now + std::time::Duration::from_millis(450)
    ));

    app.handle_player_overlay_action(overlay::OverlayAction::ClearLoopAB, &overlay_ctx, &ctx);
    assert_eq!(app.playback.loop_a_secs, None);
    assert_eq!(app.playback.loop_b_secs, None);
}

#[test]
fn audio_only_mode_toggles_visibility_and_paints_placeholder() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    let video = Video {
        title: "Podcast Rust Ao Vivo".into(),
        channel: "Canal Dev".into(),
        ..sample_videos().remove(0)
    };
    app.selected = Some(video.clone());
    assert!(app.should_show_video_window());

    let overlay_ctx = OverlayPlaybackContext {
        video: video.clone(),
        paused: false,
        ended: false,
        muted: false,
        position: 5.0,
        duration: Some(300.0),
        buffered_secs: None,
        raw_volume: 100.0,
        speed: 1.0,
        current_height: None,
        selected_quality: None,
        qualities: Vec::new(),
        qualities_loading: false,
        has_queued: false,
        has_active_subtitle: false,
        subtitles: Vec::new(),
        subtitles_loading: false,
        chapters: Vec::new(),
        theater: false,
        fullscreen: false,
        mini_player: false,
        autoplay: false,
        loop_video: false,
        can_bookmark: true,
        is_live: false,
        is_live_dvr: false,
        at_live_edge: false,
        live_behind_secs: 0.0,
        raw_path: None,
        raw_position: None,
        now: Instant::now(),
    };

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleAudioOnly, &overlay_ctx, &ctx);
    assert!(app.playback.audio_only);
    assert!(!app.should_show_video_window());
    assert!(app.message.contains("somente"));

    let mut rendered = Vec::new();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(720.0, 405.0),
            )),
            ..Default::default()
        },
        |ui| {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 405.0));
            FastTube::paint_audio_only_placeholder(ui.painter(), rect, app.selected.as_ref());
        },
    );
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            rendered.push(t.galley.text().to_owned());
        }
    }
    out.textures_delta.clear();
    assert!(rendered.iter().any(|t| t.contains("Modo somente áudio")));
    assert!(rendered.iter().any(|t| t.contains("Podcast Rust Ao Vivo")));
    assert!(rendered.iter().any(|t| t.contains("Canal Dev")));

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleAudioOnly, &overlay_ctx, &ctx);
    assert!(!app.playback.audio_only);
    assert!(app.should_show_video_window());
}

#[test]
fn screenshot_helpers_resolve_dirs_and_sanitize_filenames() {
    assert_eq!(
        resolve_screenshot_dir_from(
            Some("C:\\Users\\Alice"),
            Some("C:\\Users\\Alice\\AppData\\Local")
        ),
        std::path::PathBuf::from("C:\\Users\\Alice")
            .join("Pictures")
            .join("FastTube")
    );
    assert_eq!(
        resolve_screenshot_dir_from(Some("  "), Some("C:\\LocalAppData")),
        std::path::PathBuf::from("C:\\LocalAppData")
            .join("FastTube")
            .join("screenshots")
    );

    let video = Video {
        title: "Guia <Rust>: Dicas/Truques?".into(),
        url: "https://www.youtube.com/watch?v=shotVideo01".into(),
        ..sample_videos().remove(0)
    };
    let name = build_screenshot_filename(&video, 125.7, 1_700_000_000);
    assert!(name.ends_with("_2m05s_1700000000.png"));
    assert!(!name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));

    let mut app = FastTube {
        selected: Some(video.clone()),
        ..Default::default()
    };
    app.playback.audio_only = true;
    app.capture_player_screenshot(&video, 10.0);
    assert!(app.message.contains("somente"));
}

#[test]
fn player_shortcuts_cover_youtube_keys_ab_loop_speed_and_ignore_text_input() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let mut videos = sample_videos();
    let first = videos.remove(0);
    let next_video = videos.remove(0);
    app.selected = Some(first.clone());
    app.queue.data.add(next_video.clone()).unwrap();
    let now = Instant::now();
    app.activate_video_playback(&first, Some(30_000), None, now);
    if let Some(active) = &mut app.playback.active {
        active.duration_ms = Some(200_000);
        active.position_ms = 30_000;
    }

    fn send_key(
        ctx: &egui::Context,
        app: &mut FastTube,
        key: egui::Key,
        modifiers: egui::Modifiers,
        text: Option<&str>,
    ) {
        let mut events = vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }];
        if let Some(t) = text {
            events.push(egui::Event::Text(t.to_owned()));
        }
        let mut out = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                app.handle_player_shortcuts(ui.ctx(), true);
            },
        );
        out.textures_delta.clear();
    }

    // L seeks +10s regardless of custom seek step, J seeks -10s.
    app.settings.prefs.value.seek_step_secs = 15;
    send_key(&ctx, &mut app, egui::Key::L, egui::Modifiers::NONE, None);
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 40_000);
    send_key(&ctx, &mut app, egui::Key::J, egui::Modifiers::NONE, None);
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 30_000);

    // 0..9 jump to 0%..90%.
    send_key(&ctx, &mut app, egui::Key::Num5, egui::Modifiers::NONE, None);
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 100_000);
    send_key(&ctx, &mut app, egui::Key::Num0, egui::Modifiers::NONE, None);
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 0);

    // [ / ] / \ configure and clear A-B loop.
    if let Some(active) = &mut app.playback.active {
        active.position_ms = 12_000;
    }
    send_key(
        &ctx,
        &mut app,
        egui::Key::OpenBracket,
        egui::Modifiers::NONE,
        Some("["),
    );
    if let Some(active) = &mut app.playback.active {
        active.position_ms = 55_000;
    }
    send_key(
        &ctx,
        &mut app,
        egui::Key::CloseBracket,
        egui::Modifiers::NONE,
        Some("]"),
    );
    assert_eq!(app.playback.ab_loop_active(), Some((12.0, 55.0)));
    send_key(
        &ctx,
        &mut app,
        egui::Key::Backslash,
        egui::Modifiers::NONE,
        Some("\\"),
    );
    assert_eq!(app.playback.ab_loop_active(), None);

    // < / > step playback speed up to 3.0x and down again.
    app.playback.speed = 2.0;
    send_key(
        &ctx,
        &mut app,
        egui::Key::Period,
        egui::Modifiers::SHIFT,
        Some(">"),
    );
    assert_eq!(app.playback.speed, 2.5);
    send_key(
        &ctx,
        &mut app,
        egui::Key::Period,
        egui::Modifiers::SHIFT,
        Some(">"),
    );
    assert_eq!(app.playback.speed, 3.0);
    send_key(
        &ctx,
        &mut app,
        egui::Key::Comma,
        egui::Modifiers::SHIFT,
        Some("<"),
    );
    assert_eq!(app.playback.speed, 2.5);

    // N triggers next queued video without needing Shift.
    app.in_app_ui_pass = true;
    send_key(&ctx, &mut app, egui::Key::N, egui::Modifiers::NONE, None);
    assert!(app.queue.pending_advance);
    app.queue.pending_advance = false;
    let _ = app.queue.data.remove(0);
    app.feed.videos = vec![next_video.clone()];
    app.feed.cache_dirty = true;
    send_key(&ctx, &mut app, egui::Key::N, egui::Modifiers::NONE, None);
    assert_eq!(app.playback.pending_open_video, Some(next_video));

    // When a text input holds keyboard focus, player shortcuts must NOT trigger.
    if let Some(active) = &mut app.playback.active {
        active.position_ms = 60_000;
    }
    let mut query = String::new();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let resp = ui.text_edit_singleline(&mut query);
        resp.request_focus();
    });
    out.textures_delta.clear();
    let mut out = ctx.run_ui(
        egui::RawInput {
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
            let _ = ui.text_edit_singleline(&mut query);
            assert!(ui.ctx().egui_wants_keyboard_input());
            app.handle_player_shortcuts(ui.ctx(), true);
        },
    );
    out.textures_delta.clear();
    assert_eq!(app.playback.active.as_ref().unwrap().position_ms, 60_000);
}

#[test]
fn shortcuts_window_lists_new_player_controls_in_pt_br() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.ui.shortcuts_open = true;
    let raw = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 900.0),
        )),
        ..Default::default()
    };
    let mut warmup = ctx.run_ui(raw(), |ui| app.shortcuts_window_ui(ui.ctx()));
    warmup.textures_delta.clear();
    let mut out = ctx.run_ui(raw(), |ui| app.shortcuts_window_ui(ui.ctx()));
    let texts = collect_rendered_text(&out.shapes);
    out.textures_delta.clear();
    assert!(texts.iter().any(|t| t.contains("J / L")));
    assert!(texts.iter().any(|t| t.contains(", / .")));
    assert!(texts.iter().any(|t| t.contains("< / >")));
    assert!(texts.iter().any(|t| t.contains("[ / ] / \\")));
    assert!(texts.iter().any(|t| t.contains("capturar quadro")));
}

#[test]
fn continue_watching_shelf_renders_non_shorts_excludes_near_end_and_resumes() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.data = History::default();
    app.feed.videos = vec![Video {
        title: "Short em Andamento".into(),
        channel: "Shorts Canal".into(),
        url: "https://www.youtube.com/shorts/ccccccccccc".into(),
        thumbnail: String::new(),
        duration: "0:30".into(),
    }];
    let _ = app.history.data.record(history::Entry {
        video_id: "aaaaaaaaaaa".into(),
        title: "Guia Completo de Rust".into(),
        channel: "Dev Canal".into(),
        position_ms: 120_000,
        duration_ms: Some(300_000),
    });
    let _ = app.history.data.record(history::Entry {
        video_id: "bbbbbbbbbbb".into(),
        title: "Video Quase Terminado".into(),
        channel: "Dev Canal".into(),
        position_ms: 295_000,
        duration_ms: Some(300_000),
    });
    let _ = app.history.data.record(history::Entry {
        video_id: "ccccccccccc".into(),
        title: "Short em Andamento".into(),
        channel: "Shorts Canal".into(),
        position_ms: 15_000,
        duration_ms: Some(30_000),
    });

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(960.0, 480.0));
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let _ = app.continue_watching_ui(ui, &ctx);
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
    assert!(
        rendered.contains("Continuar assistindo"),
        "missing shelf title:\n{rendered}"
    );
    assert!(
        rendered.contains("Guia Completo de Rust"),
        "missing in-progress video:\n{rendered}"
    );
    assert!(
        rendered.contains("Restam 3:00"),
        "missing remaining time indicator:\n{rendered}"
    );
    assert!(
        !rendered.contains("Video Quase Terminado"),
        "near-end video should be excluded:\n{rendered}"
    );
    assert!(
        !rendered.contains("Short em Andamento"),
        "Shorts should be excluded from Continue Watching shelf:\n{rendered}"
    );

    let title_pos = rects
        .iter()
        .find(|(t, _)| t.contains("Guia Completo de Rust"))
        .map(|(_, r)| r.center())
        .unwrap();
    let mut chosen = None;
    let click_events = vec![
        egui::Event::PointerMoved(title_pos),
        egui::Event::PointerButton {
            pos: title_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: title_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: click_events,
            ..Default::default()
        },
        |ui| {
            chosen = app.continue_watching_ui(ui, &ctx);
        },
    );
    click_out.textures_delta.clear();
    assert_eq!(
        chosen.as_ref().map(|v| v.url.as_str()),
        Some("https://www.youtube.com/watch?v=aaaaaaaaaaa")
    );

    // Click "Concluir" and verify the entry is marked watched and toast emitted.
    let done_pos = rects
        .iter()
        .find(|(t, _)| t == "Concluir")
        .map(|(_, r)| r.center())
        .unwrap();
    let done_events = vec![
        egui::Event::PointerMoved(done_pos),
        egui::Event::PointerButton {
            pos: done_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: done_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    let mut done_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: done_events,
            ..Default::default()
        },
        |ui| {
            let _ = app.continue_watching_ui(ui, &ctx);
        },
    );
    done_out.textures_delta.clear();
    let updated_entry = app.history.data.get("aaaaaaaaaaa").unwrap();
    assert!(updated_entry.is_near_end());
    assert!(!app.toasts.is_empty());
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
        rendered.contains("Por que este vídeo?"),
        "missing explainability badge:\n{rendered}"
    );
    assert!(
        rendered.contains("📌 Rust"),
        "missing pinned topic chip indicator:\n{rendered}"
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
    app.feed.search_suggest.rx = Some(rx);
    app.feed.search_suggest.active_request_id = Some(5);
    tx.send(app_state::SearchSuggestFetchMessage {
        request_id: 4,
        query: "rust".into(),
        result: Ok(vec!["stale suggestion".into()]),
    })
    .unwrap();
    app.poll_search_suggestions(&ctx);
    assert!(app.feed.search_suggest.remote_suggestions.is_empty());

    let (tx2, rx2) = mpsc::channel();
    app.feed.search_suggest.rx = Some(rx2);
    app.feed.search_suggest.active_request_id = Some(6);
    tx2.send(app_state::SearchSuggestFetchMessage {
        request_id: 6,
        query: "rust".into(),
        result: Ok(vec!["rust tutorial".into()]),
    })
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
    if let Some(cancel) = app.feed.search.cancel.take() {
        cancel.cancel();
    }
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
    app.feed.videos = vec![s1.clone(), s2.clone(), s3.clone()];
    app.ui.browse_section = BrowseSection::Shorts;
    app.feed.filter = recommendations::FeedFilter::Shorts;
    app.selected = Some(s1.clone());

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1024.0, 760.0));
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
    assert!(
        rendered.contains("1 / 3"),
        "missing index pill:\n{rendered}"
    );
    assert!(
        rendered.contains("Loop"),
        "missing Loop toggle button:\n{rendered}"
    );

    // Click "🔁 Loop" button.
    let loop_pos = rects
        .iter()
        .find(|(t, _)| t.contains("Loop"))
        .map(|(_, r)| r.center())
        .unwrap();
    let mut loop_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(loop_pos),
                egui::Event::PointerButton {
                    pos: loop_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: loop_pos,
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
    loop_out.textures_delta.clear();
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

    // Switch to s2, verify "2 / 3" and press K to return to s1.
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
    assert!(
        rendered2.contains("2 / 3"),
        "missing 2 / 3 pill:\n{rendered2}"
    );
    assert_eq!(prev_short.as_ref().map(|v| &v.url), Some(&s1.url));
}

#[test]
fn live_chat_panel_filters_messages_and_shows_scroll_paused_pill() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = Video {
        title: "Live".into(),
        channel: "Canal".into(),
        url: "https://www.youtube.com/watch?v=live1234567".into(),
        thumbnail: String::new(),
        duration: "Ao vivo".into(),
    };
    app.selected = Some(video.clone());
    app.watch.live.video_url = Some(video.url);
    app.watch.live.video_id = Some("live1234567".into());
    app.watch.live.chat_visible = true;
    app.watch.live.auto_scroll = true;
    for i in 0..24 {
        app.watch.live.push_message(live_chat::LiveChatMessage {
            id: format!("m{i}"),
            author_name: if i == 0 {
                "@alice".into()
            } else {
                format!("@user{i}")
            },
            author_channel_id: None,
            author_avatar_url: None,
            badges: vec![],
            text: if i == 0 {
                "Alguem curte Rust aqui?".into()
            } else {
                format!("Mensagem comum numero {i}")
            },
            timestamp_text: None,
            kind: live_chat::LiveChatMessageKind::Text,
        });
    }

    // Verify keyword filtering shows only matching messages.
    app.watch.live.filter_query = "rust".into();
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(420.0, 420.0));
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.watch_live_chat_panel_ui(ui, &ctx, 380.0, 360.0);
            },
        );
        out.textures_delta.clear();
    }
    let filtered_text = collect_rendered_text_rects(&out.shapes)
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(filtered_text.contains("Alguem curte Rust aqui?"));
    assert!(!filtered_text.contains("Mensagem comum numero 15"));

    // Verify empty filter state message.
    app.watch.live.filter_query = "naoexiste_xyz".into();
    let mut out_empty = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            app.watch_live_chat_panel_ui(ui, &ctx, 380.0, 360.0);
        },
    );
    out_empty.textures_delta.clear();
    let empty_text = collect_rendered_text_rects(&out_empty.shapes)
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(empty_text.contains("Nenhuma mensagem corresponde ao filtro."));

    // Clear filter and scroll away from bottom while auto_scroll is false to see the floating pill.
    app.watch.live.filter_query.clear();
    app.watch.live.auto_scroll = false;
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.watch_live_chat_panel_ui(ui, &ctx, 380.0, 360.0);
            },
        );
        out.textures_delta.clear();
    }
    let rects = collect_rendered_text_rects(&out.shapes);
    let pill_text: String = rects
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        pill_text.contains("Rolagem pausada"),
        "expected scroll-paused floating pill:\n{pill_text}"
    );
    let pill_pos = rects
        .iter()
        .find(|(t, _)| t.contains("Rolagem pausada"))
        .map(|(_, r)| r.center())
        .unwrap();
    let mut pill_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(pill_pos),
                egui::Event::PointerButton {
                    pos: pill_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: pill_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            app.watch_live_chat_panel_ui(ui, &ctx, 380.0, 360.0);
        },
    );
    pill_out.textures_delta.clear();
    assert!(app.watch.live.auto_scroll);
}

#[test]
fn toast_stack_renders_caps_at_three_dismisses_and_expires() {
    let ctx = egui::Context::default();
    let app = FastTube::default();
    let palette = app.palette();
    let mut mgr = toast::ToastManager::with_limits(Duration::from_secs(3), 3);
    let t0 = Instant::now();

    mgr.push_at(toast::ToastKind::Info, "Um", Duration::from_secs(3), t0);
    mgr.push_at(
        toast::ToastKind::Success,
        "Dois",
        Duration::from_secs(3),
        t0,
    );
    mgr.push_at(
        toast::ToastKind::Warning,
        "Tres",
        Duration::from_secs(3),
        t0,
    );
    mgr.push_at(
        toast::ToastKind::Error,
        "Quatro",
        Duration::from_secs(3),
        t0,
    );
    assert_eq!(mgr.len(), 3);

    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let t_show = t0 + Duration::from_millis(200);
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |_| {
                mgr.show_at(&ctx, &palette, t_show);
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
    assert!(!rendered.contains("Um"), "oldest toast should be evicted");
    assert!(rendered.contains("Dois"));
    assert!(rendered.contains("Tres"));
    assert!(rendered.contains("Quatro"));

    // Click the dismiss "×" button on the first visible toast.
    let close_pos = rects
        .iter()
        .find(|(t, _)| t == "\u{00d7}")
        .map(|(_, r)| r.center())
        .unwrap();
    let mut close_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(close_pos),
                egui::Event::PointerButton {
                    pos: close_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: close_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |_| {
            mgr.show_at(&ctx, &palette, t_show);
        },
    );
    close_out.textures_delta.clear();
    assert_eq!(mgr.len(), 2);

    // Advance past duration and verify remaining toasts auto-expire.
    let t_expired = t0 + Duration::from_secs(4);
    let mut exp_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |_| {
            mgr.show_at(&ctx, &palette, t_expired);
        },
    );
    exp_out.textures_delta.clear();
    assert!(mgr.is_empty());
}
