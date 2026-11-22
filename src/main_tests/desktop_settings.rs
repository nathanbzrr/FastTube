use super::super::*;
use super::helpers::*;

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
    let (req_id, cancel, sender) = app.feed.home.stream_task.begin_manual();
    app.feed.home.title = Some("Inscrições".into());
    app.cancel_login();
    assert!(cancel.cancelled());
    assert!(!app.feed.home.stream_task.is_running());
    assert!(app.feed.home.title.is_none());
    assert!(!app.auth.authenticated);
    assert!(app.auth.session.is_none());
    assert!(app.message.contains("tentar novamente"));
    assert!(
        sender
            .send((
                req_id,
                HomeFetchMessage::Finished(
                    Some(oauth::TokenSession::test_session("resultado-antigo")),
                    Err("antigo".into()),
                ),
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
    let (req_id, _cancel, sender) = app.feed.home.stream_task.begin_manual();
    assert!(
        sender
            .send((
                req_id,
                HomeFetchMessage::Finished(
                    Some(oauth::TokenSession::test_session("test-token")),
                    Err("API unavailable".into()),
                ),
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

    let (req_id, _cancel, sender) = app.feed.home.stream_task.begin_manual();
    assert!(
        sender
            .send((
                req_id,
                HomeFetchMessage::Finished(None, Err("Login cancelado".into())),
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
    let (req_id, _cancel, sender) = app.feed.home.stream_task.begin_manual();
    sender
        .send((
            req_id,
            HomeFetchMessage::Progress(oauth::SubscriptionProgress {
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
            }),
        ))
        .unwrap();
    sender
        .send((
            req_id,
            HomeFetchMessage::Progress(oauth::SubscriptionProgress {
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
            }),
        ))
        .unwrap();
    sender
        .send((
            req_id,
            HomeFetchMessage::Finished(
                Some(oauth::TokenSession::test_session("test-token")),
                Err("API unavailable".into()),
            ),
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
    let (req_id, _cancel, sender) = app.feed.home.stream_task.begin_manual();

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
        .send((
            req_id,
            HomeFetchMessage::Progress(oauth::SubscriptionProgress {
                items: vec![first_progress],
                channels: Vec::new(),
                completed_channels: 5,
                total_channels: 80,
                reached_limit: false,
            }),
        ))
        .unwrap();
    app.poll_async_tasks(&egui::Context::default());

    assert!(app.message.contains("5 de 80"));
    assert_eq!(app.feed.home.pending_items.len(), 1);
    assert_eq!(app.feed.home.pending_items[0].video.title, "Primeiro lote");
    assert_eq!(app.feed.videos.len(), 1);
    assert_eq!(app.feed.videos[0].title, "Resultado atual");

    sender
        .send((
            req_id,
            HomeFetchMessage::Progress(oauth::SubscriptionProgress {
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
            }),
        ))
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
    let (req_id, _cancel, sender) = app.feed.home.stream_task.begin_manual();
    sender
        .send((
            req_id,
            HomeFetchMessage::Progress(oauth::SubscriptionProgress {
                items: Vec::new(),
                channels: Vec::new(),
                completed_channels: 100,
                total_channels: 100,
                reached_limit: true,
            }),
        ))
        .unwrap();
    sender
        .send((
            req_id,
            HomeFetchMessage::Finished(
                Some(oauth::TokenSession::test_session("test-token")),
                Ok(search::HomeFeed {
                    items: Vec::new(),
                    playlists: Vec::new(),
                    channels: Vec::new(),
                }),
            ),
        ))
        .unwrap();

    app.poll_async_tasks(&egui::Context::default());
    assert!(app.message.contains("Limite de 100 canais"));
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
    app.flush_persistence();
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
        followed_channels: 0,
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
                        egui::vec2(1120.0, 3200.0),
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
        app.followed_channels.path = Some(temp_dir.join("followed-channels.json"));
        app.settings.playback.path = Some(temp_dir.join("playback-settings.txt"));
        app.settings.theme.path = Some(temp_dir.join("theme-settings.txt"));
        app.feed_preferences.path = Some(temp_dir.join("feed-prefs.json"));
        app.settings.buffer.path = Some(temp_dir.join("buffer-settings.txt"));
        app.settings.prefs.path = Some(temp_dir.join("app-preferences.json"));
    };

    let mut exporter = FastTube::default();
    configure_paths(&mut exporter);
    exporter
        .followed_channels
        .data
        .channels
        .push(followed_channels::FollowedChannel {
            channel_id: "UC_x5XG1OV2P6uZZ5FSM9Ttw".to_owned(),
            title: "Google Developers".to_owned(),
            handle: Some("@GoogleDevelopers".to_owned()),
            url: "https://www.youtube.com/channel/UC_x5XG1OV2P6uZZ5FSM9Ttw".to_owned(),
            avatar_url: None,
            followed_at_ms: 1_700_000_000_000,
            last_seen_video_id: None,
        });
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
    assert_eq!(
        importer.backup.preview.as_ref().unwrap().followed_channels,
        1
    );
    importer.backup.import_mode = BackupImportMode::Replace;
    importer.import_user_data();

    assert_eq!(importer.settings.prefs.value, exporter.settings.prefs.value);
    assert_eq!(
        importer.followed_channels.data,
        exporter.followed_channels.data
    );
    let persisted = settings::load_app_preferences(&temp_dir.join("app-preferences.json")).unwrap();
    assert_eq!(persisted, exporter.settings.prefs.value);
    let persisted_followed =
        followed_channels::load(&temp_dir.join("followed-channels.json")).unwrap();
    assert_eq!(persisted_followed, exporter.followed_channels.data);

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
    assert!(search_hit.contains("Áudio, legendas, estilo e tradução automática"));
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
        followed_channels: 2,
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
            overlay_auto_hide_active: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(80))
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            overlay_auto_hide_active: true,
            ..Default::default()
        }),
        Some(Duration::from_millis(80))
    );

    assert_eq!(
        compute_repaint_interval(RepaintScheduleInput {
            next_toast_repaint: Some(Duration::from_millis(2_400)),
            ..Default::default()
        }),
        Some(Duration::from_millis(2_400))
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
        None
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

#[test]
fn command_palette_ctrl_shortcuts_filter_and_execute_actions() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let sample_video = Video {
        title: "Rust Async Internals".into(),
        channel: "Ferris Talks".into(),
        duration: "12:34".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
    };
    app.feed.videos = vec![sample_video.clone()];
    app.queue.data.add(sample_video.clone()).unwrap();
    app.library.data.watch_later.push(library::VideoRecord {
        video: sample_video.clone(),
    });
    app.library.data.bookmarks.push(library::Bookmark {
        video_id: "dQw4w9WgXcQ".into(),
        title: sample_video.title.clone(),
        time_ms: 95_000,
        note: "Pinpoint Async".into(),
    });
    app.feed
        .home
        .subscription_channels
        .push(search::HomeChannel {
            channel_id: "UC_x5XG1OV2P6uZZ5FSM9Ttw".into(),
            title: "Google Developers".into(),
            avatar_url: None,
        });
    app.record_video_in_history(&sample_video, 30_000, Some(754_000))
        .unwrap();

    // Ctrl+K opens palette, Ctrl+P closes it, Ctrl+K re-opens it.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let ctrl_key = |key: egui::Key| egui::RawInput {
        screen_rect: Some(screen),
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
        ..Default::default()
    };

    let mut toggled = false;
    ctx.run_ui(ctrl_key(egui::Key::K), |ui| {
        toggled = app.handle_command_palette_global_shortcut(ui.ctx());
    })
    .textures_delta
    .clear();
    assert!(toggled);
    assert!(app.ui.command_palette.open);

    ctx.run_ui(ctrl_key(egui::Key::P), |ui| {
        toggled = app.handle_command_palette_global_shortcut(ui.ctx());
    })
    .textures_delta
    .clear();
    assert!(toggled);
    assert!(!app.ui.command_palette.open);

    ctx.run_ui(ctrl_key(egui::Key::K), |ui| {
        toggled = app.handle_command_palette_global_shortcut(ui.ctx());
    })
    .textures_delta
    .clear();
    assert!(toggled);
    assert!(app.ui.command_palette.open);

    // Filter to SponsorBlock settings tab and execute via Enter key headlessly.
    app.ui.command_palette.query = "sponsorblock".into();
    let mut rendered = String::new();
    for step in 0..3 {
        let events = if step == 2 {
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]
        } else {
            Vec::new()
        };
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.command_palette_ui(&c);
            },
        );
        if step == 1 {
            rendered = collect_rendered_text(&out.shapes).join("\n");
        }
        out.textures_delta.clear();
    }
    assert!(
        rendered.contains("SponsorBlock"),
        "palette missing match:\n{rendered}"
    );
    assert!(!app.ui.command_palette.open);
    assert!(app.ui.settings_open);
    assert_eq!(
        app.settings.active_tab,
        app_state::SettingsTab::SponsorBlock
    );

    // Quick actions & bookmark jump-to-timestamp when unopened vs already playing.
    app.execute_palette_action(command_palette::PaletteAction::ToggleTheme, &ctx);
    assert_eq!(app.settings.theme.value, Theme::Light);

    app.execute_palette_action(
        command_palette::PaletteAction::OpenBookmark {
            bookmark: app.library.data.bookmarks[0].clone(),
        },
        &ctx,
    );
    assert!(!app.ui.settings_open);
    assert_eq!(app.playback.pending_open_seek_ms, Some(95_000));
    assert_eq!(
        app.playback
            .pending_open_video
            .as_ref()
            .map(|v| v.url.as_str()),
        Some(sample_video.url.as_str())
    );

    // When the same video is already selected, opening a bookmark seeks immediately.
    app.selected = Some(sample_video.clone());
    app.playback.pending_open_video = None;
    app.playback.pending_open_seek_ms = None;
    app.playback.active = Some(ActivePlayback {
        video_id: Some("dQw4w9WgXcQ".into()),
        url: sample_video.url.clone(),
        title: sample_video.title.clone(),
        channel: sample_video.channel.clone(),
        position_ms: 10_000,
        duration_ms: Some(754_000),
        resume_target_ms: None,
        resume_seek_pending: false,
        explicit_seek_pending: false,
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
    app.execute_palette_action(
        command_palette::PaletteAction::OpenBookmark {
            bookmark: app.library.data.bookmarks[0].clone(),
        },
        &ctx,
    );
    assert_eq!(
        app.playback.active.as_ref().map(|a| a.position_ms),
        Some(95_000)
    );

    app.execute_palette_action(command_palette::PaletteAction::ClearQueue, &ctx);
    assert!(app.queue.data.is_empty());

    app.execute_palette_action(command_palette::PaletteAction::OpenShortcuts, &ctx);
    assert!(app.ui.shortcuts_open);

    // Backdrop click outside modal closes the palette.
    app.ui.command_palette.open_palette();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(egui::pos2(10.0, 10.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(10.0, 10.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: egui::pos2(10.0, 10.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.command_palette_ui(&c);
        },
    );
    out.textures_delta.clear();
    assert!(!app.ui.command_palette.open);
}

#[test]
fn navigation_back_forward_stack_records_and_renders_header_buttons() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    // Baseline: Home
    app.sync_navigation_history();
    assert!(!app.ui.can_go_back());
    assert!(!app.ui.can_go_forward());

    // Transition to Library
    app.ui.browse_section = BrowseSection::Library;
    app.sync_navigation_history();
    assert!(app.ui.can_go_back());
    assert!(!app.ui.can_go_forward());

    // Transition to Settings(Privacy)
    app.ui.settings_open = true;
    app.settings.active_tab = app_state::SettingsTab::Privacy;
    app.sync_navigation_history();
    assert_eq!(app.ui.nav_back_stack.len(), 2);

    // Back -> Library, Back -> Home, Forward -> Library
    assert!(app.navigate_back(&ctx));
    app.sync_navigation_history();
    assert!(!app.ui.settings_open);
    assert_eq!(app.ui.browse_section, BrowseSection::Library);
    assert!(app.ui.can_go_forward());

    assert!(app.navigate_back(&ctx));
    app.sync_navigation_history();
    assert_eq!(app.ui.browse_section, BrowseSection::Home);
    assert_eq!(app.ui.nav_forward_stack.len(), 2);

    assert!(app.navigate_forward(&ctx));
    app.sync_navigation_history();
    assert_eq!(app.ui.browse_section, BrowseSection::Library);

    // Visiting a new route clears forward history.
    app.ui.browse_section = BrowseSection::Queue;
    app.sync_navigation_history();
    assert!(!app.ui.can_go_forward());

    // Header UI renders ← and → buttons and clicking ← navigates back to Library.
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 220.0));
    let mut header_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    let rects = collect_rendered_text_rects(&header_out.shapes);
    header_out.textures_delta.clear();
    let back_pos = rects
        .iter()
        .find(|(text, _)| text == "\u{2190}")
        .expect("header back button")
        .1
        .center();

    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(back_pos),
                egui::Event::PointerButton {
                    pos: back_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: back_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    click_out.textures_delta.clear();
    app.sync_navigation_history();
    assert_eq!(app.ui.browse_section, BrowseSection::Library);
    assert!(app.ui.can_go_forward());
}

#[test]
fn clipboard_quick_open_pill_opens_target_and_dismisses_cleanly() {
    assert_eq!(
        decode_utf16_clipboard_words(&[b'h' as u16, b'i' as u16, 0, b'x' as u16]),
        "hi"
    );
    assert_eq!(
        decode_utf16_clipboard_words(&[b'o' as u16, b'k' as u16]),
        "ok"
    );

    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.set_clipboard_preview_text(Some("https://youtu.be/dQw4w9WgXcQ?t=1m30s"));
    assert!(app.ui.clipboard_last_checked_at.is_some());
    assert!(app.ui.clipboard_youtube_target.is_some());

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 220.0));
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
    let rects = collect_rendered_text_rects(&out.shapes);
    let joined = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();
    assert!(
        joined.contains("Link copiado: Vídeo dQw4w9WgXcQ (1:30)"),
        "{joined}"
    );

    let open_pos = rects
        .iter()
        .find(|(text, _)| text == "Abrir")
        .expect("Abrir button in clipboard pill")
        .1
        .center();
    let mut click_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(open_pos),
                egui::Event::PointerButton {
                    pos: open_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: open_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    click_out.textures_delta.clear();
    assert_eq!(app.playback.pending_open_seek_ms, Some(90_000));
    assert_eq!(
        app.playback
            .pending_open_video
            .as_ref()
            .map(|v| v.url.as_str()),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );
    assert!(app.ui.clipboard_youtube_target.is_none());

    // Same clipboard URL stays dismissed until clipboard changes.
    app.set_clipboard_preview_text(Some("https://youtu.be/dQw4w9WgXcQ?t=1m30s"));
    assert!(app.ui.clipboard_youtube_target.is_none());

    // A new clipboard URL triggers pill again and can be dismissed via ✕.
    app.set_clipboard_preview_text(Some("https://www.youtube.com/@rustlang"));
    assert!(app.ui.clipboard_youtube_target.is_some());
    let mut out2 = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    let rects2 = collect_rendered_text_rects(&out2.shapes);
    out2.textures_delta.clear();
    let dismiss_pos = rects2
        .iter()
        .find(|(text, _)| text == "\u{2715}")
        .expect("Dismiss button in clipboard pill")
        .1
        .center();
    let mut dismiss_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(dismiss_pos),
                egui::Event::PointerButton {
                    pos: dismiss_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: dismiss_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    dismiss_out.textures_delta.clear();
    assert!(app.ui.clipboard_youtube_target.is_none());
}

#[test]
fn v030_app_preferences_defaults_and_opt_in_community_apis() {
    let prefs = settings::AppPreferences::default();
    assert!(!prefs.ryd_enabled);
    assert!(!prefs.dearrow_enabled);
    assert!(!prefs.auto_mini_player);
    assert_eq!(prefs.language, i18n::UiLanguage::PtBr);
    assert_eq!(prefs.download_dir_override, None);

    let cancel = process::Cancellation::default();
    let outcome = community_apis::fetch_community_extras("dQw4w9WgXcQ", false, false, &cancel);
    assert_eq!(outcome.video_id, "dQw4w9WgXcQ");
    assert!(outcome.ryd.is_none());
    assert!(outcome.dearrow.is_none());
}

#[test]
fn v030_ytdlp_updater_github_release_parser() {
    let payload = br#"{
        "tag_name": "2026.03.13",
        "assets": [
            {
                "name": "yt-dlp.exe",
                "browser_download_url": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.03.13/yt-dlp.exe"
            }
        ]
    }"#;
    let parsed = updater::parse_github_release_json(payload).unwrap();
    assert_eq!(parsed.0, "2026.03.13");
    assert!(parsed.1.ends_with("/yt-dlp.exe"));

    let missing_asset = br#"{"tag_name": "2026.03.13", "assets": []}"#;
    assert!(updater::parse_github_release_json(missing_asset).is_err());
}

#[test]
fn tray_and_updater_preferences_defaults_serialization_and_reset() {
    let defaults = settings::AppPreferences::default();
    assert!(!defaults.minimize_to_tray);
    assert!(!defaults.close_to_tray);
    assert!(!defaults.start_minimized_to_tray);
    assert!(defaults.auto_check_updates);
    assert_eq!(defaults.skipped_app_update_version, None);

    let json = serde_json::json!({
        "minimize_to_tray": true,
        "close_to_tray": true,
        "start_minimized_to_tray": true,
        "auto_check_updates": false,
        "skipped_app_update_version": "  0.4.0  "
    });
    let parsed: settings::AppPreferences =
        serde_json::from_value(json).expect("deserializes tray/updater preferences");
    let normalized = parsed.normalized();
    assert!(normalized.minimize_to_tray);
    assert!(normalized.close_to_tray);
    assert!(normalized.start_minimized_to_tray);
    assert!(!normalized.auto_check_updates);
    assert_eq!(
        normalized.skipped_app_update_version.as_deref(),
        Some("0.4.0")
    );

    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    app.settings.prefs.value = normalized;
    app.reset_settings_tab_defaults(app_state::SettingsTab::General, &ctx);
    assert!(!app.settings.prefs.value.minimize_to_tray);
    assert!(!app.settings.prefs.value.close_to_tray);
    assert!(!app.settings.prefs.value.start_minimized_to_tray);
    assert!(app.settings.prefs.value.auto_check_updates);
    assert_eq!(app.settings.prefs.value.skipped_app_update_version, None);
}

#[test]
fn tray_hidden_flag_suppresses_video_window_and_restores() {
    let mut app = FastTube::default();
    let ctx = egui::Context::default();
    app.selected = Some(Video {
        title: "Sample".into(),
        channel: "Channel".into(),
        duration: "1:00".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
    });
    assert!(app.should_show_video_window());

    app.desktop.hidden_to_tray = true;
    assert!(!app.should_show_video_window());

    app.show_from_tray(&ctx);
    assert!(!app.desktop.hidden_to_tray);
    assert!(app.should_show_video_window());
}

#[test]
fn settings_general_renders_tray_autostart_and_github_updater_ui_and_header_banner() {
    let mut app = FastTube::default();
    app.settings.active_tab = app_state::SettingsTab::General;
    app.desktop
        .autostart
        .set_state_for_test(desktop::AutostartUiState::Enabled { hidden: true });
    app.settings.app_update_check = Some(updater::AppUpdateCheck {
        current_version: "0.3.0".into(),
        latest_version: "0.4.0".into(),
        tag_name: "v0.4.0".into(),
        release_url: "https://github.com/nathanbzrr/FastTube/releases/tag/v0.4.0".into(),
        release_notes: "- Tray support\n- Auto-updater".into(),
        asset_name: "FastTube-Setup-0.4.0-x64.exe".into(),
        download_url: "https://github.com/nathanbzrr/FastTube/releases/download/v0.4.0/FastTube-Setup-0.4.0-x64.exe".into(),
        checksums_url: Some("https://github.com/nathanbzrr/FastTube/releases/download/v0.4.0/SHA256SUMS.txt".into()),
        install_kind: updater::AppInstallKind::Installer,
        update_available: true,
    });

    let ctx = egui::Context::default();
    let run_general = |app: &mut FastTube| -> String {
        let mut rendered = String::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1120.0, 3200.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let c = ui.ctx().clone();
                    let palette = app.palette();
                    app.app_update_banner_pill_ui(ui, &c, palette);
                    app.settings_page_ui(ui, &c);
                },
            );
            rendered = collect_rendered_text(&out.shapes).join("\n");
            out.textures_delta.clear();
        }
        rendered
    };

    let pt_text = run_general(&mut app);
    assert!(pt_text.contains("Nova versão do FastTube disponível: v0.4.0"));
    assert!(pt_text.contains("Sistema, energia e bandeja"));
    assert!(pt_text.contains("Minimizar para a bandeja do sistema"));
    assert!(pt_text.contains("Fechar janela para a bandeja"));
    assert!(pt_text.contains("Iniciar com o Windows"));
    assert!(pt_text.contains("Ativo (bandeja)"));
    assert!(pt_text.contains("Iniciar minimizado na bandeja ao entrar no Windows"));
    assert!(pt_text.contains("Atualizador automático (GitHub Releases)"));
    assert!(pt_text.contains("Baixar e instalar"));

    // Skipping the version suppresses active_app_update_available and header banner.
    app.settings.prefs.value.skipped_app_update_version = Some("0.4.0".into());
    assert!(app.active_app_update_available().is_none());
    let skipped_text = run_general(&mut app);
    assert!(!skipped_text.contains("Nova versão do FastTube disponível: v0.4.0"));
    assert!(skipped_text.contains("Aviso da versão v0.4.0 silenciado."));

    // Switching to English renders localized labels cleanly.
    app.settings.prefs.value.skipped_app_update_version = None;
    app.settings.prefs.value.language = i18n::UiLanguage::En;
    let en_text = run_general(&mut app);
    assert!(en_text.contains("New FastTube update available: v0.4.0"));
    assert!(en_text.contains("System, power & tray"));
    assert!(en_text.contains("Auto-updater (GitHub Releases)"));
    assert!(en_text.contains("Download & install"));
}

