use super::super::*;
use super::helpers::*;

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
    app.flush_persistence();
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
    app.flush_persistence();
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
    app.flush_persistence();
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

    let t3 = t0 + HISTORY_SAVE_INTERVAL + Duration::from_secs(1);
    let _ = app.process_player_telemetry(
        &videos[0],
        Some(&videos[0].url),
        Some(45.0),
        Some(200.0),
        false,
        t3,
    );
    app.flush_persistence();
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
    app.flush_persistence();
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
    app.flush_persistence();

    let final_disk = history::load(&path).unwrap();
    assert_eq!(final_disk.entries()[0].video_id, "0123456789A");
    assert_eq!(final_disk.entries()[0].position_ms, 18_000);
    assert_eq!(final_disk.entries()[1].video_id, "dQw4w9WgXcQ");
    assert_eq!(final_disk.entries()[1].position_ms, 62_000);

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
    valid_app.flush_persistence();
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
    valid_app.flush_persistence();
    assert!(valid_app.history.data.entries().is_empty());
    assert!(history::load(&valid_path).unwrap().entries().is_empty());

    let _ = std::fs::remove_dir_all(&temp_dir);
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
    let (req_id, _cancel, sender) = app.auth.reauth_task.begin_manual();
    sender
        .send((req_id, Ok(oauth::TokenSession::test_session("novo-token"))))
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
fn poll_rating_updates_ratings_refreshed_session_and_status_messages() {
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("old-token"));

    let (req_id, _cancel, tx) = app.auth.rating_task.begin_manual();
    tx.send((
        req_id,
        (
            Some(oauth::TokenSession::test_session("refreshed-token")),
            Ok(("dQw4w9WgXcQ".into(), Some(oauth::VideoRating::Like))),
        ),
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
    assert!(!app.auth.rating_task.is_running());

    let (req_id, _cancel, tx) = app.auth.rating_task.begin_manual();
    tx.send((
        req_id,
        (
            None,
            Ok(("dQw4w9WgXcQ".into(), Some(oauth::VideoRating::Dislike))),
        ),
    ))
    .unwrap();
    app.poll_rating();
    assert_eq!(
        app.auth.ratings.get("dQw4w9WgXcQ").copied(),
        Some(oauth::VideoRating::Dislike)
    );
    assert_eq!(app.message, "Você não gostou deste vídeo.");

    let (req_id, _cancel, tx) = app.auth.rating_task.begin_manual();
    tx.send((req_id, (None, Ok(("dQw4w9WgXcQ".into(), None)))))
        .unwrap();
    app.poll_rating();
    assert!(!app.auth.ratings.contains_key("dQw4w9WgXcQ"));
    assert_eq!(app.message, "Avaliação removida.");

    let (req_id, _cancel, tx) = app.auth.rating_task.begin_manual();
    tx.send((
        req_id,
        (
            None,
            Err("Sua sessão Google expirou ou foi revogada. Entre novamente.".into()),
        ),
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
fn watch_metadata_card_omits_transcript_and_markers_section() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
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
    let (req_id, _cancel, tx) = app.auth.report_task.begin_manual();
    tx.send((
        req_id,
        (
            Some(oauth::TokenSession::test_session("tok-2")),
            Ok("Spam ou enganoso".into()),
        ),
    ))
    .unwrap();

    app.poll_report(&ctx);

    assert_eq!(
        app.auth.session.as_ref().map(|s| s.access_token.as_str()),
        Some("tok-2")
    );
    assert!(!app.auth.report_task.is_running());
    assert!(app.message.contains("Denúncia enviada ao YouTube"));
    assert!(app.message.contains("Spam ou enganoso"));
}

#[test]
fn watch_metadata_prefers_real_channel_name_over_channel_id_and_formats_live_stats() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
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
fn watch_live_chat_panel_ui_renders_modes_messages_and_toggles_visibility() {
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
        ..Default::default()
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
        ..Default::default()
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
fn live_chat_panel_filters_messages_and_shows_scroll_paused_pill() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.history.data = history::History::default();
    app.history.path = None;
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
            ..Default::default()
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
fn library_play_all_and_enqueue_all_preserve_order_and_persist() {
    let dir = std::env::temp_dir().join(format!("fasttube-lib-playall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let queue_path = dir.join("queue.json");

    let mut app = FastTube::default();
    app.queue.path = Some(queue_path.clone());
    app.queue.read_failed = false;
    app.queue.data = Queue::new(10);

    let videos = three_youtube_videos();
    let first = app.play_all_videos(&videos).expect("first video to play");
    assert_eq!(first.url, videos[0].url);
    assert_eq!(app.queue.data.len(), 2);
    assert_eq!(app.queue.data.videos()[0].url, videos[1].url);
    assert_eq!(app.queue.data.videos()[1].url, videos[2].url);
    assert!(app.library.notice.as_deref().unwrap_or("").contains("fila"));

    app.flush_persistence();
    let reloaded = Queue::load(&queue_path, 10).expect("queue persisted");
    assert_eq!(reloaded.len(), 2);
    assert_eq!(reloaded.videos()[0].url, videos[1].url);

    let mut app2 = FastTube::default();
    app2.queue.path = Some(queue_path.clone());
    app2.queue.read_failed = false;
    app2.queue.data = Queue::new(10);
    let (added, rejected) = app2.enqueue_all_videos(&videos);
    assert_eq!((added, rejected), (3, 0));
    assert_eq!(app2.queue.data.len(), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn library_collections_crud_rename_delete_filter_and_remove_headless() {
    let dir = std::env::temp_dir().join(format!("fasttube-lib-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib_path = dir.join("library.json");

    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    app.library.path = Some(lib_path.clone());
    let videos = three_youtube_videos();
    app.library.data.toggle_watch_later(videos[0].clone());
    app.library.data.toggle_favorite(videos[1].clone());
    app.library.collection_name = "Estudos Rust".into();

    let size = egui::vec2(1000.0, 900.0);
    let palette = app.palette();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    out.textures_delta.clear();
    let create_pos =
        find_rendered_text_center(&out.shapes, "Criar").expect("Criar button rendered");
    let _ = headless_pointer_click(&ctx, size, create_pos, |ui| {
        let _ = app.browse_library_ui(ui, &ctx, palette);
    });
    assert_eq!(app.library.data.collections.len(), 1);
    assert_eq!(app.library.data.collections[0].name, "Estudos Rust");
    app.library.data.collections[0]
        .videos
        .push(library::VideoRecord {
            video: videos[2].clone(),
        });

    // Trigger inline rename
    out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    out.textures_delta.clear();
    let rename_pos =
        find_rendered_text_center(&out.shapes, "Renomear").expect("Renomear button rendered");
    let _ = headless_pointer_click(&ctx, size, rename_pos, |ui| {
        let _ = app.browse_library_ui(ui, &ctx, palette);
    });
    assert_eq!(app.library.renaming_collection, Some(0));
    app.library.rename_draft = "Rust Avançado".into();
    out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    out.textures_delta.clear();
    let save_pos = find_rendered_text_center(&out.shapes, "Salvar").expect("Salvar button");
    let _ = headless_pointer_click(&ctx, size, save_pos, |ui| {
        let _ = app.browse_library_ui(ui, &ctx, palette);
    });
    assert_eq!(app.library.data.collections[0].name, "Rust Avançado");

    // Library search filter isolates matching collection/video
    app.library.search_query = "Rust Avançado".into();
    let mut filtered_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    let filtered_texts = collect_rendered_text(&filtered_out.shapes);
    filtered_out.textures_delta.clear();
    assert!(filtered_texts.iter().any(|t| t.contains("Rust Avançado")));
    assert!(
        filtered_texts
            .iter()
            .any(|t| t.contains("Nenhum item em Assistir mais tarde corresponde"))
    );
    app.library.search_query.clear();

    // Remove individual item from Watch Later via button
    out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    out.textures_delta.clear();
    let remove_pos =
        find_rendered_text_center(&out.shapes, "✕ Remover").expect("Remover button rendered");
    let _ = headless_pointer_click(&ctx, size, remove_pos, |ui| {
        let _ = app.browse_library_ui(ui, &ctx, palette);
    });
    assert!(app.library.data.watch_later.is_empty());

    // Inline delete collection with confirmation
    out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    out.textures_delta.clear();
    let del_pos =
        find_rendered_text_center(&out.shapes, "Excluir coleção").expect("Excluir coleção button");
    let _ = headless_pointer_click(&ctx, size, del_pos, |ui| {
        let _ = app.browse_library_ui(ui, &ctx, palette);
    });
    assert_eq!(app.library.confirm_delete_collection, Some(0));
    out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    out.textures_delta.clear();
    let confirm_pos = find_rendered_text_center(&out.shapes, "Confirmar exclusão")
        .expect("Confirmar exclusão button");
    let _ = headless_pointer_click(&ctx, size, confirm_pos, |ui| {
        let _ = app.browse_library_ui(ui, &ctx, palette);
    });
    assert!(app.library.data.collections.is_empty());
    app.flush_persistence();
    let persisted = library::load(&lib_path).expect("library saved on disk");
    assert!(persisted.collections.is_empty());
    assert!(persisted.watch_later.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn library_bookmark_seek_without_reload_and_markdown_export_headless() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let videos = valid_youtube_videos();
    app.selected = Some(videos[0].clone());
    app.activate_video_playback(&videos[0], None, None, Instant::now());

    app.library
        .data
        .add_bookmark(library::Bookmark {
            video_id: "dQw4w9WgXcQ".into(),
            title: videos[0].title.clone(),
            time_ms: 65_000,
            note: "Refrão".into(),
        })
        .unwrap();
    app.library
        .data
        .add_bookmark(library::Bookmark {
            video_id: "M7lc1UVf-VE".into(),
            title: videos[1].title.clone(),
            time_ms: 12_000,
            note: "Demo API".into(),
        })
        .unwrap();

    // Clicking bookmark for currently playing video seeks immediately without reload
    let same_mark = app.library.data.bookmarks[0].clone();
    assert!(app.open_library_bookmark(&same_mark).is_none());
    assert!(app.library.pending_bookmark_seek.is_none());
    assert_eq!(
        app.playback.active.as_ref().map(|a| a.position_ms),
        Some(65_000)
    );

    // Clicking bookmark for a different video queues pending_bookmark_seek & returns Video
    let other_mark = app.library.data.bookmarks[1].clone();
    let chosen = app
        .open_library_bookmark(&other_mark)
        .expect("should return target video");
    assert_eq!(
        history::youtube_video_id(&chosen.url).as_deref(),
        Some("M7lc1UVf-VE")
    );
    assert_eq!(
        app.library.pending_bookmark_seek,
        Some(("M7lc1UVf-VE".into(), 12_000))
    );

    // Filter bookmarks + copy Markdown button in Library UI
    app.library.bookmark_filter = "Refrão".into();
    let size = egui::vec2(1000.0, 900.0);
    let palette = app.palette();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.browse_library_ui(ui, &ctx, palette);
        },
    );
    let texts = collect_rendered_text(&out.shapes);
    out.textures_delta.clear();
    assert!(texts.iter().any(|t| t.contains("Refrão")));
    assert!(!texts.iter().any(|t| t.contains("Demo API")));

    let copy_pos = find_rendered_text_center(&out.shapes, "Copiar marcadores (Markdown)")
        .expect("Copiar marcadores button");
    let _ = headless_pointer_click(&ctx, size, copy_pos, |ui| {
        let _ = app.browse_library_ui(ui, &ctx, palette);
    });
    assert!(
        app.library
            .notice
            .as_deref()
            .unwrap_or("")
            .contains("1 marcador(es) copiado(s)")
    );
}

#[test]
fn history_search_filter_date_grouping_stats_and_remove_headless() {
    let dir = std::env::temp_dir().join(format!("fasttube-hist-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let hist_path = dir.join("history.json");

    let mut app = FastTube::default();
    app.history.path = Some(hist_path.clone());
    app.history.loaded_path = Some(hist_path.clone());
    app.history.read_failed = false;
    app.history.data = history::History::default();

    let now_ms = history::current_unix_epoch_ms();
    let day_ms = 86_400_000u64;
    let videos = three_youtube_videos();

    app.history
        .data
        .record(history::Entry {
            video_id: "dQw4w9WgXcQ".into(),
            title: "Aula Rust Hoje".into(),
            channel: "Canal Alfa".into(),
            position_ms: 50_000,
            duration_ms: Some(200_000), // InProgress
            watched_at_ms: Some(now_ms),
            ..Default::default()
        })
        .unwrap();
    app.history
        .data
        .record(history::Entry {
            video_id: "M7lc1UVf-VE".into(),
            title: "Tutorial Ontem Concluído".into(),
            channel: "Canal Alfa".into(),
            position_ms: 195_000,
            duration_ms: Some(200_000), // Completed (near end)
            watched_at_ms: Some(now_ms.saturating_sub(day_ms)),
            ..Default::default()
        })
        .unwrap();
    app.history
        .data
        .record(history::Entry {
            video_id: "aqz-KE-bpKQ".into(),
            title: videos[2].title.clone(),
            channel: "Canal Beta".into(),
            position_ms: 10_000,
            duration_ms: Some(200_000),
            watched_at_ms: Some(now_ms.saturating_sub(14 * day_ms)), // Mais antigos
            ..Default::default()
        })
        .unwrap();

    let ctx = egui::Context::default();
    let size = egui::vec2(900.0, 800.0);
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_full_test", false, None, None);
        },
    );
    let texts = collect_rendered_text(&out.shapes);
    out.textures_delta.clear();

    assert!(texts.iter().any(|t| t == "Hoje"));
    assert!(texts.iter().any(|t| t == "Ontem"));
    assert!(texts.iter().any(|t| t == "Mais antigos"));
    for expected in [
        "Tudo",
        "V\u{00ed}deos",
        "Shorts",
        "Podcasts",
        "M\u{00fa}sica",
        "Limpar todo o hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o",
        "Pausar o hist\u{00f3}rico de visualiza\u{00e7}\u{00f5}es",
        "Gerenciar hist\u{00f3}rico",
        "Coment\u{00e1}rios",
        "Postagens",
        "Chat ao vivo",
    ] {
        assert!(
            texts.iter().any(|t| t == expected),
            "expected official YouTube history UI label '{expected}', got:\n{}",
            texts.join("\n")
        );
    }

    // Filter chip "Completed" + category filter "Shorts" + search query
    app.history.filter = history::HistoryFilter::Completed;
    let mut completed_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_full_test", false, None, None);
        },
    );
    let completed_texts = collect_rendered_text(&completed_out.shapes);
    completed_out.textures_delta.clear();
    assert!(
        completed_texts
            .iter()
            .any(|t| t.contains("Tutorial Ontem Concluído"))
    );
    assert!(!completed_texts.iter().any(|t| t.contains("Aula Rust Hoje")));

    app.history.filter = history::HistoryFilter::Shorts;
    let mut shorts_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_full_test", false, None, None);
        },
    );
    let shorts_texts = collect_rendered_text(&shorts_out.shapes);
    shorts_out.textures_delta.clear();
    assert!(!shorts_texts.iter().any(|t| t.contains("Aula Rust Hoje")));

    app.history.filter = history::HistoryFilter::All;
    app.history.search_query = "Canal Beta".into();
    let mut search_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_full_test", false, None, None);
        },
    );
    let search_texts = collect_rendered_text(&search_out.shapes);
    search_out.textures_delta.clear();
    assert!(search_texts.iter().any(|t| t.contains(&videos[2].title)));
    assert!(!search_texts.iter().any(|t| t.contains("Aula Rust Hoje")));

    // Remove visible entry via ⋮ menu -> "Remover do histórico de exibição"
    let dots_pos =
        find_more_dots_center(&search_out.shapes, 600.0).expect("more dots button on entry");
    let mut menu_out = headless_pointer_click(&ctx, size, dots_pos, |ui| {
        let _ = app.history_section_ui(ui, "hist_full_test", false, None, None);
    });
    if find_rendered_text_center(
        &menu_out.shapes,
        "Remover do hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o",
    )
    .is_none()
    {
        menu_out.textures_delta.clear();
        menu_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ui| {
                let _ = app.history_section_ui(ui, "hist_full_test", false, None, None);
            },
        );
    }
    let remove_pos = find_rendered_text_center(
        &menu_out.shapes,
        "Remover do hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o",
    )
    .expect("remove menu item on entry");
    menu_out.textures_delta.clear();
    let _ = headless_pointer_click(&ctx, size, remove_pos, |ui| {
        let _ = app.history_section_ui(ui, "hist_full_test", false, None, None);
    });
    assert!(app.history.data.get("aqz-KE-bpKQ").is_none());
    assert_eq!(app.history.data.entries().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_description_chapters_and_transcript_export_headless() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = valid_youtube_videos()[0].clone();
    app.selected = Some(video.clone());
    app.video_metadata.insert(
        video.url.clone(),
        VideoMetadata {
            description: Some(
                "Guia completo:\n00:00 Abertura\n01:30 Configuração inicial\n04:45 Conclusão"
                    .into(),
            ),
            ..Default::default()
        },
    );
    app.ensure_watch_metadata_cache(&video);
    app.watch.metadata_cache.description_expanded = true;
    assert_eq!(app.watch.metadata_cache.description_chapters.len(), 3);

    // Fallback to description chapters when libmpv has no embedded chapters
    let collected = app.collect_player_chapters();
    assert_eq!(collected.len(), 3);
    assert_eq!(collected[1].title, "Configuração inicial");
    assert_eq!(collected[1].time_secs, 90.0);

    // Subtitles UI renders Transcript header, search pill, inline chapter, Copy/Export inside ⋮ menu, without old Legendas/Trecho atual bloat
    app.watch.extras.transcript = vec![
        transcript::Cue {
            start_ms: 5_000,
            end_ms: 9_000,
            text: "Bem-vindo ao vídeo".into(),
        },
        transcript::Cue {
            start_ms: 65_000,
            end_ms: 70_000,
            text: "Segundo trecho".into(),
        },
    ];
    let size = egui::vec2(900.0, 900.0);
    let palette = app.palette();
    let draw_transcript_case = |app: &mut FastTube, ui: &mut egui::Ui| {
        let ctx = ui.ctx().clone();
        app.watch_sidebar_tabs_ui(ui, &ctx, &palette);
        app.watch_metadata_description_ui(ui, &video.url, 600.0, &palette);
        app.subtitles_ui(ui, &ctx, "test_transcript_export");
    };
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| draw_transcript_case(&mut app, ui),
    );
    let texts = collect_rendered_text(&out.shapes);
    let more_pos = find_more_dots_center(&out.shapes, size.x).expect("⋮ overflow button");
    out.textures_delta.clear();

    assert!(texts.iter().any(|t| t == "Transcrição"));
    assert!(texts.iter().any(|t| t == "Pesquisar transcrição"));
    assert!(texts.iter().any(|t| t == "Abertura"));
    assert!(texts.iter().any(|t| t.contains("Capítulos (3)")));
    assert!(texts.iter().any(|t| t.contains("01:30")));
    assert!(texts.iter().any(|t| t.contains("Configuração inicial")));
    assert!(!texts.iter().any(|t| t == "Legendas"));
    assert!(!texts.iter().any(|t| t == "Trecho atual"));

    // Open ⋮ menu and click "Copiar transcrição"
    let mut menu_out = headless_pointer_click(&ctx, size, more_pos, |ui| {
        draw_transcript_case(&mut app, ui);
    });
    if find_rendered_text_center(&menu_out.shapes, "Ocultar carimbos de data/hora").is_none() {
        menu_out.textures_delta.clear();
        menu_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ui| draw_transcript_case(&mut app, ui),
        );
    }
    let menu_texts = collect_rendered_text(&menu_out.shapes);
    assert!(
        menu_texts
            .iter()
            .any(|t| t == "Ocultar carimbos de data/hora")
    );
    assert!(menu_texts.iter().any(|t| t == "Copiar transcrição"));
    assert!(menu_texts.iter().any(|t| t == "Exportar (.txt / .md)"));

    let copy_pos =
        find_rendered_text_center(&menu_out.shapes, "Copiar transcrição").expect("Copy menu item");
    menu_out.textures_delta.clear();
    let _ = headless_pointer_click(&ctx, size, copy_pos, |ui| {
        draw_transcript_case(&mut app, ui);
    });
    assert!(matches!(
        &app.watch.extras.transcript_export_status,
        Some((true, msg)) if msg.contains("2 trecho(s) copiados")
    ));
}

#[test]
fn v030_downloads_effective_dir_and_formats() {
    let default_dir = downloads::DownloadManager::default_download_dir();
    assert_eq!(
        downloads::DownloadManager::effective_download_dir(None),
        default_dir
    );
    assert_eq!(
        downloads::DownloadManager::effective_download_dir(Some("relative/path")),
        default_dir
    );
    #[cfg(windows)]
    {
        let custom = PathBuf::from(r"C:\FastTubeDownloads");
        assert_eq!(
            downloads::DownloadManager::effective_download_dir(Some(r"C:\FastTubeDownloads")),
            custom
        );
    }
    for fmt in downloads::DownloadFormat::ALL {
        assert!(!fmt.label_pt().is_empty());
        assert!(!fmt.label_en().is_empty());
        assert!(!fmt.yt_dlp_args().is_empty());
    }
}

#[test]
fn v030_followed_channels_persistence_and_local_insights() {
    let temp = std::env::temp_dir().join(format!(
        "fasttube-v030-followed-insights-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();
    let path = temp.join("followed-channels.json");

    let mut followed = followed_channels::FollowedChannels::default();
    followed.channels.push(followed_channels::FollowedChannel {
        channel_id: "UCuAXFkgsw1L7xaCfnd5JJOw".to_owned(),
        title: "Rick Astley".to_owned(),
        handle: Some("@RickAstleyYT".to_owned()),
        url: "https://www.youtube.com/channel/UCuAXFkgsw1L7xaCfnd5JJOw".to_owned(),
        avatar_url: None,
        followed_at_ms: 1_700_000_000_000,
        last_seen_video_id: Some("dQw4w9WgXcQ".to_owned()),
    });
    followed_channels::save(&path, &followed).unwrap();
    let loaded = followed_channels::load(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert!(loaded.is_following("UCuAXFkgsw1L7xaCfnd5JJOw"));
    assert!(loaded.is_following("@rickastleyyt"));

    let mut hist = History::default();
    hist.record(history::Entry {
        video_id: "dQw4w9WgXcQ".to_owned(),
        title: "Never Gonna Give You Up".to_owned(),
        channel: "Rick Astley".to_owned(),
        position_ms: 210_000,
        duration_ms: Some(213_000),
        watched_at_ms: Some(1_700_000_000_000),
        ..Default::default()
    })
    .unwrap();
    let lib = library::Library::default();
    let mut profile = UserProfile::default();
    profile.token_weights.insert("music".to_owned(), 2.5);
    let summary = insights::compute_local_insights(&hist, &lib, &profile, &loaded);
    assert_eq!(summary.total_videos, 1);
    assert_eq!(summary.completed_videos, 1);
    assert_eq!(summary.followed_channels_count, 1);
    assert_eq!(summary.top_channels[0].channel, "Rick Astley");
    assert_eq!(summary.top_topics[0].0, "music");

    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn v030_protocol_url_and_bilingual_downloads_insights_ui() {
    assert_eq!(
        desktop::normalize_fasttube_protocol_url("fasttube://watch?v=dQw4w9WgXcQ"),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned())
    );
    assert_eq!(
        desktop::normalize_fasttube_protocol_url("fasttube://https://youtu.be/dQw4w9WgXcQ"),
        Some("https://youtu.be/dQw4w9WgXcQ".to_owned())
    );
    assert_eq!(
        desktop::normalize_fasttube_protocol_url("javascript:alert(1)"),
        None
    );

    let mut app = FastTube::default();
    assert_eq!(app.pick("Início", "Home"), "Início");
    app.settings.prefs.value.language = i18n::UiLanguage::En;
    assert_eq!(app.pick("Início", "Home"), "Home");

    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.downloads_section_ui(ui, &c);
            app.insights_section_ui(ui, &c);
        },
    );
    out.textures_delta.clear();
}

#[test]
fn playlists_watch_later_and_liked_pages_1to1_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let videos = three_youtube_videos();
    app.library.data.toggle_watch_later(videos[0].clone());
    app.library.data.toggle_watch_later(videos[1].clone());
    app.library.data.toggle_favorite(videos[2].clone());
    let col_idx = app
        .library
        .data
        .create_collection("Estudos")
        .expect("create Estudos collection");
    app.library
        .data
        .collections
        .get_mut(col_idx)
        .expect("Estudos collection")
        .videos
        .push(crate::library::VideoRecord {
            video: videos[0].clone(),
        });

    let palette = app.palette();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));

    let mut pl_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.playlists_page_ui(ui, &c, palette);
        },
    );
    let pl_text = collect_rendered_text(&pl_out.shapes).join("\n");
    pl_out.textures_delta.clear();
    for expected in [
        "Playlists",
        "Adicionadas recentemente",
        "Música",
        "Mixes",
        "Minhas",
        "Salvas",
        "Assistir mais tarde",
        "Vídeos com \"Gostei\"",
        "Estudos",
    ] {
        assert!(
            pl_text.contains(expected),
            "expected '{expected}' on Playlists page, got:\n{pl_text}"
        );
    }

    app.library.selected_collection_idx = Some(col_idx);
    let mut detail_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.playlists_page_ui(ui, &c, palette);
        },
    );
    let detail_text = collect_rendered_text(&detail_out.shapes).join("\n");
    detail_out.textures_delta.clear();
    for expected in [
        "‹  Playlists",
        "▶  Reproduzir tudo",
        "🔀 Aleatório",
        "+ Fila",
        "Estudos",
    ] {
        assert!(
            detail_text.contains(expected),
            "expected '{expected}' in collection detail view, got:\n{detail_text}"
        );
    }

    let mut wl_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_later_page_ui(ui, &c, palette);
        },
    );
    let wl_text = collect_rendered_text(&wl_out.shapes).join("\n");
    wl_out.textures_delta.clear();
    assert!(
        wl_text.contains("Assistir mais tarde")
            && wl_text.contains("Tudo")
            && wl_text.contains("Shorts")
            && wl_text.contains(&videos[0].title),
        "expected Watch Later 1:1 detail layout, got:\n{wl_text}"
    );

    let mut liked_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.liked_videos_page_ui(ui, &c, palette);
        },
    );
    let liked_text = collect_rendered_text(&liked_out.shapes).join("\n");
    liked_out.textures_delta.clear();
    assert!(
        liked_text.contains("Vídeos com \"Gostei\"")
            && liked_text.contains("Tudo")
            && liked_text.contains("Shorts")
            && liked_text.contains(&videos[2].title),
        "expected Liked Videos 1:1 detail layout, got:\n{liked_text}"
    );
}

