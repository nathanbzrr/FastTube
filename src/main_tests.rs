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

    // Simula player ativo com perfil padrão e troca para Grande: exige reiniciar o player.
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

    // Simula erro de armazenamento (caminho sem diretório pai).
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

    // 1. Sem caminho configurado em teste (default), aplicar preferências atualiza a sessão,
    // registra erro de armazenamento e protege contra escrita no playback_path() real.
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

    // 2. Com caminho isolado injetado: persiste e recarrega sem interferir em buffer ou histórico.
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

    // 3. Nova instância sincroniza automaticamente a partir do caminho isolado injetado.
    let mut reloaded_app = FastTube::default();
    reloaded_app.settings.playback.path = Some(playback_file.clone());
    reloaded_app.ensure_playback_settings_synchronized();
    assert_eq!(reloaded_app.settings.playback.value, custom_prefs);
    assert!(reloaded_app.settings.playback.error.is_none());

    // 4. Arquivo corrompido no caminho injetado cai para padrão (1x / auto) e reporta erro na UI.
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

    // 5. Erro de escrita (caminho inválido) preserva preferência na sessão e expõe playback_settings_error.
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

    // Abre configurações com vídeo ativo: HWND nativo é ocultado, sem pausar/destruir o estado do player.
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

    // Alterna para tela cheia com configurações abertas: menu continua acessível e HWND permanece oculto.
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

    // Fecha a janela de configurações em tela cheia: HWND do vídeo é restaurado imediatamente.
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

    // Fechar o vídeo limpa a seleção, sai da tela cheia e garante que o HWND não fique visível.
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

    // Abre o primeiro vídeo: grava entrada inicial em disco na posição 0.
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

    // Quadros durante os primeiros segundos (< 5s): atualiza em memória sem regravar em disco a cada quadro.
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
    // Disco ainda contém a gravação de t0 (não regravou após apenas 500ms).
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

    // Após >= 5s com avanço >= 1s, grava automaticamente em disco.
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

    // Avança mais um pouco até 62s antes de trocar de vídeo.
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

    // Troca para o vídeo 2: deve persistir imediatamente 62s do vídeo 1 e proteger o vídeo 2
    // contra leituras atrasadas (stale) da posição 62s do vídeo 1.
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

    // Simula o player ainda reportando o path antigo ou o time-pos antigo (62.1s) logo após o loadfile:
    // nenhuma dessas amostras velhas deve contaminar o vídeo 2!
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

    // Agora o vídeo 2 realmente começa em 1.5s e depois fecha em 18.0s.
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

    // Fonte não-YouTube ou ID inválido falha com segurança sem entrar no histórico.
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

    // Vídeo YouTube com posição intermediária (45s de 200s) retoma em 45s.
    app.record_video_in_history(&videos[0], 45_000, Some(200_000))
        .unwrap();
    let resume_ms = app.planned_resume_ms(&videos[0]);
    assert_eq!(resume_ms, Some(45_000));
    app.selected = Some(videos[0].clone());
    app.activate_video_playback(&videos[0], resume_ms, None, now);

    // Enquanto o player carrega (time-pos == None), a posição de retomada 45s não é zerada.
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

    // Se o player iniciar em 0.2s, emite seek explícito para 45.0s.
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

    // Vídeo perto do fim (197s de 200s) não retoma perto do fim; recomeça do início.
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

    // Mesmo abrindo, reproduzindo, limpando ou fechando vídeo, o arquivo corrompido é preservado intacto.
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

    // Agora em um caminho válido, verifica limpeza de histórico e renderização na UI.
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

    // 1. Sem vídeo ativo e painel fechado: nenhuma consulta FFI e diagnóstico permanece desconhecido/inativo.
    assert!(!app.refresh_decoding_diagnostics_with(t0, false, |_| {
        ffi_calls += 1;
        Some("d3d11va".into())
    }));
    assert_eq!(ffi_calls, 0);
    assert!(app.playback.diagnostics.effective_decoding.is_unknown());

    // 2. Com vídeo ativo, mas painel de configurações oculto: nenhuma consulta FFI.
    app.selected = Some(videos[0].clone());
    assert!(!app.refresh_decoding_diagnostics_with(t0, true, |_| {
        ffi_calls += 1;
        Some("d3d11va".into())
    }));
    assert_eq!(ffi_calls, 0);

    // 3. Sem vídeo ativo, mesmo com painel aberto: nenhuma consulta FFI.
    app.ui.settings_open = true;
    assert!(!app.refresh_decoding_diagnostics_with(t0, false, |_| {
        ffi_calls += 1;
        Some("d3d11va".into())
    }));
    assert_eq!(ffi_calls, 0);

    // 4. Painel visível + vídeo ativo: coleta imediatamente na primeira verificação.
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

    // 5. Quadros subsequentes antes de 1s (~250ms, ~900ms): suprime consultas FFI.
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

    // 6. Após >= 1s com painel visível: realiza nova coleta (ex.: mudou para software / AV1).
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

    // 7. Ao fechar o vídeo, o diagnóstico volta imediatamente para desconhecido/inativo.
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

    // Caso 1: Sem vídeo em reprodução -> inativo/desconhecido, sem alegar GPU ativa apesar de hwdec=auto-safe.
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

    // Caso 2: Vídeo ativo, mas hwdec-current ainda ausente/política -> desconhecido, não alega hardware ativo.
    let unconfirmed_diag =
        DecodingDiagnostics::from_raw(Some("auto-safe"), Some("auto-safe"), None, None, None);
    let unconfirmed_display = format_diagnostics_display(&unconfirmed_diag, true);
    assert!(unconfirmed_display.status_line.contains("desconhecida"));
    assert!(unconfirmed_display.effective_line.contains("desconhecido"));
    assert!(unconfirmed_display.metadata_line.is_none());

    // Caso 3: Vídeo ativo com hardware confirmado (d3d11va) + codec e tamanho.
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

    // Caso 4: Vídeo ativo em software (hwdec-current = no).
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

    // Exercita renderização headless da janela de configurações nos estados ativo e inativo.
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

    // 1. Sem caminho configurado em teste (default), aplicar tema atualiza a sessão,
    // registra erro de armazenamento e protege contra escrita no theme_path() real.
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
            },
        }],
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
            },
        }],
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
    };
    assert!(app.complete_channel_request(req_id, &valid_target, Ok(initial_page), &ctx));
    assert!(app.channels.videos.has_more);
    assert!(!app.channels.videos.reached_max);

    // 1. Progressive video pagination ("Carregar mais vídeos") with deduplication & error handling.
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

    // 3. Channel About ("Informações") tab on-demand load and metadata enrichment.
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

    // Render all three tabs in headless UI (Dark and Light themes).
    let base_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    for tab in [ChannelTab::Videos, ChannelTab::Playlists, ChannelTab::About] {
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
    let ctx = egui::Context::default();
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
    app.poll_rating(&ctx);
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
    app.poll_rating(&ctx);
    assert_eq!(
        app.auth.ratings.get("dQw4w9WgXcQ").copied(),
        Some(oauth::VideoRating::Dislike)
    );
    assert_eq!(app.message, "Você não gostou deste vídeo.");

    let (tx, rx) = mpsc::channel();
    app.auth.rating_rx = Some(rx);
    tx.send((None, Ok(("dQw4w9WgXcQ".into(), None)))).unwrap();
    app.poll_rating(&ctx);
    assert!(!app.auth.ratings.contains_key("dQw4w9WgXcQ"));
    assert_eq!(app.message, "Avaliação removida.");

    let (tx, rx) = mpsc::channel();
    app.auth.rating_rx = Some(rx);
    tx.send((
        None,
        Err("Sua sessão Google expirou ou foi revogada. Entre novamente.".into()),
    ))
    .unwrap();
    app.poll_rating(&ctx);
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
    assert_eq!(app.playback.player_menu, None);

    app.handle_player_overlay_action(overlay::OverlayAction::AddBookmark, &overlay_ctx, &ctx);
    assert_eq!(app.library.data.bookmarks.len(), 1);
    assert_eq!(app.library.data.bookmarks[0].time_ms, 42_000);
    assert!(app.library.error.is_none());

    app.handle_player_overlay_action(
        overlay::OverlayAction::SetMenu(Some(overlay::OverlayMenu::Root)),
        &overlay_ctx,
        &ctx,
    );
    app.handle_player_overlay_action(overlay::OverlayAction::OpenAppSettings, &overlay_ctx, &ctx);
    assert_eq!(app.playback.player_menu, None);
    assert!(app.ui.settings_open);
    assert!(!app.playback.window_visible);

    app.ui.settings_open = false;
    app.handle_player_overlay_action(overlay::OverlayAction::ToggleTheater, &overlay_ctx, &ctx);
    assert!(app.playback.theater);

    app.handle_player_overlay_action(overlay::OverlayAction::ToggleFullscreen, &overlay_ctx, &ctx);
    assert!(app.playback.fullscreen);

    // Ativar miniplayer enquanto em tela cheia deve sair da tela cheia automaticamente.
    app.handle_player_overlay_action(overlay::OverlayAction::ToggleMiniPlayer, &overlay_ctx, &ctx);
    assert!(app.playback.mini_player);
    assert!(!app.playback.fullscreen);

    app.playback.player_menu = Some(overlay::OverlayMenu::Quality);
    app.close_video(&ctx);
    assert_eq!(app.playback.player_menu, None);
    assert_eq!(app.playback.last_subtitle_id, None);

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