#[test]
fn bilingual_english_surfaces_and_followed_backup_modal_render_cleanly() {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 1100.0));
    let mut app = FastTube::default();

    // 1) Backup import modal renders the followed-channels badge in PT-BR.
    app.ui.settings_open = true;
    app.backup.confirm_import = true;
    app.backup.preview = Some(BackupPreview {
        path: PathBuf::from("fasttube-backup.json"),
        library_items: 4,
        favorites: 2,
        watch_later: 1,
        collections: 1,
        bookmarks: 3,
        followed_channels: 2,
        queue_items: 2,
        history_entries: 5,
        merge_added: 3,
        merge_conflicts: 1,
        merge_skipped: 0,
        history_progress_updates: 2,
    });
    let mut modal_text = String::new();
    for _ in 0..2 {
        let mut modal_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.backup_import_modal_ui(&c);
            },
        );
        modal_text = collect_rendered_text(&modal_out.shapes).join("\n");
        modal_out.textures_delta.clear();
    }
    assert!(
        modal_text.contains("2 canais seguidos"),
        "expected followed channels badge in backup modal, got:\n{modal_text}"
    );

    // 2) Switch UI language to English and verify Command Palette, Library, History & Downloads.
    app.ui.settings_open = false;
    app.backup.confirm_import = false;
    app.settings.prefs.value.language = i18n::UiLanguage::En;

    let palette_items = app.build_command_palette_items();
    assert!(
        palette_items
            .iter()
            .any(|item| item.category.label(false) == "Quick action"),
        "expected English category label in command palette items"
    );

    app.ui.command_palette.open_palette();
    app.ui.command_palette.query.clear();
    let mut cp_text = String::new();
    for _ in 0..2 {
        let mut cp_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.command_palette_ui(&c);
            },
        );
        cp_text = collect_rendered_text(&cp_out.shapes).join("\n");
        cp_out.textures_delta.clear();
    }
    assert!(
        cp_text.contains("Quick action") || cp_text.contains("Settings"),
        "expected English Command Palette labels, got:\n{cp_text}"
    );
    app.ui.command_palette.open = false;

    let videos = three_youtube_videos();
    app.library.data.toggle_watch_later(videos[0].clone());
    app.library.data.toggle_favorite(videos[1].clone());
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
    assert!(
        lib_text.contains("You")
            && lib_text.contains("1 in Watch later")
            && lib_text.contains("1 favorites"),
        "expected English library header, got:\n{lib_text}"
    );

    let mut hist_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "en_hist_test", false, None, None);
        },
    );
    let hist_text = collect_rendered_text(&hist_out.shapes).join("\n");
    hist_out.textures_delta.clear();
    assert!(
        hist_text.contains("Watch history"),
        "expected English history heading, got:\n{hist_text}"
    );

    let format = downloads::DownloadFormat::ALL[0];
    app.downloads.manager.items.push(downloads::DownloadItem {
        id: 1,
        video: videos[0].clone(),
        format,
        output_dir: std::env::temp_dir(),
        status: downloads::DownloadStatus::Completed {
            output_path: Some(std::env::temp_dir().join("demo.mp4")),
        },
    });
    app.downloads.manager.items.push(downloads::DownloadItem {
        id: 2,
        video: videos[1].clone(),
        format,
        output_dir: std::env::temp_dir(),
        status: downloads::DownloadStatus::Failed("network error".into()),
    });
    let mut dl_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.downloads_section_ui(ui, &c);
        },
    );
    let dl_text = collect_rendered_text(&dl_out.shapes).join("\n");
    dl_out.textures_delta.clear();
    assert!(
        dl_text.contains("Open folder")
            && dl_text.contains("Show in folder")
            && dl_text.contains("↻ Retry"),
        "expected English download actions, got:\n{dl_text}"
    );
}