#[test]
fn liked_videos_page_renders_5000_count_and_shorts_grid_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    let short_item = SearchResult {
        video: Video {
            title: "FIZEMOS o ALHO NEGRO de R$ 500!".into(),
            channel: "Manual do Mundo".into(),
            url: "https://www.youtube.com/shorts/f0UuuLmAhQk".into(),
            thumbnail: "https://i.ytimg.com/vi/f0UuuLmAhQk/oardefault.jpg".into(),
            duration: "2:27".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(461_000),
            kind: search::SearchResultKind::Short,
            ..Default::default()
        },
    };
    let regular_short_duration = SearchResult {
        video: Video {
            title: "Me at the zoo".into(),
            channel: "jawed".into(),
            url: "https://www.youtube.com/watch?v=jNQXAC9IVRw".into(),
            thumbnail: "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg".into(),
            duration: "0:19".into(),
        },
        metadata: VideoMetadata {
            view_count: Some(300_000_000),
            kind: search::SearchResultKind::Video,
            ..Default::default()
        },
    };

    app.video_metadata
        .insert(short_item.video.url.clone(), short_item.metadata.clone());
    app.video_metadata.insert(
        regular_short_duration.video.url.clone(),
        regular_short_duration.metadata.clone(),
    );
    app.library.liked_videos.total_results = Some(5_000);
    app.library.liked_videos.loaded = true;
    app.library.liked_videos.items = vec![short_item.clone(), regular_short_duration.clone()];
    app.library.liked_videos.revision = 1;

    let palette = app.palette();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));

    app.library.detail_chip_filter = 0;
    let mut all_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.liked_videos_page_ui(ui, &c, palette);
        },
    );
    let all_text = collect_rendered_text(&all_out.shapes).join("\n");
    all_out.textures_delta.clear();
    assert!(
        all_text.contains("5.000 vídeos")
            && all_text.contains(&short_item.video.title)
            && all_text.contains(&regular_short_duration.video.title),
        "expected 5.000 vídeos and both items in Tudo tab, got:\n{all_text}"
    );

    app.library.detail_chip_filter = 1;
    let mut videos_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.liked_videos_page_ui(ui, &c, palette);
        },
    );
    let videos_text = collect_rendered_text(&videos_out.shapes).join("\n");
    videos_out.textures_delta.clear();
    assert!(
        videos_text.contains(&regular_short_duration.video.title)
            && !videos_text.contains(&short_item.video.title),
        "expected Vídeos tab to include 0:19 regular video and exclude 2:27 Short, got:\n{videos_text}"
    );

    app.library.detail_chip_filter = 2;
    let mut shorts_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.liked_videos_page_ui(ui, &c, palette);
        },
    );
    let shorts_lines = collect_rendered_text(&shorts_out.shapes);
    let shorts_text = shorts_lines.join("\n");
    shorts_out.textures_delta.clear();
    assert!(
        shorts_text.contains(&short_item.video.title)
            && shorts_text.contains("461 mil visualizações")
            && !shorts_text.contains(&regular_short_duration.video.title)
            && !shorts_lines.iter().any(|line| line.trim() == "1"),
        "expected Shorts grid with 2:27 Short, compact views, no 0:19 non-Short, and no row numbering, got:\n{shorts_text}"
    );
}

