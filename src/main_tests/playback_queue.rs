use super::super::*;
use super::helpers::*;

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
fn format_time_handles_minutes_and_hours() {
    assert_eq!(format_time(0.0), "0:00");
    assert_eq!(format_time(65.9), "1:05");
    assert_eq!(format_time(3661.0), "1:01:01");
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
            watched_at_ms: None,
            ..Default::default()
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
            watched_at_ms: None,
            ..Default::default()
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
    app.flush_persistence();

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
    app.flush_persistence();

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

    app.flush_persistence();
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
        has_previous: false,
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
    assert!(app.add_current_bookmark(&video, overlay_ctx.position));
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
fn fullscreen_from_maximized_unmaximizes_clears_shadow_and_restores_on_exit() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let window_cmds = |out: &egui::FullOutput| -> Vec<egui::ViewportCommand> {
        out.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .filter(|cmd| !matches!(cmd, egui::ViewportCommand::SetTheme(_)))
            .cloned()
            .collect()
    };

    let mut raw_max = egui::RawInput::default();
    if let Some(vp) = raw_max.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(true);
        vp.fullscreen = Some(false);
    }
    let mut out1 = ctx.run_ui(raw_max, |ui| {
        app.toggle_fullscreen(ui.ctx());
    });
    out1.textures_delta.clear();
    assert_eq!(
        window_cmds(&out1),
        vec![
            egui::ViewportCommand::Maximized(false),
            egui::ViewportCommand::Fullscreen(true),
            egui::ViewportCommand::Decorations(false),
        ]
    );
    assert!(app.playback.fullscreen);
    assert!(app.playback.fullscreen_restore_maximized);

    let mut raw_fs = egui::RawInput::default();
    if let Some(vp) = raw_fs.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(true);
    }
    let mut out2 = ctx.run_ui(raw_fs, |ui| {
        app.toggle_fullscreen(ui.ctx());
    });
    out2.textures_delta.clear();
    assert_eq!(
        window_cmds(&out2),
        vec![
            egui::ViewportCommand::Fullscreen(false),
            egui::ViewportCommand::Decorations(false),
            egui::ViewportCommand::Maximized(true),
        ]
    );
    assert!(!app.playback.fullscreen);
    assert!(!app.playback.fullscreen_restore_maximized);
}

#[test]
fn fullscreen_from_normal_window_does_not_toggle_maximized() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let window_cmds = |out: &egui::FullOutput| -> Vec<egui::ViewportCommand> {
        out.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .filter(|cmd| !matches!(cmd, egui::ViewportCommand::SetTheme(_)))
            .cloned()
            .collect()
    };

    let mut raw_normal = egui::RawInput::default();
    if let Some(vp) = raw_normal.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(false);
    }
    let mut out1 = ctx.run_ui(raw_normal, |ui| {
        app.toggle_fullscreen(ui.ctx());
    });
    out1.textures_delta.clear();
    assert_eq!(
        window_cmds(&out1),
        vec![
            egui::ViewportCommand::Fullscreen(true),
            egui::ViewportCommand::Decorations(false),
        ]
    );
    assert!(app.playback.fullscreen);
    assert!(!app.playback.fullscreen_restore_maximized);

    let mut raw_fs = egui::RawInput::default();
    if let Some(vp) = raw_fs.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(true);
    }
    let mut out2 = ctx.run_ui(raw_fs, |ui| {
        app.toggle_fullscreen(ui.ctx());
    });
    out2.textures_delta.clear();
    assert_eq!(
        window_cmds(&out2),
        vec![
            egui::ViewportCommand::Fullscreen(false),
            egui::ViewportCommand::Decorations(false),
        ]
    );
    assert!(!app.playback.fullscreen);
    assert!(!app.playback.fullscreen_restore_maximized);
}

#[test]
fn sync_fullscreen_viewport_heals_maximized_state_with_bounded_retries() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.playback.fullscreen = true;
    app.playback.fullscreen_sync_retries = 3;
    let window_cmds = |out: &egui::FullOutput| -> Vec<egui::ViewportCommand> {
        out.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .filter(|cmd| !matches!(cmd, egui::ViewportCommand::SetTheme(_)))
            .cloned()
            .collect()
    };

    for _ in 0..3 {
        let mut raw = egui::RawInput::default();
        if let Some(vp) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
            vp.maximized = Some(true);
            vp.fullscreen = Some(true);
        }
        let mut out = ctx.run_ui(raw, |ui| {
            app.sync_fullscreen_viewport(ui.ctx());
        });
        out.textures_delta.clear();
        assert_eq!(
            window_cmds(&out),
            vec![
                egui::ViewportCommand::Fullscreen(false),
                egui::ViewportCommand::Maximized(false),
                egui::ViewportCommand::Fullscreen(true),
                egui::ViewportCommand::Decorations(false),
            ]
        );
        assert!(app.playback.fullscreen_restore_maximized);
    }

    let mut raw_exhausted = egui::RawInput::default();
    if let Some(vp) = raw_exhausted.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(true);
        vp.fullscreen = Some(true);
    }
    let mut out_exhausted = ctx.run_ui(raw_exhausted, |ui| {
        app.sync_fullscreen_viewport(ui.ctx());
    });
    out_exhausted.textures_delta.clear();
    assert!(window_cmds(&out_exhausted).is_empty());
}