#[test]
fn header_brand_displays_fasttube_and_dynamic_language_region_badge() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 160.0));

    app.settings.prefs.value.language = i18n::UiLanguage::PtBr;
    let mut pt_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    let pt_tokens = collect_rendered_text(&pt_out.shapes);
    pt_out.textures_delta.clear();
    assert!(pt_tokens.iter().any(|t| t == "FastTube"), "{pt_tokens:?}");
    assert!(pt_tokens.iter().any(|t| t == "BR"), "{pt_tokens:?}");
    assert!(!pt_tokens.iter().any(|t| t == "YouTube"), "{pt_tokens:?}");

    app.settings.prefs.value.language = i18n::UiLanguage::En;
    let mut en_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        },
    );
    let en_tokens = collect_rendered_text(&en_out.shapes);
    en_out.textures_delta.clear();
    assert!(en_tokens.iter().any(|t| t == "FastTube"), "{en_tokens:?}");
    assert!(en_tokens.iter().any(|t| t == "US"), "{en_tokens:?}");
    assert!(!en_tokens.iter().any(|t| t == "BR"), "{en_tokens:?}");
    assert!(!en_tokens.iter().any(|t| t == "YouTube"), "{en_tokens:?}");
}

#[test]
fn sidebar_panels_are_non_resizable_and_icons_align_with_hamburger() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.feed.videos = sample_videos();

    // Warm up one frame so egui populates widget hit-test rects before checking hover at (36.0, 28.0).
    let compact_screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 768.0));
    let mut warmup_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(compact_screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let frame = egui::Frame::new()
                .fill(c.global_style().visuals.panel_fill)
                .inner_margin(egui::Margin::ZERO);
            egui::Panel::top("header")
                .default_size(56.0)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    app.header_ui(ui, &c);
                });
            egui::Panel::left("compact_navigation")
                .resizable(false)
                .show_separator_line(false)
                .exact_size(COMPACT_NAVIGATION_WIDTH)
                .frame(frame)
                .show(ui, |ui| {
                    app.compact_navigation_ui(ui, &c);
                });
            egui::CentralPanel::default().show(ui, |_| {});
        },
    );
    warmup_out.textures_delta.clear();

    // Hover the hamburger center at (36.0, 28.0) so it paints its hover circle, then inspect centers.
    let mut compact_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(compact_screen),
            events: vec![egui::Event::PointerMoved(egui::pos2(36.0, 28.0))],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let frame = egui::Frame::new()
                .fill(c.global_style().visuals.panel_fill)
                .inner_margin(egui::Margin::ZERO);
            egui::Panel::top("header")
                .default_size(56.0)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    app.header_ui(ui, &c);
                });
            egui::Panel::left("compact_navigation")
                .resizable(false)
                .show_separator_line(false)
                .exact_size(COMPACT_NAVIGATION_WIDTH)
                .frame(frame)
                .show(ui, |ui| {
                    app.compact_navigation_ui(ui, &c);
                });
            egui::CentralPanel::default().show(ui, |_| {});
        },
    );
    compact_out.textures_delta.clear();
    let hamburger_center = compact_out
        .shapes
        .iter()
        .find_map(|clipped| find_circle_center_recursive(&clipped.shape, 18.0, 72.0))
        .expect("hamburger hover circle");
    let compact_text_rects = collect_rendered_text_rects(&compact_out.shapes);
    let home_label_rect = compact_text_rects
        .iter()
        .find(|(text, rect)| text == "In\u{00ed}cio" && rect.left() < COMPACT_NAVIGATION_WIDTH)
        .expect("compact sidebar Home label")
        .1;
    assert!(
        (hamburger_center.x - 36.0).abs() <= 0.5,
        "expected hamburger center at x=36, got {}",
        hamburger_center.x
    );
    assert!(
        (home_label_rect.center().x - 36.0).abs() <= 0.5,
        "expected compact rail item centered at x=36, got {}",
        home_label_rect.center().x
    );
    assert!(
        (hamburger_center.x - home_label_rect.center().x).abs() <= 0.5,
        "hamburger x={} and compact rail item x={} should match",
        hamburger_center.x,
        home_label_rect.center().x
    );

    // Moving the pointer over the compact sidebar right edge must not trigger a resize cursor.
    let mut compact_edge_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(compact_screen),
            events: vec![egui::Event::PointerMoved(egui::pos2(
                COMPACT_NAVIGATION_WIDTH,
                200.0,
            ))],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let frame = egui::Frame::new()
                .fill(c.global_style().visuals.panel_fill)
                .inner_margin(egui::Margin::ZERO);
            egui::Panel::top("header")
                .default_size(56.0)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    app.header_ui(ui, &c);
                });
            egui::Panel::left("compact_navigation")
                .resizable(false)
                .show_separator_line(false)
                .exact_size(COMPACT_NAVIGATION_WIDTH)
                .frame(frame)
                .show(ui, |ui| {
                    app.compact_navigation_ui(ui, &c);
                });
            egui::CentralPanel::default().show(ui, |_| {});
        },
    );
    compact_edge_out.textures_delta.clear();
    assert!(
        !matches!(
            compact_edge_out.platform_output.cursor_icon,
            egui::CursorIcon::ResizeHorizontal
                | egui::CursorIcon::ResizeEast
                | egui::CursorIcon::ResizeWest
        ),
        "compact sidebar edge should not show resize cursor: {:?}",
        compact_edge_out.platform_output.cursor_icon
    );

    // Expanded navigation sidebar at wide viewport must also be non-resizable and keep x=36 alignment.
    let wide_screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 768.0));
    let nav_width = layout::compute_youtube_sidebar_width(wide_screen.width());
    let mut wide_edge_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(wide_screen),
            events: vec![egui::Event::PointerMoved(egui::pos2(nav_width, 200.0))],
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let frame = egui::Frame::new()
                .fill(c.global_style().visuals.panel_fill)
                .inner_margin(egui::Margin::ZERO);
            egui::Panel::top("header")
                .default_size(56.0)
                .show_separator_line(false)
                .frame(frame)
                .show(ui, |ui| {
                    app.header_ui(ui, &c);
                });
            egui::Panel::left("navigation")
                .resizable(false)
                .show_separator_line(false)
                .exact_size(nav_width)
                .frame(frame)
                .show(ui, |ui| {
                    app.navigation_ui(ui, &c);
                });
            egui::CentralPanel::default().show(ui, |_| {});
        },
    );
    let expanded_texts = collect_rendered_text_rects(&wide_edge_out.shapes);
    wide_edge_out.textures_delta.clear();
    assert!(
        !matches!(
            wide_edge_out.platform_output.cursor_icon,
            egui::CursorIcon::ResizeHorizontal
                | egui::CursorIcon::ResizeEast
                | egui::CursorIcon::ResizeWest
        ),
        "expanded sidebar edge should not show resize cursor: {:?}",
        wide_edge_out.platform_output.cursor_icon
    );
    let expanded_home = expanded_texts
        .iter()
        .find(|(text, _)| text == "In\u{00ed}cio")
        .expect("expanded sidebar Home text")
        .1;
    assert!(
        (expanded_home.left() - 60.0).abs() <= 0.5,
        "expanded sidebar label should start at x=60 alongside x=36 centered icon, got {}",
        expanded_home.left()
    );
}