#[test]
fn poll_liked_videos_streams_batches_ignores_stale_requests_and_resets_on_sign_out() {
    let mut app = FastTube::default();
    app.auth.authenticated = true;
    app.auth.session = Some(oauth::TokenSession::test_session("tok"));

    let (tx, rx) = mpsc::channel();
    app.library.liked_videos.active_request_id = Some(2);
    app.library.liked_videos.rx = Some(rx);

    tx.send(LikedVideosFetchMessage::Progress {
        request_id: 1,
        batch: vec![SearchResult {
            video: Video {
                title: "Stale Short".into(),
                channel: "Old".into(),
                url: "https://www.youtube.com/shorts/stale000001".into(),
                thumbnail: String::new(),
                duration: "0:30".into(),
            },
            metadata: VideoMetadata {
                kind: search::SearchResultKind::Short,
                ..Default::default()
            },
        }],
        total_results: Some(99),
    })
    .unwrap();

    tx.send(LikedVideosFetchMessage::Progress {
        request_id: 2,
        batch: vec![SearchResult {
            video: Video {
                title: "Fresh Short".into(),
                channel: "Creator".into(),
                url: "https://www.youtube.com/shorts/fresh000001".into(),
                thumbnail: "https://i.ytimg.com/vi/fresh000001/oardefault.jpg".into(),
                duration: "1:45".into(),
            },
            metadata: VideoMetadata {
                view_count: Some(181_000),
                kind: search::SearchResultKind::Short,
                ..Default::default()
            },
        }],
        total_results: Some(5_000),
    })
    .unwrap();

    tx.send(LikedVideosFetchMessage::Finished {
        request_id: 2,
        token: Some(oauth::TokenSession::test_session("tok")),
        total_results: Some(5_000),
        result: Ok(()),
    })
    .unwrap();

    app.poll_liked_videos();

    assert!(app.library.liked_videos.loaded);
    assert_eq!(app.library.liked_videos.total_results, Some(5_000));
    assert_eq!(app.library.liked_videos.items.len(), 1);
    assert_eq!(app.library.liked_videos.items[0].video.title, "Fresh Short");
    assert_eq!(
        app.video_metadata
            .get("https://www.youtube.com/shorts/fresh000001")
            .and_then(|m| m.view_count),
        Some(181_000)
    );

    app.sign_out_and_reload_home(&egui::Context::default());
    assert!(!app.library.liked_videos.loaded);
    assert!(app.library.liked_videos.items.is_empty());
    assert_eq!(app.library.liked_videos.total_results, None);
}