#[test]
fn fullscreen_and_mini_player_transitions_preserve_maximized_restore_state() {
    let ctx_a = egui::Context::default();
    let mut app_a = FastTube {
        selected: Some(sample_videos().remove(0)),
        ..Default::default()
    };

    let mut raw_max = egui::RawInput::default();
    if let Some(vp) = raw_max.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(true);
        vp.fullscreen = Some(false);
    }
    let mut out_a1 = ctx_a.run_ui(raw_max, |ui| {
        app_a.toggle_fullscreen(ui.ctx());
    });
    out_a1.textures_delta.clear();
    assert!(app_a.playback.fullscreen);
    assert!(app_a.playback.fullscreen_restore_maximized);

    let mut raw_fs = egui::RawInput::default();
    if let Some(vp) = raw_fs.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(true);
    }
    let mut out_a2 = ctx_a.run_ui(raw_fs, |ui| {
        app_a.toggle_mini_player(ui.ctx());
    });
    out_a2.textures_delta.clear();
    let cmds_a2 = &out_a2.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(app_a.playback.mini_player);
    assert!(!app_a.playback.fullscreen);
    assert!(app_a.playback.mini_player_restore_maximized);
    assert!(!app_a.playback.fullscreen_restore_maximized);
    assert!(cmds_a2.contains(&egui::ViewportCommand::Fullscreen(false)));
    assert!(cmds_a2.contains(&egui::ViewportCommand::Decorations(false)));

    let ctx_b = egui::Context::default();
    let mut app_b = FastTube {
        selected: Some(sample_videos().remove(0)),
        ..Default::default()
    };

    let mut raw_b1 = egui::RawInput::default();
    if let Some(vp) = raw_b1.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(true);
        vp.fullscreen = Some(false);
    }
    let mut out_b1 = ctx_b.run_ui(raw_b1, |ui| {
        app_b.toggle_mini_player(ui.ctx());
    });
    out_b1.textures_delta.clear();
    assert!(app_b.playback.mini_player);
    assert!(app_b.playback.mini_player_restore_maximized);

    let mut raw_b2 = egui::RawInput::default();
    if let Some(vp) = raw_b2.viewports.get_mut(&egui::ViewportId::ROOT) {
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
    let mut out_b2 = ctx_b.run_ui(raw_b2, |ui| {
        app_b.sync_mini_player_viewport(ui.ctx());
    });
    out_b2.textures_delta.clear();
    assert!(app_b.playback.mini_player_settled);

    let mut raw_b3 = egui::RawInput::default();
    if let Some(vp) = raw_b3.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(false);
    }
    let mut out_b3 = ctx_b.run_ui(raw_b3, |ui| {
        app_b.toggle_fullscreen(ui.ctx());
    });
    out_b3.textures_delta.clear();
    let cmds_b3 = &out_b3.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(!cmds_b3.contains(&egui::ViewportCommand::Maximized(true)));
    assert!(cmds_b3.contains(&egui::ViewportCommand::Fullscreen(true)));
    assert!(cmds_b3.contains(&egui::ViewportCommand::Decorations(false)));
    assert!(app_b.playback.fullscreen);
    assert!(!app_b.playback.mini_player);
    assert!(app_b.playback.fullscreen_restore_maximized);
    assert!(!app_b.playback.mini_player_restore_maximized);

    let mut raw_b4 = egui::RawInput::default();
    if let Some(vp) = raw_b4.viewports.get_mut(&egui::ViewportId::ROOT) {
        vp.maximized = Some(false);
        vp.fullscreen = Some(true);
    }
    let mut out_b4 = ctx_b.run_ui(raw_b4, |ui| {
        app_b.toggle_fullscreen(ui.ctx());
    });
    out_b4.textures_delta.clear();
    assert_eq!(
        out_b4.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![
            egui::ViewportCommand::Fullscreen(false),
            egui::ViewportCommand::Decorations(false),
            egui::ViewportCommand::Maximized(true),
        ]
    );
    assert!(!app_b.playback.fullscreen);
    assert!(!app_b.playback.fullscreen_restore_maximized);
}

#[test]
fn poll_quality_populates_subtitles_from_playback_metadata() {
    let mut app = FastTube::default();
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    app.watch.extras.sync_video(Some(url));
    assert!(!app.watch.extras.subtitles_loaded);

    let (req_id, _cancel, tx) = app.playback.quality_task.begin_manual();
    tx.send((
        req_id,
        Ok(search::VideoPlaybackMetadata {
            heights: vec![1080, 720, 480],
            subtitles: vec![search::SubtitleTrack {
                language: "pt".into(),
                url: "https://www.youtube.com/api/timedtext?v=dQw4w9WgXcQ&lang=pt&fmt=vtt".into(),
                automatic: false,
            }],
            ..Default::default()
        }),
    ))
    .unwrap();

    app.poll_quality();
    assert!(!app.playback.quality_task.is_running());
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
            watched_at_ms: None,
            ..Default::default()
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
fn live_playback_state_tracks_pause_lag_dvr_lag_and_go_live_edge() {
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
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
fn handle_player_overlay_action_live_non_dvr_disables_loop_autoplay_and_restores_live_edge() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
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
        has_previous: false,
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
fn ab_loop_overlay_actions_and_auto_seek_enforcer() {
    let mut app = FastTube::default();
    let mut video = sample_videos().remove(0);
    video.url = "https://www.youtube.com/watch?v=abLoopTest1".into();
    let now = Instant::now();
    app.selected = Some(video.clone());
    app.activate_video_playback(&video, Some(0), None, now);

    app.set_ab_loop_point_a(20.0);
    assert_eq!(app.playback.loop_a_secs, Some(20.0));
    assert_eq!(app.playback.ab_loop_active(), None);

    // Setting B <= A clears A so an invalid inverted range cannot stay active.
    app.set_ab_loop_point_b(10.0);
    assert_eq!(app.playback.loop_a_secs, None);
    assert_eq!(app.playback.loop_b_secs, Some(10.0));

    // Proper A < B activates the loop and auto-seeks back to A when reaching B.
    app.set_ab_loop_point_a(15.0);
    app.set_ab_loop_point_b(45.0);
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

    app.clear_ab_loop_points();
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
        has_previous: false,
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

    // Plain N must NOT trigger next video; Shift+N triggers next queued/recommended video.
    app.in_app_ui_pass = true;
    send_key(&ctx, &mut app, egui::Key::N, egui::Modifiers::NONE, None);
    assert!(!app.queue.pending_advance);
    assert!(app.playback.pending_open_video.is_none());

    send_key(&ctx, &mut app, egui::Key::N, egui::Modifiers::SHIFT, None);
    assert!(app.queue.pending_advance);
    app.queue.pending_advance = false;
    let _ = app.queue.data.remove(0);
    app.feed.videos = vec![next_video.clone()];
    app.feed.cache_dirty = true;
    send_key(&ctx, &mut app, egui::Key::N, egui::Modifiers::NONE, None);
    assert!(app.playback.pending_open_video.is_none());
    send_key(&ctx, &mut app, egui::Key::N, egui::Modifiers::SHIFT, None);
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
    assert!(texts.iter().any(|t| t.contains("Shift+, / Shift+.")));
    assert!(texts.iter().any(|t| t.contains("[ / ] / \\")));
    assert!(texts.iter().any(|t| t.contains("capturar quadro")));
}

#[test]
fn queue_power_controls_play_now_move_top_shuffle_dedupe_clear_and_save_collection() {
    let dir = std::env::temp_dir().join(format!("fasttube-queue-power-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let queue_path = dir.join("queue.json");
    let lib_path = dir.join("library.json");

    let mut app = FastTube::default();
    app.queue.path = Some(queue_path.clone());
    app.queue.read_failed = false;
    app.library.path = Some(lib_path.clone());
    app.queue.data = Queue::new(10);

    let videos = three_youtube_videos();
    app.enqueue_video(videos[0].clone()).unwrap();
    app.enqueue_video(videos[1].clone()).unwrap();
    app.enqueue_video(videos[0].clone()).unwrap(); // duplicate of videos[0]
    app.enqueue_video(videos[2].clone()).unwrap();

    // Header duration summary renders in queue_section_ui
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            app.queue_section_ui(ui, "test_queue_hdr", false, None, None);
        },
    );
    let rendered = collect_rendered_text(&out.shapes);
    out.textures_delta.clear();
    assert!(
        rendered
            .iter()
            .any(|t| t.contains("4 vídeos") && t.contains("~")),
        "Queue header must display total count and approximate duration: {rendered:?}"
    );
    assert!(
        rendered
            .iter()
            .any(|t| t.contains("Salvar fila como coleção"))
    );

    // Move last item (videos[2]) to top
    app.move_queue_item_to_top(3);
    assert_eq!(app.queue.data.videos()[0].url, videos[2].url);

    // Deduplicate removes second occurrence of videos[0]
    let removed = app.deduplicate_queue();
    assert_eq!(removed, 1);
    assert_eq!(app.queue.data.len(), 3);

    // Save queue as collection in 1 click (blank name auto-generates "Fila salva")
    let col_name = app.save_queue_as_collection("").expect("save queue");
    assert_eq!(col_name, "Fila salva");
    assert_eq!(app.library.data.collections.len(), 1);
    assert_eq!(app.library.data.collections[0].videos.len(), 3);
    app.flush_persistence();
    let saved_lib = library::load(&lib_path).expect("library persisted");
    assert_eq!(saved_lib.collections[0].name, "Fila salva");

    // Play Now promotes specific middle entry_id cleanly with QueuePlaybackCorrelation
    let mid_id = app.queue.data.entry_id(1).unwrap();
    let mid_url = app.queue.data.videos()[1].url.clone();
    let now = Instant::now();
    let played = app
        .play_queue_entry_at(mid_id, now)
        .expect("play_queue_entry_at returns matching video");
    assert_eq!(played.url, mid_url);
    assert_eq!(app.queue.pending_entry_id, Some(mid_id));
    assert!(app.queue.pending_video.is_some());

    // Confirm StartFile/FileLoaded consumes only that entry_id
    app.handle_queue_player_event(PlaybackEvent::StartFile {
        playlist_entry_id: 101,
    });
    app.handle_queue_player_event(PlaybackEvent::FileLoaded);
    assert_eq!(app.queue.data.len(), 2);
    assert!(app.queue.data.index_of_entry(mid_id).is_none());

    // Shuffle + Clear queue
    app.shuffle_queue();
    assert_eq!(app.queue.data.len(), 2);
    app.clear_queue();
    assert!(app.queue.data.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_sidebar_omits_playback_queue_panel() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.queue.data = Queue::new(10);
    let videos = three_youtube_videos();
    app.feed.videos = videos.clone();
    app.selected = Some(videos[0].clone());
    app.queue.data.add(videos[1].clone()).unwrap();
    app.queue.data.add(videos[2].clone()).unwrap();

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 900.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_sidebar_results_ui(ui, &c, "test_watch_sidebar_no_queue");
        },
    );
    let rendered = collect_rendered_text(&out.shapes);
    out.textures_delta.clear();

    assert!(
        rendered.iter().any(|t| t == "Todos") && rendered.iter().any(|t| t == "Relacionados"),
        "Watch sidebar must still render its filter tabs: {rendered:?}"
    );
    assert!(
        !rendered.iter().any(|t| {
            t.contains("Fila de reprodu\u{00e7}\u{00e3}o")
                || t.contains("Playback queue")
                || t.contains("Salvar fila como cole\u{00e7}\u{00e3}o")
        }),
        "Watch sidebar must omit the playback queue panel: {rendered:?}"
    );
}

#[test]
fn v030_keybindings_parse_format_and_normalization() {
    assert_eq!(
        settings::Keybindings::parse_egui_key("Space"),
        Some((false, false, false, egui::Key::Space))
    );
    assert_eq!(
        settings::Keybindings::parse_egui_key("Ctrl+Shift+M"),
        Some((true, false, true, egui::Key::M))
    );
    assert_eq!(
        settings::Keybindings::format_egui_key(true, false, false, egui::Key::K),
        Some("Ctrl+K".to_owned())
    );
    let custom = settings::Keybindings {
        play_pause: "   ".to_owned(),
        seek_back: "J".to_owned(),
        ..settings::Keybindings::default()
    }
    .normalized();
    assert_eq!(custom.play_pause, "K");
    assert_eq!(custom.seek_back, "J");
    assert!(custom.valid());
}

#[test]
fn v030_queue_repeat_mode_and_playlist_id_extraction() {
    let mode = queue::RepeatMode::Off;
    assert_eq!(mode.cycle(), queue::RepeatMode::All);
    assert_eq!(queue::RepeatMode::All.cycle(), queue::RepeatMode::One);
    assert_eq!(queue::RepeatMode::One.cycle(), queue::RepeatMode::Off);

    assert_eq!(
        extract_playlist_id_from_url("https://www.youtube.com/playlist?list=PLabcdef12345"),
        Some("PLabcdef12345".to_owned())
    );
    assert_eq!(
        extract_playlist_id_from_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
        None
    );
}

#[test]
fn queued_video_card_renders_na_fila_badge() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
    let video = Video {
        title: "Queued Walkthrough".into(),
        channel: "Rust Channel".into(),
        url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
        thumbnail: String::new(),
        duration: "8:20".into(),
    };
    app.queue.data.add(video.clone()).expect("enqueue video");
    let scored = ScoredVideo {
        video,
        score: 100.0,
        metadata: VideoMetadata::default(),
        is_short: false,
        reasons: Vec::new(),
        matched_topic: None,
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 340.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.youtube_grid_card_ui(ui, &c, &scored, 300.0);
        },
    );
    let rendered = collect_rendered_text(&out.shapes);
    out.textures_delta.clear();
    assert!(
        rendered.iter().any(|t| t == "Na fila"),
        "expected 'Na fila' badge on queued video card, got: {rendered:?}"
    );
}

