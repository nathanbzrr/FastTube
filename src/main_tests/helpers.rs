use super::super::*;

pub(super) fn sample_videos() -> Vec<Video> {
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

pub(super) fn valid_youtube_videos() -> Vec<Video> {
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

pub(super) fn three_youtube_videos() -> Vec<Video> {
    let mut videos = valid_youtube_videos();
    videos.push(Video {
        title: "Vídeo YouTube Três".into(),
        channel: "Canal Gama".into(),
        url: "https://www.youtube.com/watch?v=aqz-KE-bpKQ".into(),
        thumbnail: String::new(),
        duration: "4:10".into(),
    });
    videos
}

pub(super) fn render_history_section(
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

pub(super) fn history_queue_button_center(output: &egui::FullOutput) -> egui::Pos2 {
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

pub(super) fn click_history_queue_button(
    ctx: &egui::Context,
    app: &mut FastTube,
    position: egui::Pos2,
) {
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

pub(super) fn collect_rendered_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
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

pub(super) fn collect_rendered_text_rects(
    shapes: &[egui::epaint::ClippedShape],
) -> Vec<(String, egui::Rect)> {
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

pub(super) fn test_sub_result(
    video_id: &str,
    title: &str,
    channel: &str,
    channel_id: &str,
) -> SearchResult {
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

pub(super) fn find_text_center_recursive(shape: &egui::Shape, needle: &str) -> Option<egui::Pos2> {
    match shape {
        egui::Shape::Text(text) => {
            if text.galley.job.text == needle || text.galley.job.text.contains(needle) {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                return Some(rect.center());
            }
            None
        }
        egui::Shape::Vec(items) => items
            .iter()
            .find_map(|item| find_text_center_recursive(item, needle)),
        _ => None,
    }
}

pub(super) fn find_rendered_text_center(
    shapes: &[egui::epaint::ClippedShape],
    needle: &str,
) -> Option<egui::Pos2> {
    shapes
        .iter()
        .find_map(|clipped| find_text_center_recursive(&clipped.shape, needle))
}

pub(super) fn find_circle_center_recursive(
    shape: &egui::Shape,
    radius: f32,
    max_x: f32,
) -> Option<egui::Pos2> {
    match shape {
        egui::Shape::Circle(circle) => ((circle.radius - radius).abs() < 0.15
            && circle.center.x <= max_x)
            .then_some(circle.center),
        egui::Shape::Vec(items) => items
            .iter()
            .find_map(|item| find_circle_center_recursive(item, radius, max_x)),
        _ => None,
    }
}

pub(super) fn find_more_dots_center(
    shapes: &[egui::epaint::ClippedShape],
    max_x: f32,
) -> Option<egui::Pos2> {
    shapes
        .iter()
        .find_map(|clipped| find_circle_center_recursive(&clipped.shape, 1.45, max_x))
}

pub(super) fn headless_pointer_click<F>(
    ctx: &egui::Context,
    size: egui::Vec2,
    pos: egui::Pos2,
    mut draw: F,
) -> egui::FullOutput
where
    F: FnMut(&mut egui::Ui),
{
    for pressed in [true, false] {
        let mut out = ctx.run_ui(
            egui::RawInput {
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
            },
            &mut draw,
        );
        out.textures_delta.clear();
        if !pressed {
            return out;
        }
    }
    unreachable!()
}