#[test]
fn downloads_two_column_page_and_direct_input_headless() {
    assert!(library_ui::parse_direct_youtube_input("").is_none());
    assert!(library_ui::parse_direct_youtube_input("not a video").is_none());
    for raw in [
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQ",
        "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        "dQw4w9WgXcQ",
    ] {
        let parsed = library_ui::parse_direct_youtube_input(raw).expect("valid YouTube video");
        assert_eq!(parsed.url, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    }

    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.downloads_section_ui(ui, &c);
        },
    );
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();
    for expected in [
        "Downloads",
        "Salvos localmente via yt-dlp",
        "Tudo",
        "Vídeos",
        "Áudio",
        "Em andamento",
        "Concluídos",
        "⬇ Baixar",
        "Abrir pasta",
    ] {
        assert!(
            rendered.contains(expected),
            "expected '{expected}' on Downloads page, got:\n{rendered}"
        );
    }
}

#[test]
fn report_history_and_youtube_labs_pages_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let palette = app.palette();
    app.feed
        .report_history
        .push(app_state::ReportedContentEntry {
            kind: "Vídeo".into(),
            title: "Vídeo de Teste".into(),
            channel: "Canal X".into(),
            video_id: Some("dQw4w9WgXcQ".into()),
            reason: "Problema na imagem ou no título > Conteúdo violento".into(),
            timestamp_label: "20 de nov. de 2026".into(),
            status: "Ativo".into(),
        });

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut report_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.report_history_page_ui(ui, &c, palette);
        },
    );
    let report_text = collect_rendered_text(&report_out.shapes).join("\n");
    report_out.textures_delta.clear();
    for expected in [
        "Agradecemos pela denúncia",
        "Tipo",
        "Conteúdo",
        "Motivo da denúncia",
        "Status",
        "Vídeo de Teste",
        "Problema na imagem ou no título > Conteúdo violento",
        "Ativo",
    ] {
        assert!(
            report_text.contains(expected),
            "expected '{expected}' on Report history page, got:\n{report_text}"
        );
    }

    let mut labs_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            app.youtube_labs_page_ui(ui, &c, palette);
        },
    );
    let labs_text = collect_rendered_text(&labs_out.shapes).join("\n");
    labs_out.textures_delta.clear();
    for expected in [
        "YouTube Labs",
        "SponsorBlock",
        "Return YouTube Dislike",
        "DeArrow",
        "Miniplayer automático",
    ] {
        assert!(
            labs_text.contains(expected),
            "expected '{expected}' on YouTube Labs page, got:\n{labs_text}"
        );
    }
}