#[test]
fn frameless_window_controls_and_resize_directions() {
    use egui::viewport::ResizeDirection;

    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(2.0, 2.0), 4.0, 10.0),
        Some(ResizeDirection::NorthWest)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(1198.0, 2.0), 4.0, 10.0),
        Some(ResizeDirection::NorthEast)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(2.0, 798.0), 4.0, 10.0),
        Some(ResizeDirection::SouthWest)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(1198.0, 798.0), 4.0, 10.0),
        Some(ResizeDirection::SouthEast)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(600.0, 2.0), 4.0, 10.0),
        Some(ResizeDirection::North)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(600.0, 798.0), 4.0, 10.0),
        Some(ResizeDirection::South)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(2.0, 400.0), 4.0, 10.0),
        Some(ResizeDirection::West)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(1198.0, 400.0), 4.0, 10.0),
        Some(ResizeDirection::East)
    );
    assert_eq!(
        FastTube::viewport_resize_direction(rect, egui::pos2(600.0, 400.0), 4.0, 10.0),
        None
    );

    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    for width in [480.0_f32, 760.0, 1360.0] {
        let size = egui::vec2(width, 160.0);
        let mut init = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.header_ui(ui, &c);
            },
        );
        init.textures_delta.clear();

        let min_out = headless_pointer_click(&ctx, size, egui::pos2(width - 100.0, 28.0), |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        });
        let min_cmds = &min_out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(
            min_cmds.contains(&egui::ViewportCommand::Minimized(true)),
            "width={width}: expected Minimized(true), got {min_cmds:?}"
        );

        let max_out = headless_pointer_click(&ctx, size, egui::pos2(width - 62.0, 28.0), |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        });
        let max_cmds = &max_out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(
            max_cmds.contains(&egui::ViewportCommand::Maximized(true)),
            "width={width}: expected Maximized(true), got {max_cmds:?}"
        );

        let close_out = headless_pointer_click(&ctx, size, egui::pos2(width - 24.0, 28.0), |ui| {
            let c = ui.ctx().clone();
            app.header_ui(ui, &c);
        });
        let close_cmds = &close_out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(
            close_cmds.contains(&egui::ViewportCommand::Close),
            "width={width}: expected Close, got {close_cmds:?}"
        );
    }

    // When maximized, the middle window control toggles back to Maximized(false).
    let size = egui::vec2(1440.0, 160.0);
    let mut max_warmup_raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    };
    if let Some(vp) = max_warmup_raw.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(true);
    }
    let mut max_warmup_out = ctx.run_ui(max_warmup_raw, |ui| {
        let c = ui.ctx().clone();
        app.header_ui(ui, &c);
    });
    max_warmup_out.textures_delta.clear();

    let mut click_maximized = |pos: egui::Pos2| {
        for pressed in [true, false] {
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            };
            if let Some(vp) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
                vp.maximized = Some(true);
            }
            let mut out = ctx.run_ui(raw, |ui| {
                let c = ui.ctx().clone();
                app.header_ui(ui, &c);
            });
            out.textures_delta.clear();
            if !pressed {
                return out;
            }
        }
        unreachable!()
    };
    let restore_out = click_maximized(egui::pos2(1440.0 - 62.0, 28.0));
    let restore_cmds = &restore_out.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(
        restore_cmds.contains(&egui::ViewportCommand::Maximized(false)),
        "expected Maximized(false) when already maximized, got {restore_cmds:?}"
    );

    // Double-clicking empty header space toggles maximize (advance time well past prior restore click).
    let mut run_header_at = |time: f64, events: Vec<egui::Event>| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.header_ui(ui, &c);
            },
        );
        out.textures_delta.clear();
        out
    };
    let empty_pos = egui::pos2(1005.0, 28.0);
    let _ = run_header_at(10.0, vec![egui::Event::PointerMoved(empty_pos)]);
    for (time, pressed) in [(10.02, true), (10.04, false), (10.08, true)] {
        let _ = run_header_at(
            time,
            vec![
                egui::Event::PointerMoved(empty_pos),
                egui::Event::PointerButton {
                    pos: empty_pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }
    let dbl_out = run_header_at(
        10.10,
        vec![
            egui::Event::PointerMoved(empty_pos),
            egui::Event::PointerButton {
                pos: empty_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    let dbl_cmds = &dbl_out.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(
        dbl_cmds.contains(&egui::ViewportCommand::Maximized(true)),
        "double-clicking empty header should emit Maximized(true), got {dbl_cmds:?}"
    );

    // Dragging empty header space starts native window move, while dragging a header button does not.
    let _ = run_header_at(
        11.0,
        vec![
            egui::Event::PointerMoved(empty_pos),
            egui::Event::PointerButton {
                pos: empty_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    let drag_empty_out = run_header_at(
        11.05,
        vec![egui::Event::PointerMoved(egui::pos2(
            empty_pos.x + 20.0,
            empty_pos.y,
        ))],
    );
    let drag_empty_cmds = &drag_empty_out.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(
        drag_empty_cmds.contains(&egui::ViewportCommand::StartDrag),
        "dragging empty header space should emit StartDrag, got {drag_empty_cmds:?}"
    );
    let _ = run_header_at(
        11.08,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(empty_pos.x + 20.0, empty_pos.y),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
    );

    let close_pos = egui::pos2(1440.0 - 24.0, 28.0);
    let _ = run_header_at(11.98, vec![egui::Event::PointerMoved(close_pos)]);
    let _ = run_header_at(
        12.0,
        vec![
            egui::Event::PointerMoved(close_pos),
            egui::Event::PointerButton {
                pos: close_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
    );
    let drag_button_out = run_header_at(
        12.05,
        vec![egui::Event::PointerMoved(egui::pos2(
            close_pos.x - 20.0,
            close_pos.y,
        ))],
    );
    let drag_button_cmds = &drag_button_out.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(
        !drag_button_cmds.contains(&egui::ViewportCommand::StartDrag),
        "dragging from an interactive header button must not emit StartDrag, got {drag_button_cmds:?}"
    );
    let _ = run_header_at(
        12.08,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(close_pos.x - 20.0, close_pos.y),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
    );
}

#[test]
fn middle_click_autoscroll_pans_scroll_area_and_exits_cleanly() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0));
    let mut sim_time = 1.0_f64;

    let mut step = |app: &mut FastTube, events: Vec<egui::Event>| {
        sim_time += 1.0 / 60.0;
        let mut clicked_button = false;
        let mut scroll_offset = 0.0_f32;
        let mut card_rect = egui::Rect::NOTHING;
        let mut raw = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(sim_time),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        app.preprocess_raw_input(&mut raw);
        let mut out = ctx.run_ui(raw, |ui| {
            let c = ui.ctx().clone();
            app.scrolling.apply(&c);
            app.apply_middle_autoscroll(&c);
            egui::Panel::top("test_autoscroll_hdr")
                .default_size(56.0)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(screen.width(), 56.0));
                });
            egui::CentralPanel::default().show(ui, |ui| {
                let sa = egui::ScrollArea::vertical()
                    .id_salt("test_autoscroll_sa")
                    .show(ui, |ui| {
                        let resp = ui.button("Card 0");
                        card_rect = resp.rect;
                        if resp.clicked() {
                            clicked_button = true;
                        }
                        for _ in 0..40 {
                            ui.allocate_exact_size(egui::vec2(600.0, 48.0), egui::Sense::hover());
                        }
                    });
                scroll_offset = sa.state.offset.y;
            });
            app.paint_middle_autoscroll_overlay(&c);
        });
        out.textures_delta.clear();
        (out, scroll_offset, card_rect, clicked_button)
    };

    // Warm up layout.
    let anchor = egui::pos2(400.0, 220.0);
    let (_, _, card_rect, _) = step(&mut app, vec![egui::Event::PointerMoved(anchor)]);

    // Middle-click inside header (< 56px) must not activate autoscroll.
    let _ = step(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(400.0, 24.0),
            button: egui::PointerButton::Middle,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let _ = step(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(400.0, 24.0),
            button: egui::PointerButton::Middle,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.autoscroll.is_active());

    // Click-toggle autoscroll: press and release inside deadzone keeps panning active.
    let (_, scroll_offset, _, _) = step(
        &mut app,
        vec![
            egui::Event::PointerMoved(anchor),
            egui::Event::PointerButton {
                pos: anchor,
                button: egui::PointerButton::Middle,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: anchor,
                button: egui::PointerButton::Middle,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(app.autoscroll.is_active());
    assert_eq!(scroll_offset, 0.0);

    // Moving far downward (even near left edge outside inner content) pans vertically downward.
    let pan_pos = egui::pos2(18.0, anchor.y + 180.0);
    let _ = step(&mut app, vec![egui::Event::PointerMoved(pan_pos)]);
    let (mut pan_out, mut scroll_offset, _, _) = step(&mut app, Vec::new());
    for _ in 0..4 {
        let res = step(&mut app, Vec::new());
        pan_out = res.0;
        scroll_offset = res.1;
    }
    assert!(
        scroll_offset > 10.0,
        "expected vertical autoscroll downward, got offset={scroll_offset}"
    );
    assert_eq!(
        pan_out.platform_output.cursor_icon,
        egui::CursorIcon::ResizeSouthWest
    );
    let anchor_circle = pan_out
        .shapes
        .iter()
        .find_map(|s| find_circle_center_recursive(&s.shape, 13.5, anchor.x + 16.0))
        .expect("autoscroll anchor circle");
    assert!(
        anchor_circle.distance(anchor) <= 1.0,
        "expected anchor circle near {anchor:?}, got {anchor_circle:?}"
    );

    // Exiting via left-click over Card 0 must deactivate without clicking Card 0.
    let exit_pos = card_rect.center();
    let (_, _, _, clicked_button) = step(
        &mut app,
        vec![
            egui::Event::PointerMoved(exit_pos),
            egui::Event::PointerButton {
                pos: exit_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: exit_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(!app.autoscroll.is_active());
    assert!(
        !clicked_button,
        "exiting autoscroll via click must not activate buttons underneath"
    );

    // Escape exits click-toggle autoscroll.
    let _ = step(
        &mut app,
        vec![
            egui::Event::PointerButton {
                pos: anchor,
                button: egui::PointerButton::Middle,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: anchor,
                button: egui::PointerButton::Middle,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(app.autoscroll.is_active());
    let _ = step(
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.autoscroll.is_active());

    // Hold-and-drag autoscroll exits automatically on middle release.
    let offset_before_hold = scroll_offset;
    let _ = step(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: anchor,
            button: egui::PointerButton::Middle,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let _ = step(
        &mut app,
        vec![egui::Event::PointerMoved(egui::pos2(
            anchor.x,
            anchor.y + 150.0,
        ))],
    );
    let (hold_out, scroll_after_hold, _, _) = step(&mut app, Vec::new());
    assert!(scroll_after_hold > offset_before_hold);
    assert_eq!(
        hold_out.platform_output.cursor_icon,
        egui::CursorIcon::ResizeSouth
    );
    let _ = step(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: egui::pos2(anchor.x, anchor.y + 150.0),
            button: egui::PointerButton::Middle,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!app.autoscroll.is_active());
}

#[test]
fn prune_session_caches_trims_stale_metadata_while_preserving_active_references() {
    let mut app = FastTube::default();
    let selected_url = "https://www.youtube.com/watch?v=Sel12345678".to_owned();
    let feed_url = "https://www.youtube.com/watch?v=Feed2345678".to_owned();
    let queue_url = "https://www.youtube.com/watch?v=Queu3456789".to_owned();
    let back_url = "https://www.youtube.com/watch?v=Back4567890".to_owned();

    let mk_video = |url: &str| Video {
        title: "T".into(),
        channel: "C".into(),
        url: url.to_owned(),
        thumbnail: String::new(),
        duration: "1:00".into(),
    };

    app.selected = Some(mk_video(&selected_url));
    app.feed.videos.push(mk_video(&feed_url));
    app.queue.data.add(mk_video(&queue_url)).unwrap();
    app.playback.video_back_stack.push(mk_video(&back_url));

    for url in [&selected_url, &feed_url, &queue_url, &back_url] {
        app.video_metadata
            .insert(url.clone(), VideoMetadata::default());
    }
    for idx in 0..260 {
        app.video_metadata.insert(
            format!("https://www.youtube.com/watch?v=Stale{idx:06}"),
            VideoMetadata::default(),
        );
    }
    assert!(app.video_metadata.len() > 250);

    app.prune_session_caches();

    assert_eq!(app.video_metadata.len(), 4);
    assert!(app.video_metadata.contains_key(&selected_url));
    assert!(app.video_metadata.contains_key(&feed_url));
    assert!(app.video_metadata.contains_key(&queue_url));
    assert!(app.video_metadata.contains_key(&back_url));
}

#[test]
fn sync_power_window_hidden_condition_covers_minimized_and_tray() {
    assert!(!FastTube::is_window_hidden_for_power(false, false));
    assert!(FastTube::is_window_hidden_for_power(true, false));
    assert!(FastTube::is_window_hidden_for_power(false, true));
    assert!(FastTube::is_window_hidden_for_power(true, true));
}