#[test]
fn in_app_docked_mini_player_activates_across_tabs_without_desktop_pip_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let videos = three_youtube_videos();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));

    // 1. With auto_mini_player = true, navigating from watch page to Home/tabs docks in-app only.
    app.settings.prefs.value.auto_mini_player = true;
    app.feed.videos = videos.clone();
    app.selected = Some(videos[0].clone());
    assert!(app.is_watch_page_active());

    app.handle_header_home(&ctx);
    assert!(app.is_in_app_mini_player_active());
    assert!(!app.playback.mini_player);
    assert!(!app.is_watch_page_active());
    assert_eq!(
        app.current_navigation_route(),
        app_state::NavigationRoute::Browse {
            section: BrowseSection::Home,
            search_query: None,
        }
    );

    // 2. Switching between non-video tabs keeps the docked miniplayer active instead of closing.
    app.navigate_from_shortcut(&ctx, BrowseSection::History);
    assert!(app.is_in_app_mini_player_active());
    assert!(!app.playback.mini_player);
    assert_eq!(app.ui.browse_section, BrowseSection::History);
    assert_eq!(
        app.current_navigation_route(),
        app_state::NavigationRoute::Browse {
            section: BrowseSection::History,
            search_query: None,
        }
    );

    app.navigate_from_shortcut(&ctx, BrowseSection::Playlists);
    assert!(app.is_in_app_mini_player_active());
    assert_eq!(app.ui.browse_section, BrowseSection::Playlists);

    app.open_subscriptions_section(false, &ctx);
    assert!(app.is_in_app_mini_player_active());
    assert_eq!(app.ui.browse_section, BrowseSection::Subscriptions);

    app.navigate_from_shortcut(&ctx, BrowseSection::Downloads);
    assert!(app.is_in_app_mini_player_active());
    assert_eq!(app.ui.browse_section, BrowseSection::Downloads);

    // 3. Sidebar remains highlighted on active browse tab while miniplayer is docked.
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
    assert!(nav_text.contains("Downloads") && nav_text.contains("Início"));

    // 4. Docked miniplayer stays compact (no footer when idle), reveals title on hover, and expands/closes cleanly.
    let area_id = egui::Id::new("fasttube_in_app_mini_player");
    let mut idle_text = String::new();
    for _ in 0..2 {
        let mut mini_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                app.in_app_mini_player_ui(ui.ctx());
            },
        );
        idle_text = collect_rendered_text(&mini_out.shapes).join("\n");
        mini_out.textures_delta.clear();
    }
    assert!(
        idle_text.trim().is_empty(),
        "idle unhovered miniplayer must not render title or footer controls, got:\n{idle_text}"
    );
    let mini_rect = ctx
        .memory(|m| m.area_rect(area_id))
        .expect("miniplayer area rect");
    let expected_size = FastTube::compute_in_app_mini_player_size(screen.width());
    assert!(
        (mini_rect.width() - (expected_size.x + 2.0)).abs() < 0.5
            && (mini_rect.height() - (expected_size.y + 2.0)).abs() < 0.5,
        "expected pure 16:9 compact miniplayer {:?}, got {:?}",
        expected_size + egui::vec2(2.0, 2.0),
        mini_rect.size()
    );
    let vrect = egui::Rect::from_min_size(mini_rect.min + egui::vec2(1.0, 1.0), expected_size);
    let layout = crate::watch_ui::MiniPlayerOverlayLayout::compute(vrect);

    let mut hovered_text = String::new();
    for _ in 0..2 {
        let mut mini_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(vrect.center())],
                ..Default::default()
            },
            |ui| {
                app.in_app_mini_player_ui(ui.ctx());
            },
        );
        hovered_text = collect_rendered_text(&mini_out.shapes).join("\n");
        mini_out.textures_delta.clear();
    }
    assert!(
        hovered_text.contains(videos[0].title.as_str()),
        "expected video title on hover overlay, got:\n{hovered_text}"
    );

    let _ = headless_pointer_click(&ctx, screen.size(), layout.top_expand.center(), |ui| {
        app.in_app_mini_player_ui(ui.ctx());
    });
    assert!(!app.playback.in_app_mini_player);
    assert!(app.is_watch_page_active());

    // Re-dock via Home, hover miniplayer, then click Close on overlay.
    app.handle_header_home(&ctx);
    assert!(app.is_in_app_mini_player_active());
    for _ in 0..2 {
        let mut mini_out2 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(vrect.center())],
                ..Default::default()
            },
            |ui| {
                app.in_app_mini_player_ui(ui.ctx());
            },
        );
        mini_out2.textures_delta.clear();
    }
    let _ = headless_pointer_click(&ctx, screen.size(), layout.top_close.center(), |ui| {
        app.in_app_mini_player_ui(ui.ctx());
    });
    assert!(app.selected.is_none());
    assert!(!app.playback.in_app_mini_player);

    // 5. Opening same or another video clears in_app_mini_player cleanly.
    app.selected = Some(videos[0].clone());
    app.playback.in_app_mini_player = true;
    app.open_video_at_timestamp(videos[0].clone(), None, &ctx);
    assert!(!app.playback.in_app_mini_player);
    assert!(app.is_watch_page_active());

    app.playback.in_app_mini_player = true;
    app.open_video_at_timestamp(videos[1].clone(), None, &ctx);
    assert!(!app.playback.in_app_mini_player);
    assert!(app.playback.pending_open_video.is_some());

    // 6. With auto_mini_player = false, navigating home closes the video normally.
    app.settings.prefs.value.auto_mini_player = false;
    app.playback.pending_open_video = None;
    app.selected = Some(videos[1].clone());
    app.playback.in_app_mini_player = false;
    app.handle_header_home(&ctx);
    assert!(app.selected.is_none());
    assert!(!app.playback.in_app_mini_player);
    assert!(!app.playback.mini_player);
}