#[test]
fn transcript_panel_matches_youtube_grouping_chapters_search_and_width() {
    let ctx = egui::Context::default();
    let mut app = FastTube::default();
    let video = valid_youtube_videos()[0].clone();
    app.selected = Some(video.clone());
    app.watch.live.video_url = Some(video.url.clone());
    app.watch.live.info = Some(live_chat::WatchLiveInfo {
        video_id: "dQw4w9WgXcQ".into(),
        chapters: vec![
            overlay::OverlayChapter {
                title: "Capítulo 1: Introduction to The Strokes".into(),
                time_secs: 0.0,
            },
            overlay::OverlayChapter {
                title: "Capítulo 2: Context of the new single".into(),
                time_secs: 33.0,
            },
        ],
        ..Default::default()
    });
    app.watch.extras.subtitles_loaded = true;
    app.watch.extras.transcript_auto_attempted = true;
    app.watch.extras.open_transcript();
    app.watch.extras.transcript = vec![
        transcript::Cue {
            start_ms: 0,
            end_ms: 4_500,
            text: "Hey buddy, did you hear the news? It's".into(),
        },
        transcript::Cue {
            start_ms: 4_500,
            end_ms: 8_800,
            text: "Track Review.".into(),
        },
        transcript::Cue {
            start_ms: 9_000,
            end_ms: 12_500,
            text: "Hi everyone, Anthony Fantano".into(),
        },
        transcript::Cue {
            start_ms: 12_500,
            end_ms: 15_900,
            text: "here, the internet's busiest music nerd and".into(),
        },
        transcript::Cue {
            start_ms: 16_000,
            end_ms: 20_000,
            text: "it's time for Track Reviews where we talk about".into(),
        },
        transcript::Cue {
            start_ms: 20_000,
            end_ms: 23_900,
            text: "the latest in rock pop electronic and experimental music. And".into(),
        },
        transcript::Cue {
            start_ms: 24_000,
            end_ms: 28_000,
            text: "yeah, that's about it. Today on Track Reviews, brand new single.".into(),
        },
        transcript::Cue {
            start_ms: 33_000,
            end_ms: 38_000,
            text: "Strips. The Strokes. Context section.".into(),
        },
    ];

    let sidebar_size = egui::vec2(402.0, 760.0);
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, sidebar_size)),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_sidebar_results_ui(ui, &c, "yt_parity_transcript");
        },
    );
    let text_rects = collect_rendered_text_rects(&out.shapes);
    let rendered: Vec<String> = text_rects.iter().map(|(t, _)| t.clone()).collect();
    out.textures_delta.clear();

    // Grouped timestamps & inline chapters
    assert!(rendered.iter().any(|t| t == "0:00"));
    assert!(rendered.iter().any(|t| t == "0:09"));
    assert!(rendered.iter().any(|t| t == "0:16"));
    assert!(rendered.iter().any(|t| t == "0:24"));
    assert!(rendered.iter().any(|t| t == "0:33"));
    assert!(!rendered.iter().any(|t| t == "0:12"));
    assert!(!rendered.iter().any(|t| t == "0:20"));
    assert!(
        rendered
            .iter()
            .any(|t| t == "Capítulo 1: Introduction to The Strokes")
    );
    assert!(
        rendered
            .iter()
            .any(|t| t == "Capítulo 2: Context of the new single")
    );

    // Every rendered text box stays inside the 402px sidebar without horizontal overflow.
    for (text, rect) in &text_rects {
        assert!(
            rect.right() <= sidebar_size.x + 1.0,
            "text '{text}' overflowed 402px sidebar: right={}",
            rect.right()
        );
    }

    // Clicking a segment seeks playback.
    app.activate_video_playback(&video, None, None, Instant::now());
    let seg_pos = text_rects
        .iter()
        .find(|(t, _)| t.contains("Anthony Fantano"))
        .map(|(_, r)| r.center())
        .expect("0:09 segment");
    let _ = headless_pointer_click(&ctx, sidebar_size, seg_pos, |ui| {
        let c = ui.ctx().clone();
        let _ = app.watch_sidebar_results_ui(ui, &c, "yt_parity_transcript");
    });
    assert_eq!(
        app.playback.active.as_ref().map(|a| a.position_ms),
        Some(9_000)
    );

    // Filtering narrows rendered segments to matches.
    app.watch.extras.transcript_query = "busiest".into();
    let mut search_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, sidebar_size)),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_sidebar_results_ui(ui, &c, "yt_parity_transcript");
        },
    );
    let search_texts = collect_rendered_text(&search_out.shapes);
    search_out.textures_delta.clear();
    assert!(search_texts.iter().any(|t| t.contains("Anthony Fantano")));
    assert!(!search_texts.iter().any(|t| t.contains("Hey buddy")));

    // Hiding timestamps omits timestamp pills.
    app.watch.extras.transcript_query.clear();
    app.watch.extras.transcript_show_timestamps = false;
    let mut no_ts_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, sidebar_size)),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_sidebar_results_ui(ui, &c, "yt_parity_transcript");
        },
    );
    let no_ts_texts = collect_rendered_text(&no_ts_out.shapes);
    no_ts_out.textures_delta.clear();
    assert!(!no_ts_texts.iter().any(|t| t == "0:00"));
    assert!(no_ts_texts.iter().any(|t| t.contains("Hey buddy")));
}