#[test]
fn in_app_mini_player_edge_hover_stability_and_controls_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let videos = three_youtube_videos();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));

    app.settings.prefs.value.auto_mini_player = true;
    app.feed.videos = videos.clone();
    app.selected = Some(videos[1].clone());
    app.playback.video_back_stack = vec![videos[0].clone()];
    app.playback.mini_player_volume = 80.0;
    app.playback.mini_player_muted = false;
    app.playback.mini_player_paused = false;
    assert!(app.enter_in_app_mini_player(&ctx));

    let area_id = egui::Id::new("fasttube_in_app_mini_player");
    let mut base_texts = String::new();
    for _ in 0..2 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |_| app.in_app_mini_player_ui(&ctx),
        );
        base_texts = collect_rendered_text(&out.shapes).join("\n");
        out.textures_delta.clear();
    }

    assert!(
        base_texts.trim().is_empty(),
        "idle unhovered miniplayer must render zero text, got:\n{base_texts}"
    );
    let base_rect = ctx
        .memory(|m| m.area_rect(area_id))
        .expect("miniplayer area rect");
    assert!(
        (base_rect.bottom()
            - (screen.bottom() - crate::watch_ui::IN_APP_MINI_PLAYER_ANCHOR_OFFSET))
            .abs()
            < 0.5,
        "miniplayer should settle at compact bottom-right anchor within 2 frames, got {base_rect:?}"
    );
    let expected_size = FastTube::compute_in_app_mini_player_size(screen.width());
    let card_w = base_rect.width() - 2.0;
    let video_h = (card_w * 9.0 / 16.0).round();
    assert!(
        (card_w - expected_size.x).abs() < 0.5
            && (base_rect.height() - (video_h + 2.0)).abs() < 0.5,
        "miniplayer must have pure 16:9 compact dimensions without footer, got {base_rect:?} vs {expected_size:?}"
    );
    let vrect = egui::Rect::from_min_size(
        base_rect.min + egui::vec2(1.0, 1.0),
        egui::vec2(card_w, video_h),
    );
    let layout = crate::watch_ui::MiniPlayerOverlayLayout::compute(vrect);

    // Hovering any outer edge, corner, video center, or moving off and back onto the top edge must never shift or resize the miniplayer.
    let outside_probe = egui::Pos2::new(10.0, 10.0);
    let hover_probes = [
        egui::pos2(vrect.center().x, vrect.top() + 0.5),
        egui::pos2(vrect.center().x, base_rect.bottom() - 0.5),
        egui::pos2(vrect.left() + 0.5, vrect.center().y),
        egui::pos2(vrect.right() - 0.5, vrect.center().y),
        egui::pos2(vrect.left() + 1.0, vrect.top() + 1.0),
        egui::pos2(vrect.right() - 1.0, vrect.top() + 1.0),
        egui::pos2(base_rect.left() + 1.0, base_rect.bottom() - 1.0),
        egui::pos2(base_rect.right() - 1.0, base_rect.bottom() - 1.0),
        vrect.center(),
        outside_probe,
        egui::pos2(vrect.center().x, vrect.top() + 0.5),
    ];
    for pos in hover_probes {
        let mut txt = String::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerMoved(pos)],
                    ..Default::default()
                },
                |_| app.in_app_mini_player_ui(&ctx),
            );
            txt = collect_rendered_text(&out.shapes).join("\n");
            out.textures_delta.clear();
        }
        if pos == outside_probe {
            assert!(
                txt.trim().is_empty(),
                "moving pointer outside must immediately hide overlay text, got:\n{txt}"
            );
        } else if pos == vrect.center() {
            assert!(
                txt.contains(videos[1].title.as_str()),
                "hovering video center must reveal overlay title, got:\n{txt}"
            );
        }
        let hovered_rect = ctx
            .memory(|m| m.area_rect(area_id))
            .expect("hovered miniplayer rect");
        assert!(
            (hovered_rect.min - base_rect.min).length() < 0.5
                && (hovered_rect.size() - base_rect.size()).length() < 0.5,
            "miniplayer shifted on hover at {pos:?}: base={base_rect:?}, hovered={hovered_rect:?}"
        );
    }

    // Clicking open areas of the video surface or center play button toggles play/pause without leaving the miniplayer.
    for (idx, click_pos) in [
        egui::pos2(vrect.left() + 16.0, vrect.top() + 44.0),
        egui::pos2(vrect.right() - 16.0, vrect.top() + 44.0),
        egui::pos2(vrect.left() + 16.0, vrect.bottom() - 44.0),
        layout.center_play.center(),
    ]
    .into_iter()
    .enumerate()
    {
        let expected_paused = idx % 2 == 0;
        let _ = headless_pointer_click(&ctx, screen.size(), click_pos, |_| {
            app.in_app_mini_player_ui(&ctx);
        });
        assert_eq!(
            app.playback.mini_player_paused, expected_paused,
            "video click #{idx} at {click_pos:?} should toggle pause"
        );
        assert!(
            app.playback.in_app_mini_player,
            "clicking video surface must not expand miniplayer"
        );
    }

    // Clicking the title overlay must also keep the miniplayer docked (expand button only).
    let _ = headless_pointer_click(&ctx, screen.size(), layout.title.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert!(app.playback.in_app_mini_player);

    let hover_overlay = |ctx: &egui::Context, app: &mut FastTube| {
        for _ in 0..2 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerMoved(vrect.center())],
                    ..Default::default()
                },
                |_| app.in_app_mini_player_ui(ctx),
            );
            out.textures_delta.clear();
        }
    };

    // Mute toggle & inline volume slider on the hover overlay.
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(&ctx, screen.size(), layout.bottom_mute.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert!(app.playback.mini_player_muted);
    let slider_track_left = layout.bottom_volume.left() + 5.0;
    let slider_track_right = layout.bottom_volume.right() - 5.0;
    let quarter_x = slider_track_left + (slider_track_right - slider_track_left) * 0.25;
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(
        &ctx,
        screen.size(),
        egui::pos2(quarter_x, layout.bottom_volume.center().y),
        |_| {
            app.in_app_mini_player_ui(&ctx);
        },
    );
    assert!(!app.playback.mini_player_muted);
    assert!(
        (app.playback.mini_player_volume - 25.0).abs() <= 2.0,
        "expected volume near 25%, got {}",
        app.playback.mini_player_volume
    );

    // Previous button pops video_back_stack and preserves miniplayer mode.
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(&ctx, screen.size(), layout.center_prev.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert_eq!(app.playback.pending_open_video, Some(videos[0].clone()));
    assert!(app.playback.pending_open_keep_mini_player);
    assert!(app.playback.pending_open_suppress_back_stack);
    assert_eq!(app.playback.video_forward_stack, vec![videos[1].clone()]);

    // Next button pops video_forward_stack first.
    app.playback.pending_open_video = None;
    app.playback.pending_open_keep_mini_player = false;
    app.playback.pending_open_suppress_back_stack = false;
    app.selected = Some(videos[0].clone());
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(&ctx, screen.size(), layout.center_next.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert_eq!(app.playback.pending_open_video, Some(videos[1].clone()));
    assert!(app.playback.pending_open_keep_mini_player);
    assert_eq!(app.playback.video_back_stack.last(), Some(&videos[0]));

    // With empty forward stack and non-empty queue, Next requests queue advance without popping.
    app.playback.pending_open_video = None;
    app.playback.pending_open_keep_mini_player = false;
    app.playback.video_forward_stack.clear();
    assert!(app.queue.data.add(videos[2].clone()).is_ok());
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(&ctx, screen.size(), layout.center_next.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert!(app.queue.pending_advance);
    assert_eq!(app.queue.data.videos(), &[videos[2].clone()]);

    // With empty forward stack and empty queue, Next falls back to next_recommended_video.
    app.queue.pending_advance = false;
    app.queue.data.clear();
    app.selected = Some(videos[0].clone());
    app.invalidate_feed_cache();
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(&ctx, screen.size(), layout.center_next.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert!(app.playback.pending_open_video.is_some());
    assert!(app.playback.pending_open_keep_mini_player);

    // With empty back stack, Previous falls back to local watch history without ping-ponging.
    app.playback.pending_open_video = None;
    app.playback.pending_open_keep_mini_player = false;
    app.playback.video_back_stack.clear();
    app.playback.video_forward_stack.clear();
    let hist_id = history::youtube_video_id(&videos[2].url).unwrap();
    app.history
        .data
        .record(history::Entry {
            video_id: hist_id,
            title: videos[2].title.clone(),
            channel: videos[2].channel.clone(),
            position_ms: 15_000,
            duration_ms: Some(120_000),
            watched_at_ms: Some(1_700_000_000_000),
            ..Default::default()
        })
        .unwrap();
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(&ctx, screen.size(), layout.center_prev.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert!(
        app.playback
            .pending_open_video
            .as_ref()
            .is_some_and(|v| FastTube::same_video_target(&v.url, &videos[2].url))
    );
    assert!(app.playback.pending_open_keep_mini_player);

    // Only the Expand button returns from the miniplayer to the full watch view.
    hover_overlay(&ctx, &mut app);
    let _ = headless_pointer_click(&ctx, screen.size(), layout.top_expand.center(), |_| {
        app.in_app_mini_player_ui(&ctx);
    });
    assert!(!app.playback.in_app_mini_player);
    assert!(app.is_watch_page_active());
}

#[test]
fn player_right_click_context_menu_and_stats_for_nerds_work_headlessly() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = sample_videos().remove(0);
    app.selected = Some(video.clone());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0));
    let video_rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(800.0, 450.0));

    let run_player_frame = |app: &mut FastTube, events: Vec<egui::Event>| {
        let playback = OverlayPlaybackContext {
            video: video.clone(),
            paused: false,
            ended: false,
            muted: false,
            position: 42.0,
            duration: Some(180.0),
            buffered_secs: Some(15.0),
            raw_volume: 100.0,
            speed: 1.0,
            current_height: Some(1080),
            selected_quality: None,
            qualities: vec![1080, 720],
            qualities_loading: false,
            has_previous: false,
            has_queued: false,
            has_active_subtitle: false,
            subtitles: Vec::new(),
            subtitles_loading: false,
            chapters: Vec::new(),
            theater: false,
            fullscreen: false,
            mini_player: false,
            autoplay: true,
            loop_video: app.playback.loop_video,
            is_live: false,
            is_live_dvr: false,
            at_live_edge: false,
            live_behind_secs: 0.0,
            raw_path: Some(video.url.clone()),
            raw_position: Some(42.0),
            now: Instant::now(),
        };
        let mut emitted = None;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| {
                    let (_, resp) = ui.allocate_exact_size(video_rect.size(), egui::Sense::click());
                    emitted = app.player_overlay_ui(ui, Some((resp.rect, resp)), &playback, &c);
                });
            },
        );
        out.textures_delta.clear();
        (out, emitted)
    };

    // Warm up video area hit rect.
    let _ = run_player_frame(
        &mut app,
        vec![egui::Event::PointerMoved(egui::pos2(300.0, 200.0))],
    );

    // Right-click opens YouTube-style context menu.
    let rclick_pos = egui::pos2(300.0, 200.0);
    let _ = run_player_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: rclick_pos,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let _ = run_player_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: rclick_pos,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.playback.player_context_menu, Some(rclick_pos));
    // Run one repaint frame so egui::Area completes its initial sizing pass and paints the menu.
    let (menu_out, _) = run_player_frame(&mut app, Vec::new());
    let menu_texts = collect_rendered_text_rects(&menu_out.shapes);
    for expected in [
        "Loop",
        "Miniplayer",
        "Copiar URL do vídeo",
        "Copiar URL do vídeo no momento atual",
        "Copiar código de incorporação",
        "Copiar informações de depuração",
        "Resolver problemas de reprodução",
        "Estatísticas para nerds",
    ] {
        assert!(
            menu_texts.iter().any(|(text, _)| text == expected),
            "missing context menu item {expected:?} in {menu_texts:?}"
        );
    }

    // Clicking "Loop" toggles loop_video and closes menu.
    let loop_rect = menu_texts
        .iter()
        .find(|(text, _)| text == "Loop")
        .expect("Loop menu item")
        .1;
    let _ = run_player_frame(
        &mut app,
        vec![egui::Event::PointerMoved(loop_rect.center())],
    );
    let _ = run_player_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: loop_rect.center(),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let _ = run_player_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: loop_rect.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.playback.loop_video);
    assert!(app.playback.player_context_menu.is_none());

    // Open again and toggle "Estatísticas para nerds".
    let _ = run_player_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(rclick_pos),
            egui::Event::PointerButton {
                pos: rclick_pos,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: rclick_pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let (reopened_out, _) = run_player_frame(&mut app, Vec::new());
    let stats_item_rect = collect_rendered_text_rects(&reopened_out.shapes)
        .into_iter()
        .find(|(text, _)| text == "Estatísticas para nerds")
        .expect("Stats for nerds menu item")
        .1;
    let _ = run_player_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(stats_item_rect.center()),
            egui::Event::PointerButton {
                pos: stats_item_rect.center(),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: stats_item_rect.center(),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(app.playback.stats_for_nerds_open);
    let (stats_out, _) = run_player_frame(&mut app, Vec::new());
    let stats_texts = collect_rendered_text(&stats_out.shapes);
    assert!(stats_texts.iter().any(|t| t == "Estatísticas para nerds"));

    // Clicking outside an open context menu dismisses it without toggling Play/Pause.
    let _ = run_player_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(rclick_pos),
            egui::Event::PointerButton {
                pos: rclick_pos,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: rclick_pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let dismiss_pos = egui::pos2(80.0, 420.0);
    let _ = run_player_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(dismiss_pos),
            egui::Event::PointerButton {
                pos: dismiss_pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let (_, emitted_on_dismiss) = run_player_frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: dismiss_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(app.playback.player_context_menu.is_none());
    assert_eq!(emitted_on_dismiss, None);

    // "Resolver problemas de reprodução" routes to BackupDiagnostics settings tab.
    let _ = run_player_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(rclick_pos),
            egui::Event::PointerButton {
                pos: rclick_pos,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: rclick_pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let (troubleshoot_out, _) = run_player_frame(&mut app, Vec::new());
    let troubleshoot_rect = collect_rendered_text_rects(&troubleshoot_out.shapes)
        .into_iter()
        .find(|(text, _)| text == "Resolver problemas de reprodução")
        .expect("Troubleshoot menu item")
        .1;
    let _ = run_player_frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(troubleshoot_rect.center()),
            egui::Event::PointerButton {
                pos: troubleshoot_rect.center(),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: troubleshoot_rect.center(),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(app.ui.settings_open);
    assert_eq!(
        app.settings.active_tab,
        app_state::SettingsTab::BackupDiagnostics
    );
}

#[test]
fn fullscreen_player_auto_hides_overlay_and_cursor_when_mouse_is_idle() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = sample_videos().remove(0);
    app.selected = Some(video.clone());
    app.playback.fullscreen = true;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));

    let run_fs_frame = |app: &mut FastTube, paused: bool, events: Vec<egui::Event>| {
        let playback = OverlayPlaybackContext {
            video: video.clone(),
            paused,
            ended: false,
            muted: false,
            position: 42.0,
            duration: Some(180.0),
            buffered_secs: Some(15.0),
            raw_volume: 100.0,
            speed: 1.0,
            current_height: Some(1080),
            selected_quality: None,
            qualities: vec![1080, 720],
            qualities_loading: false,
            has_previous: false,
            has_queued: false,
            has_active_subtitle: false,
            subtitles: Vec::new(),
            subtitles_loading: false,
            chapters: Vec::new(),
            theater: false,
            fullscreen: true,
            mini_player: false,
            autoplay: true,
            loop_video: false,
            is_live: false,
            is_live_dvr: false,
            at_live_edge: false,
            live_behind_secs: 0.0,
            raw_path: Some(video.url.clone()),
            raw_position: Some(42.0),
            now: Instant::now(),
        };
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| {
                    let (_, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                    let _ = app.player_overlay_ui(ui, Some((resp.rect, resp)), &playback, &c);
                });
            },
        );
        out.textures_delta.clear();
        out
    };

    // Moving mouse inside fullscreen reveals overlay & cursor.
    let pos1 = egui::pos2(640.0, 360.0);
    let active_out = run_fs_frame(&mut app, false, vec![egui::Event::PointerMoved(pos1)]);
    assert!(
        collect_rendered_text(&active_out.shapes)
            .iter()
            .any(|t| t == "0:42 / 3:00")
    );
    assert_ne!(
        active_out.platform_output.cursor_icon,
        egui::CursorIcon::None
    );

    // Leaving mouse stationary past OVERLAY_HIDE_DELAY_SECS hides both HUD and cursor.
    app.playback.overlay_last_activity = Instant::now() - Duration::from_secs(4);
    let idle_out = run_fs_frame(&mut app, false, Vec::new());
    assert!(
        !collect_rendered_text(&idle_out.shapes)
            .iter()
            .any(|t| t == "0:42 / 3:00"),
        "fullscreen HUD should hide when mouse stays still"
    );
    assert_eq!(idle_out.platform_output.cursor_icon, egui::CursorIcon::None);

    // Nudging the mouse brings both HUD and cursor back immediately.
    let pos2 = egui::pos2(656.0, 372.0);
    let wake_out = run_fs_frame(&mut app, false, vec![egui::Event::PointerMoved(pos2)]);
    assert!(
        collect_rendered_text(&wake_out.shapes)
            .iter()
            .any(|t| t == "0:42 / 3:00")
    );
    assert_ne!(wake_out.platform_output.cursor_icon, egui::CursorIcon::None);

    // Paused video keeps HUD and cursor visible even after idle timeout.
    app.playback.overlay_last_activity = Instant::now() - Duration::from_secs(4);
    let paused_out = run_fs_frame(&mut app, true, Vec::new());
    assert!(
        collect_rendered_text(&paused_out.shapes)
            .iter()
            .any(|t| t == "0:42 / 3:00")
    );
    assert_ne!(
        paused_out.platform_output.cursor_icon,
        egui::CursorIcon::None
    );
}

#[test]
fn handle_watch_live_info_populates_qualities_subtitles_and_storyboard_without_yt_dlp() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
    let video = Video {
        title: "Video Teste".into(),
        channel: "Canal".into(),
        url: "https://www.youtube.com/watch?v=WatchMeta01".into(),
        thumbnail: String::new(),
        duration: "3:00".into(),
    };
    app.selected = Some(video.clone());
    app.watch.extras.sync_video(Some(&video.url));
    app.watch.live.video_url = Some(video.url.clone());
    app.watch.live.session_id = 1;
    app.watch.live.watch_info_loading = true;

    let sb = search::StoryboardSpec {
        rows: 5,
        columns: 5,
        thumb_width: 160,
        thumb_height: 90,
        fragments: vec![search::StoryboardFragment {
            url: "https://i.ytimg.com/sb/WatchMeta01/storyboard3_L1/M0.jpg?sigh=abc".into(),
            duration_secs: 50.0,
        }],
    };
    let sub = search::SubtitleTrack {
        language: "pt".into(),
        automatic: false,
        url: "https://www.youtube.com/api/timedtext?v=WatchMeta01&lang=pt&fmt=vtt".into(),
    };
    app.handle_watch_live_info_result(
        1,
        Ok(live_chat::WatchLiveInfo {
            video_id: "WatchMeta01".into(),
            heights: vec![1080, 720, 480],
            subtitles: vec![sub.clone()],
            subtitles_parsed: true,
            storyboard: Some(sb.clone()),
            ..Default::default()
        }),
        &ctx,
    );

    assert_eq!(app.playback.qualities, vec![1080, 720, 480]);
    assert!(app.watch.extras.subtitles_loaded);
    assert_eq!(app.watch.extras.subtitles, vec![sub]);
    assert_eq!(app.playback.storyboard, Some(sb));
    assert_eq!(app.playback.quality_fallback_url, None);
    assert!(!app.playback.quality_task.is_running());
}