#[test]
fn history_categories_shorts_shelf_music_badge_and_access_sync_headless() {
    let dir = std::env::temp_dir().join(format!("fasttube-hist-cat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let hist_path = dir.join("history.json");

    let mut app = FastTube::default();
    app.history.path = Some(hist_path.clone());
    app.history.loaded_path = Some(hist_path.clone());
    app.history.read_failed = false;
    app.history.data = history::History::default();

    let now_ms = history::current_unix_epoch_ms();
    // Long gameplay videos (33:34, 46:53, 1:37:28) must NEVER be misclassified as Podcasts.
    for (idx, (id, title, channel, dur_ms)) in [
        (
            "gta6vid0001",
            "GTA 6 \u{00c9} INCR\u{00cd}VEL",
            "pescocofino",
            2_014_000u64,
        ),
        (
            "gta6vid0002",
            "GTA 6 - #12: O \u{00da}ltimo Rol\u{00ea}",
            "Funky Black Cat",
            2_813_000u64,
        ),
        (
            "gta6vid0003",
            "GTA 6 - #11: Essa miss\u{00e3}o foi uma PORCARIA",
            "Funky Black Cat",
            5_848_000u64,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        app.history
            .data
            .record(history::Entry {
                video_id: id.into(),
                title: title.into(),
                channel: channel.into(),
                position_ms: 0,
                duration_ms: Some(dur_ms),
                watched_at_ms: Some(now_ms.saturating_sub(idx as u64 * 1_000)),
                is_short: Some(false),
                is_podcast: Some(false),
                is_music: Some(false),
                ..Default::default()
            })
            .unwrap();
    }

    // Podcast flagged authoritatively without needing the word "podcast" in the title.
    app.history
        .data
        .record(history::Entry {
            video_id: "podcast0001".into(),
            title: "\u{3010}ASMR / 4K\u{3011} Ear Cleaning \u{3010}EllaFreya\u{3011}".into(),
            channel: "La'Joule / ASMR".into(),
            position_ms: 120_000,
            duration_ms: Some(982_000),
            watched_at_ms: Some(now_ms.saturating_sub(5_000)),
            is_short: Some(false),
            is_podcast: Some(true),
            is_music: Some(false),
            ..Default::default()
        })
        .unwrap();

    // Music video flagged authoritatively without music keywords in the title.
    app.history
        .data
        .record(history::Entry {
            video_id: "music000001".into(),
            title: "The Strokes - One Way Trigger (Live at the Forum)".into(),
            channel: "Steely Dang".into(),
            position_ms: 60_000,
            duration_ms: Some(342_000),
            watched_at_ms: Some(now_ms.saturating_sub(6_000)),
            is_short: Some(false),
            is_podcast: Some(false),
            is_music: Some(true),
            ..Default::default()
        })
        .unwrap();

    // Four consecutive Shorts in "Hoje" so Shorts carousel arrow '>' renders at width 1100.
    for idx in (1..=4usize).rev() {
        app.history
            .data
            .record(history::Entry {
                video_id: format!("short00000{idx}"),
                title: format!("Short Destaque {idx}"),
                channel: "Canal Shorts".into(),
                position_ms: 0,
                duration_ms: Some(25_000),
                watched_at_ms: Some(now_ms.saturating_sub(10_000 + idx as u64 * 1_000)),
                is_short: Some(true),
                is_podcast: Some(false),
                is_music: Some(false),
                ..Default::default()
            })
            .unwrap();
    }

    let ctx = egui::Context::default();
    let size = egui::vec2(1100.0, 800.0);

    // 1. Podcasts filter shows the authoritative podcast and excludes long GTA 6 videos.
    app.history.filter = history::HistoryFilter::Podcasts;
    let mut pod_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_cat_test", false, None, None);
        },
    );
    let pod_texts = collect_rendered_text(&pod_out.shapes);
    pod_out.textures_delta.clear();
    assert!(
        pod_texts.iter().any(|t| t.contains("Ear Cleaning")),
        "expected authoritative podcast in Podcasts tab, got:\n{}",
        pod_texts.join("\n")
    );
    assert!(
        !pod_texts.iter().any(|t| t.contains("GTA 6")),
        "long GTA 6 videos must not appear in Podcasts tab"
    );

    // 2. Music filter renders authoritative music video with ♫ duration badge.
    app.history.filter = history::HistoryFilter::Music;
    let mut music_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_cat_test", false, None, None);
        },
    );
    let music_texts = collect_rendered_text(&music_out.shapes);
    music_out.textures_delta.clear();
    assert!(music_texts.iter().any(|t| t.contains("One Way Trigger")));
    assert!(
        music_texts.iter().any(|t| t == "\u{266b} 5:42"),
        "expected music note duration badge '\u{266b} 5:42', got:\n{}",
        music_texts.join("\n")
    );

    // 3. Shorts filter renders Shorts shelf carousel and '>' advances shorts_shelf_offsets.
    app.history.filter = history::HistoryFilter::Shorts;
    let mut shorts_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_cat_test", false, None, None);
        },
    );
    let shorts_texts = collect_rendered_text(&shorts_out.shapes);
    let next_arrow =
        find_rendered_text_center(&shorts_out.shapes, ">").expect("Shorts carousel '>' button");
    shorts_out.textures_delta.clear();
    assert!(shorts_texts.iter().any(|t| t.contains("Short Destaque 1")));
    assert!(!shorts_texts.iter().any(|t| t.contains("Short Destaque 4")));

    let mut shorts_click_out = headless_pointer_click(&ctx, size, next_arrow, |ui| {
        let _ = app.history_section_ui(ui, "hist_cat_test", false, None, None);
    });
    shorts_click_out.textures_delta.clear();
    assert_eq!(
        app.history
            .remote
            .shorts_shelf_offsets
            .get("shorts:0")
            .copied(),
        Some(1)
    );
    let mut shorts_next_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_cat_test", false, None, None);
        },
    );
    let shorts_next_texts = collect_rendered_text(&shorts_next_out.shapes);
    shorts_next_out.textures_delta.clear();
    assert!(
        shorts_next_texts
            .iter()
            .any(|t| t.contains("Short Destaque 4"))
    );

    // 4. All filter groups consecutive shorts into a vertical Shorts shelf alongside regular rows.
    app.history.filter = history::HistoryFilter::All;
    let mut all_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        },
        |ui| {
            let _ = app.history_section_ui(ui, "hist_cat_test", false, None, None);
        },
    );
    let all_texts = collect_rendered_text(&all_out.shapes);
    all_out.textures_delta.clear();
    assert!(all_texts.iter().any(|t| t == "Shorts"));
    assert!(all_texts.iter().any(|t| t.contains("GTA 6")));
    assert!(all_texts.iter().any(|t| t.contains("Short Destaque 1")));

    // 5. Accessing a video updates both local history.json and optimistic remote feeds while preserving resume progress.
    app.history.remote.loaded = true;
    app.history.remote.feeds.insert(
        history::HistoryFilter::All,
        app_state::RemoteHistoryFeed {
            loaded: true,
            ..Default::default()
        },
    );
    app.history.remote.feeds.insert(
        history::HistoryFilter::Videos,
        app_state::RemoteHistoryFeed {
            loaded: true,
            ..Default::default()
        },
    );
    let accessed_video = Video {
        title: "Novo V\u{00ed}deo Acessado".into(),
        channel: "Canal Teste".into(),
        duration: "10:00".into(),
        url: "https://www.youtube.com/watch?v=access00001".into(),
        thumbnail: "https://i.ytimg.com/vi/access00001/hqdefault.jpg".into(),
    };
    app.record_video_accessed_in_history(&accessed_video, Some(45_000));
    let local_saved = app
        .history
        .data
        .get("access00001")
        .expect("accessed video persisted locally");
    assert_eq!(local_saved.position_ms, 45_000);
    for filter in [history::HistoryFilter::All, history::HistoryFilter::Videos] {
        let feed = app.history.remote.feeds.get(&filter).unwrap();
        assert!(feed.seen_ids.contains("access00001"));
        assert_eq!(
            feed.sections
                .first()
                .and_then(|s| s.items.first())
                .map(|i| i.video_id.as_str()),
            Some("access00001")
        );
    }

    // Re-accessing an entry with higher existing local progress preserves that position_ms.
    app.record_video_in_history(&accessed_video, 120_000, Some(600_000))
        .unwrap();
    app.record_video_accessed_in_history(&accessed_video, None);
    assert_eq!(
        app.history.data.get("access00001").unwrap().position_ms,
        120_000
    );
    assert_eq!(
        app.history
            .remote
            .feeds
            .get(&history::HistoryFilter::All)
            .unwrap()
            .sections[0]
            .items[0]
            .position_ms,
        120_000
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn playlists_oauth_connect_prompt_and_server_az_continuation_preserve_state_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    app.settings.prefs.value.language = i18n::UiLanguage::PtBr;
    app.settings.theme.value = settings::Theme::Dark;

    let oauth_card = playlist_sync::SyncedPlaylistCard {
        id: "PLoauth001".into(),
        title: "OAuth Playlist".into(),
        thumbnail_url: String::new(),
        video_count: Some(5),
        badge_text: "5 vídeos".into(),
        owner_label: "Canal OAuth".into(),
        subtitle_line: "Canal OAuth · Playlist".into(),
        updated_text: None,
        privacy: playlist_sync::PlaylistPrivacy::Public,
        kind: playlist_sync::PlaylistKind::Playlist,
        is_mix: false,
        is_music: false,
        is_owned: true,
        is_saved: false,
        seed_video_id: None,
        browse_id: Some("VLPLoauth001".into()),
        has_full_playlist_link: true,
        stack_color_dark: None,
        stack_color_light: None,
    };

    let (oauth_req_id, _oauth_cancel, oauth_tx) = app.auth.remote_playlists_task.begin_manual();
    oauth_tx
        .send((oauth_req_id, (None, Ok(vec![oauth_card]))))
        .unwrap();
    app.poll_remote_playlists(&ctx);

    assert!(
        !app.library.remote_playlists.loaded,
        "OAuth playlist preload must not block FEplaylist_aggregation InnerTube sync"
    );
    assert_eq!(app.library.remote_playlists.oauth_items.len(), 1);
    assert_eq!(app.auth.remote_playlists.len(), 1);

    app.auth.authenticated = true;
    app.auth.youtube_web_session = None;
    app.library.remote_playlists.needs_connect = true;

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut connect_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let palette = app.palette();
            let _ = app.playlists_page_ui(ui, &c, palette);
        },
    );
    let connect_rendered = collect_rendered_text(&connect_out.shapes).join("\n");
    connect_out.textures_delta.clear();
    assert!(
        connect_rendered.contains("Conectar YouTube"),
        "expected 'Conectar YouTube' when web session is missing even if OAuth is connected, got:\n{connect_rendered}"
    );
    assert!(connect_rendered.contains("OAuth Playlist"));

    let zulu_card = playlist_sync::SyncedPlaylistCard {
        id: "PLzulu001".into(),
        title: "Zulu Playlist".into(),
        thumbnail_url: "https://i.ytimg.com/vi/zulu0000001/hqdefault.jpg".into(),
        video_count: Some(12),
        badge_text: "12 vídeos".into(),
        owner_label: "@ZuluCreator".into(),
        subtitle_line: "@ZuluCreator · Playlist".into(),
        updated_text: None,
        privacy: playlist_sync::PlaylistPrivacy::Public,
        kind: playlist_sync::PlaylistKind::Playlist,
        is_mix: false,
        is_music: false,
        is_owned: false,
        is_saved: true,
        seed_video_id: Some("zulu0000001".into()),
        browse_id: Some("VLPLzulu001".into()),
        has_full_playlist_link: true,
        stack_color_dark: Some(0xFF4A_3B32),
        stack_color_light: Some(0xFFD8_CFC8),
    };
    let pinned_card = playlist_sync::SyncedPlaylistCard {
        id: "PLpinned002".into(),
        title: "!Pinned First In Server Order".into(),
        thumbnail_url: "https://i.ytimg.com/vi/pin00000002/hqdefault.jpg".into(),
        video_count: Some(4),
        badge_text: "4 vídeos".into(),
        owner_label: "Particular".into(),
        subtitle_line: "Particular · Playlist".into(),
        updated_text: None,
        privacy: playlist_sync::PlaylistPrivacy::Private,
        kind: playlist_sync::PlaylistKind::Playlist,
        is_mix: false,
        is_music: false,
        is_owned: true,
        is_saved: false,
        seed_video_id: Some("pin00000002".into()),
        browse_id: Some("VLPLpinned002".into()),
        has_full_playlist_link: true,
        stack_color_dark: None,
        stack_color_light: None,
    };

    let (pl_tx, pl_rx) = std::sync::mpsc::channel();
    app.library.remote_playlists.active_request_id = Some(1);
    app.library.remote_playlists.active_filter = playlist_sync::PlaylistCategoryFilter::All;
    app.library.remote_playlists.active_sort = playlist_sync::PlaylistSort::RecentlyAdded;
    app.library.remote_playlists.server_sorted_for = None;
    app.library.remote_playlists.rx = Some(pl_rx);

    pl_tx
        .send(playlist_sync::PlaylistsSyncMessage::Page {
            request_id: 1,
            filter: playlist_sync::PlaylistCategoryFilter::All,
            sort: playlist_sync::PlaylistSort::RecentlyAdded,
            page_index: 0,
            page: playlist_sync::RemotePlaylistsFeedPage {
                cards: vec![zulu_card.clone()],
                chips: HashMap::new(),
                sort_options: vec![
                    playlist_sync::PlaylistSortOption {
                        sort: playlist_sync::PlaylistSort::RecentlyAdded,
                        label: "Adicionadas recentemente".into(),
                        action: playlist_sync::PlaylistChipAction::Continuation(
                            "sort_recent".into(),
                        ),
                    },
                    playlist_sync::PlaylistSortOption {
                        sort: playlist_sync::PlaylistSort::AlphabeticalAZ,
                        label: "A-Z".into(),
                        action: playlist_sync::PlaylistChipAction::Continuation("sort_az".into()),
                    },
                ],
                selected_sort: Some(playlist_sync::PlaylistSort::AlphabeticalAZ),
                next_continuation: Some("cont_page_1".into()),
                signed_out: false,
            },
        })
        .unwrap();
    pl_tx
        .send(playlist_sync::PlaylistsSyncMessage::Page {
            request_id: 1,
            filter: playlist_sync::PlaylistCategoryFilter::All,
            sort: playlist_sync::PlaylistSort::RecentlyAdded,
            page_index: 1,
            page: playlist_sync::RemotePlaylistsFeedPage {
                cards: vec![pinned_card.clone()],
                chips: HashMap::new(),
                sort_options: Vec::new(),
                selected_sort: None,
                next_continuation: None,
                signed_out: false,
            },
        })
        .unwrap();
    pl_tx
        .send(playlist_sync::PlaylistsSyncMessage::Finished {
            request_id: 1,
            filter: playlist_sync::PlaylistCategoryFilter::All,
            sort: playlist_sync::PlaylistSort::RecentlyAdded,
            auth_expired: false,
            result: Ok(2),
        })
        .unwrap();

    app.poll_playlists_sync(&ctx);

    assert!(app.library.remote_playlists.loaded);
    assert_eq!(
        app.library.remote_playlists.active_sort,
        playlist_sync::PlaylistSort::AlphabeticalAZ
    );
    assert_eq!(
        app.library.remote_playlists.server_sorted_for,
        Some(playlist_sync::PlaylistSort::AlphabeticalAZ)
    );
    assert_eq!(
        app.library
            .remote_playlists
            .all_items
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["PLzulu001", "PLpinned002"]
    );
    assert_eq!(
        app.library
            .remote_playlists
            .items
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["PLzulu001", "PLpinned002"],
        "server-sorted continuation pages must preserve exact server order without local resorting"
    );

    for theme in [settings::Theme::Dark, settings::Theme::Light] {
        app.settings.theme.value = theme;
        app.sync_theme_visuals(&ctx);
        let palette = app.palette();
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                let _ = app.playlists_page_ui(ui, &c, palette);
            },
        );
        let rendered = collect_rendered_text(&out.shapes).join("\n");
        out.textures_delta.clear();
        for expected in [
            "Playlists",
            "A-Z",
            "Zulu Playlist",
            "@ZuluCreator · Playlist",
            "!Pinned First In Server Order",
            "Particular · Playlist",
        ] {
            assert!(
                rendered.contains(expected),
                "expected '{expected}' in {theme:?} theme render, got:\n{rendered}"
            );
        }
    }
}