#[test]
fn handle_watch_live_info_defers_quality_fallback_until_file_loaded() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
    let video = Video {
        title: "Fallback Teste".into(),
        channel: "Canal".into(),
        url: "https://www.youtube.com/watch?v=WatchFall01".into(),
        thumbnail: String::new(),
        duration: "3:00".into(),
    };
    app.selected = Some(video.clone());
    app.watch.live.video_url = Some(video.url.clone());
    app.watch.live.session_id = 1;
    app.playback.current_file_loaded = false;

    app.handle_watch_live_info_result(
        1,
        Ok(live_chat::WatchLiveInfo {
            video_id: "WatchFall01".into(),
            heights: Vec::new(),
            ..Default::default()
        }),
        &ctx,
    );

    assert_eq!(
        app.playback.quality_fallback_url.as_deref(),
        Some(video.url.as_str())
    );
    assert!(!app.playback.quality_task.is_running());

    app.handle_queue_player_event(PlaybackEvent::FileLoaded);
    assert!(app.playback.current_file_loaded);
}

#[test]
fn try_auto_skip_sponsor_segment_coalesces_contiguous_segments_end_to_end() {
    let mut app = FastTube::default();
    app.settings.prefs.value.sponsorblock.enabled = true;
    app.settings.prefs.value.sponsorblock.skip_mode = sponsorblock::SponsorSkipMode::AutoSkip;
    app.settings.prefs.value.sponsorblock.categories.sponsor = true;
    app.settings.prefs.value.sponsorblock.categories.selfpromo = true;
    app.watch.sponsorblock.segments = vec![
        sponsorblock::SponsorSegment {
            uuid: "seg-a".into(),
            start_secs: 10.0,
            end_secs: 25.0,
            category: sponsorblock::SponsorCategory::Sponsor,
        },
        sponsorblock::SponsorSegment {
            uuid: "seg-b".into(),
            start_secs: 25.2,
            end_secs: 42.0,
            category: sponsorblock::SponsorCategory::SelfPromo,
        },
    ];

    let now = Instant::now();
    let target = app.try_auto_skip_sponsor_segment(11.0, Some(120.0), false, now);
    assert_eq!(target, Some(42.0));
    assert!(app.watch.sponsorblock.auto_skipped_uuids.contains("seg-a"));
    assert!(app.watch.sponsorblock.auto_skipped_uuids.contains("seg-b"));
    assert_eq!(
        app.try_auto_skip_sponsor_segment(26.0, Some(120.0), false, now),
        None
    );
}