#[test]
fn playlists_page_renders_synced_youtube_playlists_mixes_and_filters_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();

    let cards = vec![
        playlist_sync::SyncedPlaylistCard {
            id: "PLlilpeep001".into(),
            title: "ALL LIL PEEP SONGS".into(),
            thumbnail_url: "https://i.ytimg.com/vi/abc11111111/hqdefault.jpg".into(),
            video_count: Some(162),
            badge_text: "162 vídeos".into(),
            owner_label: "@LILPEEPENERGY".into(),
            subtitle_line: "@LILPEEPENERGY · Playlist".into(),
            updated_text: None,
            privacy: playlist_sync::PlaylistPrivacy::Public,
            kind: playlist_sync::PlaylistKind::Playlist,
            is_mix: false,
            is_music: false,
            is_owned: false,
            is_saved: true,
            seed_video_id: Some("abc11111111".into()),
            browse_id: Some("VLPLlilpeep001".into()),
            has_full_playlist_link: true,
            stack_color_dark: None,
            stack_color_light: None,
        },
        playlist_sync::SyncedPlaylistCard {
            id: "WL".into(),
            title: "Assistir mais tarde".into(),
            thumbnail_url: "https://i.ytimg.com/vi/wl111111111/hqdefault.jpg".into(),
            video_count: Some(33),
            badge_text: "33 vídeos".into(),
            owner_label: "Particular".into(),
            subtitle_line: "Particular · Playlist".into(),
            updated_text: None,
            privacy: playlist_sync::PlaylistPrivacy::Private,
            kind: playlist_sync::PlaylistKind::WatchLater,
            is_mix: false,
            is_music: false,
            is_owned: true,
            is_saved: false,
            seed_video_id: Some("wl111111111".into()),
            browse_id: Some("VLWL".into()),
            has_full_playlist_link: true,
            stack_color_dark: None,
            stack_color_light: None,
        },
        playlist_sync::SyncedPlaylistCard {
            id: "RDMMseed1234567".into(),
            title: "Mix de Passionfruit".into(),
            thumbnail_url: "https://i.ytimg.com/vi/seed1234567/hqdefault.jpg".into(),
            video_count: None,
            badge_text: "Mix".into(),
            owner_label: "Drake, Jay Rock, Chris Brown e outros".into(),
            subtitle_line: "Drake, Jay Rock, Chris Brown e outros · Playlist".into(),
            updated_text: None,
            privacy: playlist_sync::PlaylistPrivacy::Public,
            kind: playlist_sync::PlaylistKind::Mix,
            is_mix: true,
            is_music: true,
            is_owned: false,
            is_saved: false,
            seed_video_id: Some("seed1234567".into()),
            browse_id: None,
            has_full_playlist_link: false,
            stack_color_dark: None,
            stack_color_light: None,
        },
    ];
    app.library.remote_playlists.all_items = cards.clone();
    app.library.remote_playlists.items = cards;
    app.library.remote_playlists.loaded = true;

    let palette = app.palette();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.playlists_page_ui(ui, &c, palette);
        },
    );
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();
    for expected in [
        "Playlists",
        "Adicionadas recentemente",
        "Música",
        "Mixes",
        "Minhas",
        "Salvas",
        "ALL LIL PEEP SONGS",
        "162 vídeos",
        "Assistir mais tarde",
        "33 vídeos",
        "Mix de Passionfruit",
        "Ver playlist completa",
    ] {
        assert!(
            rendered.contains(expected),
            "expected '{expected}' on synced Playlists page, got:\n{rendered}"
        );
    }

    app.start_playlists_filtered_sync(
        playlist_sync::PlaylistCategoryFilter::Mixes,
        playlist_sync::PlaylistSort::AlphabeticalAZ,
        &ctx,
    );
    let mut mix_out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.playlists_page_ui(ui, &c, palette);
        },
    );
    let mix_rendered = collect_rendered_text(&mix_out.shapes).join("\n");
    mix_out.textures_delta.clear();
    assert!(mix_rendered.contains("Mix de Passionfruit"));
    assert!(!mix_rendered.contains("ALL LIL PEEP SONGS"));
    assert!(mix_rendered.contains("A-Z"));
}