#[test]
fn player_wheel_volume_only_triggers_over_volume_area_and_never_fights_page_scroll() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = sample_videos().remove(0);
    app.selected = Some(video.clone());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0));
    let video_size = egui::vec2(800.0, 450.0);
    let mut sim_now = Instant::now();

    let run_scroll_frame = |app: &mut FastTube,
                            raw_volume: f64,
                            muted: bool,
                            mini_player: bool,
                            now: Instant,
                            events: Vec<egui::Event>| {
        let playback = OverlayPlaybackContext {
            video: video.clone(),
            paused: false,
            ended: false,
            muted,
            position: 42.0,
            duration: Some(180.0),
            buffered_secs: Some(15.0),
            raw_volume,
            speed: 1.0,
            current_height: Some(1080),
            selected_quality: None,
            qualities: vec![1080, 720],
            qualities_loading: false,
            has_previous: false,
            has_queued: false,
            has_active_subtitle: false,
            subtitles: Vec::new(),
            subtitles_loading: false,
            chapters: Vec::new(),
            theater: false,
            fullscreen: false,
            mini_player,
            autoplay: true,
            loop_video: false,
            is_live: false,
            is_live_dvr: false,
            at_live_edge: false,
            live_behind_secs: 0.0,
            raw_path: Some(video.url.clone()),
            raw_position: Some(42.0),
            now,
        };
        let mut emitted = None;
        let mut scroll_y = 0.0_f32;
        let mut player_rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                egui::CentralPanel::default().show(ui, |ui| {
                    let sa = egui::ScrollArea::vertical()
                        .id_salt("test_player_wheel_volume_scroll")
                        .show(ui, |ui| {
                            let (_, resp) =
                                ui.allocate_exact_size(video_size, egui::Sense::click());
                            player_rect = resp.rect;
                            emitted =
                                app.player_overlay_ui(ui, Some((resp.rect, resp)), &playback, &c);
                            // Tall content below the player so the page is vertically scrollable.
                            ui.allocate_exact_size(egui::vec2(800.0, 900.0), egui::Sense::hover());
                        });
                    scroll_y = sa.state.offset.y;
                });
            },
        );
        out.textures_delta.clear();
        (out, emitted, scroll_y, player_rect)
    };

    let wheel_down = |modifiers: egui::Modifiers| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -40.0),
        phase: egui::TouchPhase::Move,
        modifiers,
    };
    let wheel_up = |modifiers: egui::Modifiers| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 40.0),
        phase: egui::TouchPhase::Move,
        modifiers,
    };

    // Warm up layout and locate the bottom bar and time readout to anchor the volume region.
    let stage_center = egui::pos2(400.0, 200.0);
    let (warm_out, _, initial_scroll, player_rect) = run_scroll_frame(
        &mut app,
        50.0,
        false,
        false,
        sim_now,
        vec![egui::Event::PointerMoved(stage_center)],
    );
    assert_eq!(initial_scroll, 0.0);
    let bar_rect = overlay::compute_bar_rect(player_rect).expect("bar_rect");
    let time_rect = collect_rendered_text_rects(&warm_out.shapes)
        .into_iter()
        .find(|(text, _)| text == "0:42 / 3:00")
        .expect("time readout rect")
        .1;
    let bar_y = time_rect.center().y;
    let slider_center = egui::pos2(time_rect.left() - 36.0, bar_y);
    let mute_center = egui::pos2(time_rect.left() - 86.0, bar_y);

    // 1. Scrolling over the video stage (even with Shift held) scrolls the page and never sets volume.
    for modifiers in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
        sim_now += Duration::from_millis(400);
        let (_, emitted, scroll_after, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now,
            vec![
                egui::Event::PointerMoved(stage_center),
                wheel_down(modifiers),
            ],
        );
        assert_eq!(emitted, None, "stage wheel should not change volume");
        if modifiers == egui::Modifiers::NONE {
            assert!(scroll_after > 0.0, "stage wheel should scroll page");

            // Scroll back to top.
            sim_now += Duration::from_millis(400);
            let (_, emitted_up, scroll_reset, _) = run_scroll_frame(
                &mut app,
                50.0,
                false,
                false,
                sim_now,
                vec![egui::Event::PointerMoved(stage_center), wheel_up(modifiers)],
            );
            assert_eq!(emitted_up, None);
            assert!(
                scroll_reset < scroll_after,
                "stage wheel up should scroll page back toward top"
            );
        }
    }

    // 2. Scrolling over non-volume parts of the bottom control bar (play button, progress strip, right buttons)
    //    must scroll the page and never change volume.
    let non_volume_bar_points = [
        egui::pos2(bar_rect.left() + 26.0, bar_y), // play button
        egui::pos2(bar_rect.center().x, bar_rect.top() + 8.0), // progress strip
        egui::pos2(bar_rect.right() - 28.0, bar_y), // fullscreen button
    ];
    for pos in non_volume_bar_points {
        sim_now += Duration::from_millis(400);
        let (_, _, scroll_before, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now,
            vec![egui::Event::PointerMoved(pos)],
        );
        let (_, emitted, scroll_after, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now + Duration::from_millis(20),
            vec![
                egui::Event::PointerMoved(pos),
                wheel_down(egui::Modifiers::NONE),
            ],
        );
        assert_eq!(
            emitted, None,
            "non-volume bar point {pos:?} must not adjust volume"
        );
        assert!(
            scroll_after > scroll_before,
            "non-volume bar point {pos:?} should allow page scroll"
        );

        sim_now += Duration::from_millis(400);
        let (_, emitted_up, scroll_reset, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now,
            vec![
                egui::Event::PointerMoved(pos),
                wheel_up(egui::Modifiers::NONE),
            ],
        );
        assert_eq!(emitted_up, None);
        assert!(
            scroll_reset < scroll_after,
            "non-volume bar point {pos:?} should scroll back up"
        );
    }

    // Reset scroll offset to 0 before volume hit assertions.
    sim_now += Duration::from_millis(400);
    let _ = run_scroll_frame(
        &mut app,
        50.0,
        false,
        false,
        sim_now,
        vec![
            egui::Event::PointerMoved(stage_center),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 200.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );

    // 3. Mini-player mode outside the volume area must also NOT adjust volume on wheel.
    sim_now += Duration::from_millis(400);
    let (_, mini_emitted, _, _) = run_scroll_frame(
        &mut app,
        50.0,
        false,
        true,
        sim_now,
        vec![
            egui::Event::PointerMoved(stage_center),
            wheel_down(egui::Modifiers::NONE),
        ],
    );
    assert_eq!(mini_emitted, None);

    // 4. Scrolling over the mute icon and volume slider adjusts volume and suppresses page scrolling.
    for vol_pos in [mute_center, slider_center] {
        sim_now += Duration::from_millis(400);
        let (_, _, scroll_before, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now,
            vec![egui::Event::PointerMoved(vol_pos)],
        );
        let (_, emitted, scroll_after, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now + Duration::from_millis(20),
            vec![
                egui::Event::PointerMoved(vol_pos),
                wheel_up(egui::Modifiers::NONE),
            ],
        );
        assert_eq!(emitted, Some(overlay::OverlayAction::SetVolume(55.0)));
        assert_eq!(
            scroll_after, scroll_before,
            "scrolling over volume area {vol_pos:?} must not scroll page"
        );
    }

    // 5. Scrolling up while muted over the volume area unmutes to 5% and keeps page scroll fixed.
    sim_now += Duration::from_millis(400);
    let (_, emitted_unmute, scroll_after_unmute, _) = run_scroll_frame(
        &mut app,
        40.0,
        true,
        false,
        sim_now,
        vec![
            egui::Event::PointerMoved(slider_center),
            wheel_up(egui::Modifiers::NONE),
        ],
    );
    assert_eq!(emitted_unmute, Some(overlay::OverlayAction::SetVolume(5.0)));
    assert_eq!(scroll_after_unmute, 0.0);

    // 6. Scroll-through guard: when the page scrolls while the cursor is outside the volume bar
    //    and the volume bar slides under that stationary cursor position on the very next wheel event,
    //    continued scrolling must keep scrolling the page and never adjust volume.
    sim_now += Duration::from_millis(400);
    // Start above the bottom control bar (`bar_y - 30.0`), scroll down so the page scrolls and arms
    // the guard while keeping the cursor stationary.
    let stationary_pos = egui::pos2(slider_center.x, bar_y - 30.0);
    let mut scroll_prime = 0.0;
    for prime_attempt in 1..=2 {
        let (_, emitted_prime, next_scroll, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now + Duration::from_millis(10 * prime_attempt),
            vec![
                egui::Event::PointerMoved(stationary_pos),
                wheel_down(egui::Modifiers::NONE),
            ],
        );
        assert_eq!(emitted_prime, None);
        scroll_prime = next_scroll;
        if scroll_prime > 0.0 {
            break;
        }
    }
    assert!(
        scroll_prime > 0.0,
        "initial page scroll above volume bar must scroll page"
    );
    // Keep scrolling with the pointer stationary at `stationary_pos` as the volume bar passes underneath it.
    for step in 1..=8 {
        let (_, emitted_guard, scroll_next, _) = run_scroll_frame(
            &mut app,
            50.0,
            false,
            false,
            sim_now + Duration::from_millis(30 + 30 * step),
            vec![
                egui::Event::PointerMoved(stationary_pos),
                if step <= 4 {
                    wheel_down(egui::Modifiers::NONE)
                } else {
                    wheel_up(egui::Modifiers::NONE)
                },
            ],
        );
        assert_eq!(
            emitted_guard, None,
            "volume must not change when bar slides under stationary cursor during scroll (step {step}, scroll={scroll_next})"
        );
    }
}