#[test]
fn watch_playlist_panel_renders_above_chips_and_controls_playback_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let videos = three_youtube_videos();
    app.selected = Some(videos[0].clone());
    app.watch.active_playlist = Some(app_state::ActivePlaylistPlayback::new(
        "WL".into(),
        "Assistir mais tarde".into(),
        "nathan".into(),
        playlist_sync::PlaylistPrivacy::Private,
        false,
        videos.clone(),
        Some(33),
        0,
        false,
        1,
        false,
        None,
    ));

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 900.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_sidebar_results_ui(ui, &c, "test_watch_pl_panel");
        },
    );
    let text_rects = collect_rendered_text_rects(&out.shapes);
    out.textures_delta.clear();
    let joined: String = text_rects
        .iter()
        .map(|(t, _)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for expected in [
        "Assistir mais tarde",
        "Privado",
        "nathan",
        "- 1 / 33",
        &videos[0].title,
        &videos[1].title,
        &videos[2].title,
    ] {
        assert!(
            joined.contains(expected),
            "expected '{expected}' in watch playlist panel, got:\n{joined}"
        );
    }
    let panel_y = text_rects
        .iter()
        .find(|(t, _)| t.contains("Assistir mais tarde"))
        .map(|(_, r)| r.min.y)
        .expect("panel header y");
    let tabs_y = text_rects
        .iter()
        .find(|(t, _)| t.contains("Relacionados"))
        .map(|(_, r)| r.min.y)
        .expect("tabs y");
    assert!(
        panel_y < tabs_y,
        "watch playlist panel ({panel_y}) must render above sidebar tabs ({tabs_y})"
    );

    assert!(app.has_next_video_target());
    assert!(!app.has_previous_video_target());

    app.trigger_next_video_action(false);
    assert_eq!(
        app.playback.pending_open_video.as_ref().map(|v| &v.url),
        Some(&videos[1].url)
    );
    assert!(app.playback.pending_open_keep_playlist);
    assert_eq!(
        app.watch.active_playlist.as_ref().map(|p| p.current_pos),
        Some(1)
    );
    assert!(app.has_previous_video_target());

    app.trigger_previous_video_action(false, 2.0, false);
    assert_eq!(
        app.playback.pending_open_video.as_ref().map(|v| &v.url),
        Some(&videos[0].url)
    );
    assert_eq!(
        app.watch.active_playlist.as_ref().map(|p| p.current_pos),
        Some(0)
    );

    let pl = app.watch.active_playlist.as_mut().expect("active playlist");
    pl.toggle_loop();
    assert!(pl.loop_playlist);
    assert_eq!(
        pl.peek_previous().map(|(pos, vid)| (pos, vid.url)),
        Some((2, videos[2].url.clone()))
    );
    pl.toggle_shuffle(42);
    assert!(pl.shuffle);
    assert_eq!(pl.current_video().map(|v| &v.url), Some(&videos[0].url));
}

#[test]
fn opening_playlist_video_sets_active_playlist_without_mutating_local_queue() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let videos = three_youtube_videos();
    let queued_extra = Video {
        title: "Explicit Queue Item".into(),
        channel: "Queue Channel".into(),
        url: "https://www.youtube.com/watch?v=queueOnly999".into(),
        thumbnail: String::new(),
        duration: "4:00".into(),
    };
    app.queue
        .data
        .add(queued_extra.clone())
        .expect("enqueue extra");
    let col_idx = app
        .library
        .data
        .create_collection("Estudos")
        .expect("create collection");
    for video in &videos {
        app.library.data.collections[col_idx]
            .videos
            .push(crate::library::VideoRecord {
                video: video.clone(),
            });
    }
    app.ui.browse_section = BrowseSection::Playlists;
    app.library.selected_collection_idx = Some(col_idx);

    let palette = app.palette();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.playlists_page_ui(ui, &c, palette);
        },
    );
    let play_all_pos = collect_rendered_text_rects(&out.shapes)
        .into_iter()
        .find(|(t, _)| t.contains("Reproduzir tudo"))
        .map(|(_, r)| r.center())
        .expect("Reproduzir tudo button rect");
    out.textures_delta.clear();

    let mut click_out = headless_pointer_click(&ctx, screen.size(), play_all_pos, |ui| {
        let c = ui.ctx().clone();
        let chosen = app.playlists_page_ui(ui, &c, palette);
        assert!(chosen.is_none());
    });
    click_out.textures_delta.clear();

    let active_pl = app
        .watch
        .active_playlist
        .as_ref()
        .expect("active playlist started");
    assert_eq!(active_pl.title, "Estudos");
    assert_eq!(active_pl.videos.len(), 3);
    assert_eq!(active_pl.current_pos, 0);
    assert!(app.playback.pending_open_keep_playlist);
    assert_eq!(
        app.playback.pending_open_video.as_ref().map(|v| &v.url),
        Some(&videos[0].url)
    );
    assert_eq!(app.queue.data.len(), 1);
    assert_eq!(app.queue.data.videos()[0].url, queued_extra.url);
}

#[test]
fn watch_later_page_merges_synced_youtube_wl_and_local_items_headless() {
    let ctx = egui::Context::default();
    fastframe_icons::install::<Icon>(&ctx);
    let mut app = FastTube::default();
    let videos = three_youtube_videos();

    app.library.watch_later_remote.playlist_id = Some("WL".into());
    app.library.watch_later_remote.total_results = Some(33);
    app.library.watch_later_remote.loaded = true;
    app.library.watch_later_remote.meta = Some(playlist_sync::RemotePlaylistDetailMetadata {
        playlist_id: "WL".into(),
        title: "Assistir mais tarde".into(),
        owner: "nathan".into(),
        privacy: playlist_sync::PlaylistPrivacy::Private,
        total_videos: Some(33),
        thumbnail_url: None,
        is_mix: false,
        is_editable: true,
    });
    app.library.watch_later_remote.items = vec![
        playlist_sync::RemotePlaylistDetailItem {
            result: SearchResult {
                video: videos[0].clone(),
                metadata: VideoMetadata::default(),
            },
            set_video_id: Some("set0".into()),
            index_text: Some("1".into()),
        },
        playlist_sync::RemotePlaylistDetailItem {
            result: SearchResult {
                video: videos[1].clone(),
                metadata: VideoMetadata::default(),
            },
            set_video_id: Some("set1".into()),
            index_text: Some("2".into()),
        },
    ];
    // Add duplicate videos[0] + unsynced local videos[2]
    app.library.data.toggle_watch_later(videos[0].clone());
    app.library.data.toggle_watch_later(videos[2].clone());

    let palette = app.palette();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 820.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            let c = ui.ctx().clone();
            let _ = app.watch_later_page_ui(ui, &c, palette);
        },
    );
    let rendered = collect_rendered_text(&out.shapes).join("\n");
    out.textures_delta.clear();

    assert_eq!(app.library.watch_later_remote.merged_videos.len(), 3);
    assert_eq!(app.watch_later_display_total(), 34);
    assert!(rendered.contains("Assistir mais tarde"));
    assert!(rendered.contains("nathan"));
    assert!(rendered.contains("34 vídeos"));
    assert!(rendered.contains(&videos[0].title));
    assert!(rendered.contains(&videos[1].title));
    assert!(rendered.contains(&videos[2].title));
}
