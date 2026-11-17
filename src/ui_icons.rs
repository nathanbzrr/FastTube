use crate::*;
use eframe::egui::{self, Color32, RichText};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YtSidebarIcon {
    Home,
    Shorts,
    Subscriptions,
    You,
    YourChannel,
    History,
    Playlists,
    WatchLater,
    Liked,
    Queue,
    Library,
    Downloads,
    Insights,
    Music,
    Live,
    Gaming,
    Podcasts,
    Trending,
    Mixes,
    NewToYou,
    Settings,
    Shortcuts,
    SignOut,
    ChevronUp,
    ChevronDown,
    Mic,
    Bell,
    Keyboard,
}

pub(crate) fn paint_arc_points(
    center: egui::Pos2,
    radius: f32,
    start_deg: f32,
    end_deg: f32,
    steps: usize,
) -> Vec<egui::Pos2> {
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let angle = (start_deg + (end_deg - start_deg) * t).to_radians();
            egui::pos2(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect()
}

pub(crate) fn paint_yt_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    icon: YtSidebarIcon,
    color: Color32,
    filled: bool,
) {
    let stroke = egui::Stroke::new(1.5, color);
    let thin = egui::Stroke::new(1.25, color);
    match icon {
        YtSidebarIcon::Home => {
            if filled {
                let roof = vec![
                    egui::pos2(center.x, center.y - 8.5),
                    egui::pos2(center.x + 8.5, center.y - 1.0),
                    egui::pos2(center.x + 6.5, center.y - 1.0),
                    egui::pos2(center.x + 6.5, center.y + 7.5),
                    egui::pos2(center.x + 2.2, center.y + 7.5),
                    egui::pos2(center.x + 2.2, center.y + 2.0),
                    egui::pos2(center.x - 2.2, center.y + 2.0),
                    egui::pos2(center.x - 2.2, center.y + 7.5),
                    egui::pos2(center.x - 6.5, center.y + 7.5),
                    egui::pos2(center.x - 6.5, center.y - 1.0),
                    egui::pos2(center.x - 8.5, center.y - 1.0),
                ];
                // Split into convex upper gable and lower legs for crisp concave silhouette.
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        roof[0],
                        egui::pos2(center.x + 8.5, center.y - 0.5),
                        egui::pos2(center.x - 8.5, center.y - 0.5),
                    ],
                    color,
                    egui::Stroke::NONE,
                ));
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(center.x - 6.5, center.y - 1.0),
                        egui::pos2(center.x - 2.2, center.y + 7.5),
                    ),
                    0.0,
                    color,
                );
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(center.x + 2.2, center.y - 1.0),
                        egui::pos2(center.x + 6.5, center.y + 7.5),
                    ),
                    0.0,
                    color,
                );
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(center.x - 3.0, center.y - 1.5),
                        egui::pos2(center.x + 3.0, center.y + 2.2),
                    ),
                    0.0,
                    color,
                );
            } else {
                let pts = vec![
                    egui::pos2(center.x - 8.0, center.y - 0.5),
                    egui::pos2(center.x, center.y - 8.0),
                    egui::pos2(center.x + 8.0, center.y - 0.5),
                    egui::pos2(center.x + 6.2, center.y - 0.5),
                    egui::pos2(center.x + 6.2, center.y + 7.5),
                    egui::pos2(center.x + 2.2, center.y + 7.5),
                    egui::pos2(center.x + 2.2, center.y + 2.0),
                    egui::pos2(center.x - 2.2, center.y + 2.0),
                    egui::pos2(center.x - 2.2, center.y + 7.5),
                    egui::pos2(center.x - 6.2, center.y + 7.5),
                    egui::pos2(center.x - 6.2, center.y - 0.5),
                ];
                painter.add(egui::Shape::closed_line(pts, stroke));
            }
        }
        YtSidebarIcon::Shorts => {
            let s_loop = vec![
                egui::pos2(center.x + 4.8, center.y - 6.8),
                egui::pos2(center.x - 4.8, center.y - 2.4),
                egui::pos2(center.x - 4.8, center.y + 0.6),
                egui::pos2(center.x + 4.8, center.y + 4.2),
                egui::pos2(center.x - 4.5, center.y + 7.8),
            ];
            let outer = vec![
                egui::pos2(center.x + 5.8, center.y - 4.2),
                egui::pos2(center.x + 3.2, center.y - 8.2),
                egui::pos2(center.x - 5.6, center.y - 3.8),
                egui::pos2(center.x - 6.2, center.y + 0.8),
                egui::pos2(center.x - 3.0, center.y + 2.4),
            ];
            let lower = vec![
                egui::pos2(center.x - 5.8, center.y + 4.2),
                egui::pos2(center.x - 3.2, center.y + 8.2),
                egui::pos2(center.x + 5.6, center.y + 3.8),
                egui::pos2(center.x + 6.2, center.y - 0.8),
                egui::pos2(center.x + 3.0, center.y - 2.4),
            ];
            let _ = s_loop;
            painter.add(egui::Shape::line(outer, stroke));
            painter.add(egui::Shape::line(lower, stroke));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(center.x - 1.6, center.y - 2.6),
                    egui::pos2(center.x - 1.6, center.y + 2.6),
                    egui::pos2(center.x + 2.8, center.y),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        YtSidebarIcon::Subscriptions => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y - 7.5),
                    egui::pos2(center.x + 5.0, center.y - 7.5),
                ],
                thin,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 6.8, center.y - 4.8),
                    egui::pos2(center.x + 6.8, center.y - 4.8),
                ],
                thin,
            );
            let box_rect = egui::Rect::from_min_max(
                egui::pos2(center.x - 8.0, center.y - 2.2),
                egui::pos2(center.x + 8.0, center.y + 7.5),
            );
            painter.rect_stroke(box_rect, 2.0, stroke, egui::StrokeKind::Inside);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(center.x - 1.8, center.y + 0.4),
                    egui::pos2(center.x - 1.8, center.y + 5.0),
                    egui::pos2(center.x + 2.4, center.y + 2.7),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        YtSidebarIcon::You => {
            painter.circle_stroke(center, 8.2, stroke);
            painter.circle_stroke(egui::pos2(center.x, center.y - 2.0), 2.6, thin);
            let shoulders =
                paint_arc_points(egui::pos2(center.x, center.y + 6.5), 4.8, 215.0, 325.0, 10);
            painter.add(egui::Shape::line(shoulders, thin));
        }
        YtSidebarIcon::YourChannel => {
            let box_rect = egui::Rect::from_center_size(center, egui::vec2(16.0, 15.0));
            painter.rect_stroke(box_rect, 2.5, stroke, egui::StrokeKind::Inside);
            painter.circle_stroke(egui::pos2(center.x, center.y - 2.0), 2.5, thin);
            let shoulders =
                paint_arc_points(egui::pos2(center.x, center.y + 6.2), 4.6, 215.0, 325.0, 10);
            painter.add(egui::Shape::line(shoulders, thin));
        }
        YtSidebarIcon::History => {
            let arc = paint_arc_points(center, 7.2, -145.0, 140.0, 20);
            painter.add(egui::Shape::line(arc, stroke));
            let arrow_tip = egui::pos2(center.x - 6.0, center.y - 4.0);
            painter.line_segment(
                [arrow_tip, egui::pos2(arrow_tip.x - 2.0, arrow_tip.y - 3.0)],
                thin,
            );
            painter.line_segment(
                [arrow_tip, egui::pos2(arrow_tip.x + 3.0, arrow_tip.y - 1.2)],
                thin,
            );
            painter.line_segment([center, egui::pos2(center.x, center.y - 4.2)], thin);
            painter.line_segment([center, egui::pos2(center.x + 3.2, center.y + 2.0)], thin);
        }
        YtSidebarIcon::Playlists => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 7.5, center.y - 4.5),
                    egui::pos2(center.x + 7.0, center.y - 4.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 7.5, center.y - 0.5),
                    egui::pos2(center.x + 7.0, center.y - 0.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 7.5, center.y + 3.5),
                    egui::pos2(center.x + 0.5, center.y + 3.5),
                ],
                stroke,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(center.x + 2.5, center.y + 1.5),
                    egui::pos2(center.x + 2.5, center.y + 7.0),
                    egui::pos2(center.x + 7.5, center.y + 4.2),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        YtSidebarIcon::WatchLater => {
            painter.circle_stroke(center, 7.6, stroke);
            painter.line_segment([center, egui::pos2(center.x, center.y - 4.4)], thin);
            painter.line_segment([center, egui::pos2(center.x + 3.4, center.y + 2.0)], thin);
        }
        YtSidebarIcon::Liked => {
            let outline = vec![
                egui::pos2(center.x - 7.5, center.y - 0.5),
                egui::pos2(center.x - 4.0, center.y - 0.5),
                egui::pos2(center.x - 4.0, center.y + 6.8),
                egui::pos2(center.x - 7.5, center.y + 6.8),
            ];
            painter.rect_stroke(
                egui::Rect::from_points(&outline),
                1.0,
                thin,
                egui::StrokeKind::Inside,
            );
            let thumb_body = vec![
                egui::pos2(center.x - 4.0, center.y - 0.5),
                egui::pos2(center.x - 0.5, center.y - 6.8),
                egui::pos2(center.x + 1.5, center.y - 6.2),
                egui::pos2(center.x + 1.0, center.y - 1.5),
                egui::pos2(center.x + 6.5, center.y - 1.5),
                egui::pos2(center.x + 5.2, center.y + 6.8),
                egui::pos2(center.x - 4.0, center.y + 6.8),
            ];
            painter.add(egui::Shape::closed_line(thumb_body, thin));
        }
        YtSidebarIcon::Queue => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 7.0, center.y - 5.5),
                    egui::pos2(center.x + 5.0, center.y - 5.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 7.0, center.y - 1.5),
                    egui::pos2(center.x + 5.0, center.y - 1.5),
                ],
                stroke,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(center.x - 6.5, center.y + 1.5),
                    egui::pos2(center.x - 6.5, center.y + 7.2),
                    egui::pos2(center.x - 1.5, center.y + 4.3),
                ],
                color,
                egui::Stroke::NONE,
            ));
            painter.line_segment(
                [
                    egui::pos2(center.x + 0.5, center.y + 4.3),
                    egui::pos2(center.x + 7.0, center.y + 4.3),
                ],
                stroke,
            );
        }
        YtSidebarIcon::Library => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.5, center.y - 6.8),
                    egui::pos2(center.x + 5.5, center.y - 6.8),
                ],
                thin,
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    egui::pos2(center.x - 7.5, center.y - 3.8),
                    egui::pos2(center.x + 7.5, center.y + 7.0),
                ),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(center.x - 1.5, center.y - 0.8),
                    egui::pos2(center.x - 1.5, center.y + 4.2),
                    egui::pos2(center.x + 3.0, center.y + 1.7),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        YtSidebarIcon::Music => {
            painter.circle_filled(egui::pos2(center.x - 2.5, center.y + 4.8), 2.8, color);
            painter.line_segment(
                [
                    egui::pos2(center.x + 0.1, center.y + 4.8),
                    egui::pos2(center.x + 0.1, center.y - 6.8),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 0.1, center.y - 6.8),
                    egui::pos2(center.x + 5.5, center.y - 4.2),
                ],
                egui::Stroke::new(2.2, color),
            );
        }
        YtSidebarIcon::Live => {
            painter.circle_filled(center, 2.2, color);
            for (r, a1, a2) in [
                (4.6, -48.0, 48.0),
                (4.6, 132.0, 228.0),
                (7.8, -50.0, 50.0),
                (7.8, 130.0, 230.0),
            ] {
                painter.add(egui::Shape::line(
                    paint_arc_points(center, r, a1, a2, 8),
                    thin,
                ));
            }
        }
        YtSidebarIcon::Gaming => {
            let body = egui::Rect::from_center_size(center, egui::vec2(16.0, 10.5));
            painter.rect_stroke(body, 4.5, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.2, center.y - 2.2),
                    egui::pos2(center.x - 4.2, center.y + 2.2),
                ],
                thin,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 6.4, center.y),
                    egui::pos2(center.x - 2.0, center.y),
                ],
                thin,
            );
            painter.circle_filled(egui::pos2(center.x + 4.8, center.y - 1.4), 1.1, color);
            painter.circle_filled(egui::pos2(center.x + 3.2, center.y + 1.4), 1.1, color);
        }
        YtSidebarIcon::Podcasts => {
            painter.circle_filled(egui::pos2(center.x, center.y - 1.5), 2.2, color);
            for r in [5.0, 8.0] {
                painter.add(egui::Shape::line(
                    paint_arc_points(egui::pos2(center.x, center.y - 1.5), r, -145.0, 35.0, 12),
                    thin,
                ));
            }
            painter.line_segment(
                [
                    egui::pos2(center.x, center.y + 1.2),
                    egui::pos2(center.x, center.y + 7.5),
                ],
                stroke,
            );
        }
        YtSidebarIcon::Trending => {
            let flame = vec![
                egui::pos2(center.x, center.y - 8.2),
                egui::pos2(center.x + 5.8, center.y - 1.0),
                egui::pos2(center.x + 5.2, center.y + 5.5),
                egui::pos2(center.x, center.y + 8.0),
                egui::pos2(center.x - 5.2, center.y + 5.5),
                egui::pos2(center.x - 5.8, center.y - 0.5),
                egui::pos2(center.x - 1.8, center.y - 3.8),
            ];
            painter.add(egui::Shape::closed_line(flame, stroke));
        }
        YtSidebarIcon::Mixes => {
            for dy in [-4.5_f32, 0.0, 4.5] {
                painter.line_segment(
                    [
                        egui::pos2(center.x - 7.0, center.y + dy),
                        egui::pos2(center.x + 7.0, center.y + dy),
                    ],
                    stroke,
                );
            }
            painter.circle_filled(egui::pos2(center.x - 3.0, center.y - 4.5), 1.8, color);
            painter.circle_filled(egui::pos2(center.x + 3.0, center.y), 1.8, color);
            painter.circle_filled(egui::pos2(center.x - 1.5, center.y + 4.5), 1.8, color);
        }
        YtSidebarIcon::NewToYou => {
            let star = vec![
                egui::pos2(center.x, center.y - 8.0),
                egui::pos2(center.x + 2.2, center.y - 2.2),
                egui::pos2(center.x + 8.0, center.y),
                egui::pos2(center.x + 2.2, center.y + 2.2),
                egui::pos2(center.x, center.y + 8.0),
                egui::pos2(center.x - 2.2, center.y + 2.2),
                egui::pos2(center.x - 8.0, center.y),
                egui::pos2(center.x - 2.2, center.y - 2.2),
            ];
            painter.add(egui::Shape::closed_line(star, thin));
        }
        YtSidebarIcon::Settings => {
            painter.circle_stroke(center, 3.2, stroke);
            painter.circle_stroke(center, 6.6, thin);
            for i in 0..4 {
                let a = (i as f32 * 45.0).to_radians();
                let dx = a.cos();
                let dy = a.sin();
                painter.line_segment(
                    [
                        egui::pos2(center.x + dx * 6.5, center.y + dy * 6.5),
                        egui::pos2(center.x + dx * 8.4, center.y + dy * 8.4),
                    ],
                    thin,
                );
                painter.line_segment(
                    [
                        egui::pos2(center.x - dx * 6.5, center.y - dy * 6.5),
                        egui::pos2(center.x - dx * 8.4, center.y - dy * 8.4),
                    ],
                    thin,
                );
            }
        }
        YtSidebarIcon::Shortcuts | YtSidebarIcon::Keyboard => {
            let rect = egui::Rect::from_center_size(center, egui::vec2(16.0, 11.0));
            painter.rect_stroke(rect, 2.0, thin, egui::StrokeKind::Inside);
            for dx in [-4.5_f32, -1.5, 1.5, 4.5] {
                painter.circle_filled(egui::pos2(center.x + dx, center.y - 1.8), 0.9, color);
            }
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.0, center.y + 2.2),
                    egui::pos2(center.x + 4.0, center.y + 2.2),
                ],
                thin,
            );
        }
        YtSidebarIcon::SignOut => {
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(center.x + 1.0, center.y - 7.0),
                    egui::pos2(center.x - 6.5, center.y - 7.0),
                    egui::pos2(center.x - 6.5, center.y + 7.0),
                    egui::pos2(center.x + 1.0, center.y + 7.0),
                ],
                stroke,
            ));
            painter.line_segment(
                [
                    egui::pos2(center.x - 1.5, center.y),
                    egui::pos2(center.x + 7.2, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 4.2, center.y - 3.2),
                    egui::pos2(center.x + 7.5, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 4.2, center.y + 3.2),
                    egui::pos2(center.x + 7.5, center.y),
                ],
                stroke,
            );
        }
        YtSidebarIcon::ChevronUp => {
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(center.x - 5.2, center.y + 2.5),
                    egui::pos2(center.x, center.y - 2.8),
                    egui::pos2(center.x + 5.2, center.y + 2.5),
                ],
                stroke,
            ));
        }
        YtSidebarIcon::ChevronDown => {
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(center.x - 5.2, center.y - 2.5),
                    egui::pos2(center.x, center.y + 2.8),
                    egui::pos2(center.x + 5.2, center.y - 2.5),
                ],
                stroke,
            ));
        }
        YtSidebarIcon::Mic => {
            let capsule = egui::Rect::from_center_size(
                egui::pos2(center.x, center.y - 2.2),
                egui::vec2(5.6, 9.6),
            );
            painter.rect_filled(capsule, 2.8, color);
            let cradle =
                paint_arc_points(egui::pos2(center.x, center.y - 0.5), 5.4, 15.0, 165.0, 12);
            painter.add(egui::Shape::line(cradle, stroke));
            painter.line_segment(
                [
                    egui::pos2(center.x, center.y + 4.9),
                    egui::pos2(center.x, center.y + 8.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 3.6, center.y + 8.0),
                    egui::pos2(center.x + 3.6, center.y + 8.0),
                ],
                stroke,
            );
        }
        YtSidebarIcon::Bell => {
            let bell = vec![
                egui::pos2(center.x - 6.2, center.y + 3.6),
                egui::pos2(center.x - 4.8, center.y + 1.2),
                egui::pos2(center.x - 4.6, center.y - 2.8),
                egui::pos2(center.x - 2.8, center.y - 6.2),
                egui::pos2(center.x, center.y - 7.2),
                egui::pos2(center.x + 2.8, center.y - 6.2),
                egui::pos2(center.x + 4.6, center.y - 2.8),
                egui::pos2(center.x + 4.8, center.y + 1.2),
                egui::pos2(center.x + 6.2, center.y + 3.6),
            ];
            painter.add(egui::Shape::closed_line(bell, stroke));
            let clapper =
                paint_arc_points(egui::pos2(center.x, center.y + 4.2), 2.2, 10.0, 170.0, 8);
            painter.add(egui::Shape::line(clapper, thin));
        }
        YtSidebarIcon::Downloads => {
            painter.line_segment(
                [
                    egui::pos2(center.x, center.y - 7.2),
                    egui::pos2(center.x, center.y + 2.4),
                ],
                stroke,
            );
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(center.x - 4.2, center.y - 1.4),
                    egui::pos2(center.x, center.y + 3.0),
                    egui::pos2(center.x + 4.2, center.y - 1.4),
                ],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(center.x - 6.8, center.y + 3.5),
                    egui::pos2(center.x - 6.8, center.y + 6.8),
                    egui::pos2(center.x + 6.8, center.y + 6.8),
                    egui::pos2(center.x + 6.8, center.y + 3.5),
                ],
                stroke,
            ));
        }
        YtSidebarIcon::Insights => {
            painter.line_segment(
                [
                    egui::pos2(center.x - 6.8, center.y + 6.8),
                    egui::pos2(center.x + 6.8, center.y + 6.8),
                ],
                thin,
            );
            for (dx, h) in [(-4.2_f32, 5.5_f32), (0.0, 11.0), (4.2, 8.2)] {
                let bar = egui::Rect::from_min_max(
                    egui::pos2(center.x + dx - 1.4, center.y + 6.0 - h),
                    egui::pos2(center.x + dx + 1.4, center.y + 6.0),
                );
                if filled {
                    painter.rect_filled(bar, 1.0, color);
                } else {
                    painter.rect_stroke(bar, 1.0, thin, egui::StrokeKind::Inside);
                }
            }
        }
    }
}

pub(crate) fn paint_verified_badge_sized(ui: &mut egui::Ui, palette: &ThemePalette, diameter: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
    let c = rect.center();
    let radius = diameter * 0.446;
    ui.painter()
        .circle_filled(c, radius, palette.neutral_status_fg);
    let check_color = if palette.card_bg.r() < 128 {
        Color32::from_rgb(15, 15, 15)
    } else {
        Color32::WHITE
    };
    let s = diameter / 13.0;
    ui.painter().add(egui::Shape::line(
        vec![
            egui::pos2(c.x - 2.8 * s, c.y + 0.1 * s),
            egui::pos2(c.x - 0.6 * s, c.y + 2.3 * s),
            egui::pos2(c.x + 3.0 * s, c.y - 2.1 * s),
        ],
        egui::Stroke::new((1.5 * s).max(1.3), check_color),
    ));
}

pub(crate) fn paint_verified_badge(ui: &mut egui::Ui, palette: &ThemePalette) {
    paint_verified_badge_sized(ui, palette, 13.0);
}

pub(crate) fn paint_link_chain_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let c = rect.center();
    let stroke = egui::Stroke::new(1.35, color);
    let left_arc = paint_arc_points(egui::pos2(c.x - 2.5, c.y), 3.0, 90.0, 270.0, 8);
    let right_arc = paint_arc_points(egui::pos2(c.x + 2.5, c.y), 3.0, -90.0, 90.0, 8);
    ui.painter().add(egui::Shape::line(left_arc, stroke));
    ui.painter().add(egui::Shape::line(right_arc, stroke));
    ui.painter().line_segment(
        [
            egui::pos2(c.x - 2.5, c.y - 3.0),
            egui::pos2(c.x - 0.4, c.y - 3.0),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(c.x - 2.5, c.y + 3.0),
            egui::pos2(c.x - 0.4, c.y + 3.0),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(c.x + 0.4, c.y - 3.0),
            egui::pos2(c.x + 2.5, c.y - 3.0),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(c.x + 0.4, c.y + 3.0),
            egui::pos2(c.x + 2.5, c.y + 3.0),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [egui::pos2(c.x - 2.3, c.y), egui::pos2(c.x + 2.3, c.y)],
        stroke,
    );
}

pub(crate) fn paint_channel_search_icon_btn(
    ui: &mut egui::Ui,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(34.0, 40.0), egui::Sense::click());
    let c = rect.center();
    let color = if active || response.hovered() {
        palette.header_button_fg
    } else {
        palette.neutral_status_fg
    };
    let glass = egui::pos2(c.x - 1.5, c.y - 1.5);
    let stroke = egui::Stroke::new(1.6, color);
    ui.painter().circle_stroke(glass, 5.6, stroke);
    ui.painter().line_segment(
        [
            egui::pos2(glass.x + 4.0, glass.y + 4.0),
            egui::pos2(glass.x + 8.0, glass.y + 8.0),
        ],
        stroke,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn paint_globe_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    let stroke = egui::Stroke::new(1.25, color);
    ui.painter().circle_stroke(c, 6.8, stroke);
    ui.painter().line_segment(
        [egui::pos2(c.x - 6.8, c.y), egui::pos2(c.x + 6.8, c.y)],
        stroke,
    );
    ui.painter().line_segment(
        [egui::pos2(c.x, c.y - 6.8), egui::pos2(c.x, c.y + 6.8)],
        stroke,
    );
    let left_mer = paint_arc_points(egui::pos2(c.x + 3.0, c.y), 7.2, 125.0, 235.0, 8);
    let right_mer = paint_arc_points(egui::pos2(c.x - 3.0, c.y), 7.2, -55.0, 55.0, 8);
    ui.painter().add(egui::Shape::line(left_mer, stroke));
    ui.painter().add(egui::Shape::line(right_mer, stroke));
}

pub(crate) fn paint_info_circle_icon(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    let stroke = egui::Stroke::new(1.25, color);
    ui.painter().circle_stroke(c, 6.8, stroke);
    ui.painter()
        .circle_filled(egui::pos2(c.x, c.y - 3.0), 1.0, color);
    ui.painter().line_segment(
        [egui::pos2(c.x, c.y - 0.8), egui::pos2(c.x, c.y + 3.6)],
        stroke,
    );
}

pub(crate) fn paint_shorts_badge_icon(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 24.0), egui::Sense::hover());
    let c = rect.center();
    let red = Color32::from_rgb(255, 0, 51);
    let top_pill = egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x + 2.5, c.y - 9.5),
            egui::pos2(c.x + 8.0, c.y - 5.5),
            egui::pos2(c.x - 2.5, c.y + 1.5),
            egui::pos2(c.x - 7.5, c.y - 2.5),
        ],
        red,
        egui::Stroke::NONE,
    );
    let bot_pill = egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x + 2.5, c.y - 1.5),
            egui::pos2(c.x + 7.5, c.y + 2.5),
            egui::pos2(c.x - 2.5, c.y + 9.5),
            egui::pos2(c.x - 8.0, c.y + 5.5),
        ],
        red,
        egui::Stroke::NONE,
    );
    ui.painter().add(top_pill);
    ui.painter().add(bot_pill);
    ui.painter().rect_filled(
        egui::Rect::from_center_size(c, egui::vec2(12.0, 8.5)),
        3.0,
        red,
    );
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - 2.2, c.y - 3.2),
            egui::pos2(c.x - 2.2, c.y + 3.2),
            egui::pos2(c.x + 3.4, c.y),
        ],
        Color32::WHITE,
        egui::Stroke::NONE,
    ));
}

pub(crate) fn paint_more_dots_button(ui: &mut egui::Ui, palette: &ThemePalette) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .circle_filled(rect.center(), 13.0, palette.header_button_bg);
    }
    let c = rect.center();
    let dot_color = if response.hovered() {
        palette.header_button_fg
    } else {
        palette.neutral_status_fg
    };
    for dy in [-4.5_f32, 0.0, 4.5] {
        ui.painter()
            .circle_filled(egui::pos2(c.x, c.y + dy), 1.45, dot_color);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn paint_short_novo_badge(ui: &egui::Ui, rect: egui::Rect) {
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 8.0, rect.top() + 8.0),
        egui::vec2(38.0, 20.0),
    );
    ui.painter().rect_filled(
        badge_rect,
        4.0,
        Color32::from_rgba_unmultiplied(15, 15, 15, 220),
    );
    ui.painter().text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Novo",
        egui::FontId::proportional(11.0),
        Color32::WHITE,
    );
}

pub(crate) fn paint_mix_top_tabs(ui: &egui::Ui, rect: egui::Rect, dark_theme: bool) {
    let outer_color = if dark_theme {
        Color32::from_rgb(56, 56, 58)
    } else {
        Color32::from_rgb(208, 208, 212)
    };
    let inner_color = if dark_theme {
        Color32::from_rgb(86, 86, 90)
    } else {
        Color32::from_rgb(176, 176, 182)
    };
    let top_strip = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 18.0, rect.top()),
        egui::pos2(rect.right() - 18.0, rect.top() + 5.0),
    );
    let mid_strip = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 9.0, rect.top() + 4.0),
        egui::pos2(rect.right() - 9.0, rect.top() + 9.0),
    );
    ui.painter().rect_filled(
        top_strip,
        egui::CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        },
        outer_color,
    );
    ui.painter().rect_filled(
        mid_strip,
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        inner_color,
    );
}

pub(crate) fn paint_youtube_logo_badge(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 21.0), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, 6.0, Color32::from_rgb(255, 0, 51));
    let center = rect.center();
    let triangle = vec![
        egui::pos2(center.x - 3.6, center.y - 4.6),
        egui::pos2(center.x - 3.6, center.y + 4.6),
        egui::pos2(center.x + 5.0, center.y),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        triangle,
        Color32::WHITE,
        egui::Stroke::NONE,
    ));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn paint_channel_avatar(
    ui: &mut egui::Ui,
    channel_name: &str,
    diameter: f32,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::click());
    let [r, g, b] = recommendations::channel_avatar_rgb(channel_name);
    let initials = recommendations::channel_avatar_initials(channel_name);
    ui.painter()
        .circle_filled(rect.center(), diameter * 0.5, Color32::from_rgb(r, g, b));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional((diameter * 0.39).clamp(10.0, 16.0)),
        Color32::WHITE,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn youtube_tab_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let text = if selected {
        RichText::new(label)
            .size(14.0)
            .strong()
            .color(palette.header_button_fg)
    } else {
        RichText::new(label)
            .size(14.0)
            .color(palette.neutral_status_fg)
    };
    let response = ui.add(
        egui::Button::new(text)
            .frame(false)
            .fill(Color32::TRANSPARENT)
            .min_size(egui::vec2(0.0, 40.0)),
    );
    if selected {
        let y = response.rect.bottom() - 1.0;
        ui.painter().line_segment(
            [
                egui::pos2(response.rect.left() + 10.0, y),
                egui::pos2(response.rect.right() - 10.0, y),
            ],
            egui::Stroke::new(3.0, palette.header_button_fg),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn sidebar_section_header_item(
    ui: &mut egui::Ui,
    title: &str,
    show_chevron: bool,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let desired = egui::vec2(ui.available_width().max(110.0), 40.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let pill_rect = rect.shrink2(egui::vec2(12.0, 0.0));
    let fill = if active {
        palette.card_alt_bg
    } else if response.hovered() {
        palette.header_button_bg
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(pill_rect, 10.0, fill);
    }
    let header_text = if show_chevron {
        format!("{title}  \u{203a}")
    } else {
        title.to_owned()
    };
    ui.painter().text(
        egui::pos2(pill_rect.left() + 12.0, pill_rect.center().y),
        egui::Align2::LEFT_CENTER,
        header_text,
        egui::FontId::proportional(15.5),
        palette.header_button_fg,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn sidebar_pill_item(
    ui: &mut egui::Ui,
    icon: YtSidebarIcon,
    label: &str,
    active: bool,
    unread_dot: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let desired = egui::vec2(ui.available_width().max(110.0), 40.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let pill_rect = rect.shrink2(egui::vec2(12.0, 0.0));
    let fill = if active {
        palette.card_alt_bg
    } else if response.hovered() {
        palette.header_button_bg
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(pill_rect, 10.0, fill);
    }
    let text_color = if active {
        palette.header_button_fg
    } else {
        palette.channel_fg
    };
    paint_yt_icon(
        ui.painter(),
        egui::pos2(pill_rect.left() + 22.0, pill_rect.center().y),
        icon,
        text_color,
        active,
    );
    let display_label = truncate_text_chars(label, 23);
    ui.painter().text(
        egui::pos2(pill_rect.left() + 48.0, pill_rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_label,
        egui::FontId::proportional(14.0),
        text_color,
    );
    if unread_dot {
        ui.painter().circle_filled(
            egui::pos2(pill_rect.right() - 12.0, pill_rect.center().y),
            2.5,
            Color32::from_rgb(62, 166, 255),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn compact_sidebar_item(
    ui: &mut egui::Ui,
    icon: YtSidebarIcon,
    label: &str,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(64.0, 72.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(6.0, 4.0)),
            10.0,
            palette.card_alt_bg,
        );
    }
    let label_color = if active {
        palette.header_button_fg
    } else {
        palette.neutral_status_fg
    };
    paint_yt_icon(
        ui.painter(),
        egui::pos2(rect.center().x, rect.center().y - 9.0),
        icon,
        palette.header_button_fg,
        active,
    );
    ui.painter().text(
        egui::pos2(rect.center().x, rect.center().y + 20.0),
        egui::Align2::CENTER_CENTER,
        truncate_text_chars(label, 13),
        egui::FontId::proportional(10.0),
        label_color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn sidebar_channel_item(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    avatars: &mut thumbnails::AvatarCache,
    avatar_url: Option<&str>,
    channel_name: &str,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let desired = egui::vec2(ui.available_width().max(110.0), 40.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let pill_rect = rect.shrink2(egui::vec2(12.0, 0.0));
    let fill = if active {
        palette.card_alt_bg
    } else if response.hovered() {
        palette.header_button_bg
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(pill_rect, 10.0, fill);
    }
    let avatar_center = egui::pos2(pill_rect.left() + 24.0, pill_rect.center().y);
    let avatar_rect = egui::Rect::from_center_size(avatar_center, egui::vec2(24.0, 24.0));
    let painted = avatar_url
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
        .is_some_and(|uri| avatars.try_paint_circle(ui, ctx, avatar_rect, uri));
    if !painted {
        let [r, g, b] = recommendations::channel_avatar_rgb(channel_name);
        let initials = recommendations::channel_avatar_initials(channel_name);
        ui.painter()
            .circle_filled(avatar_center, 12.0, Color32::from_rgb(r, g, b));
        ui.painter().text(
            avatar_center,
            egui::Align2::CENTER_CENTER,
            initials,
            egui::FontId::proportional(10.0),
            Color32::WHITE,
        );
    }
    let display_name = truncate_text_chars(channel_name, 16);
    ui.painter().text(
        egui::pos2(pill_rect.left() + 48.0, pill_rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_name,
        egui::FontId::proportional(14.0),
        palette.channel_fg,
    );
    ui.painter().circle_filled(
        egui::pos2(pill_rect.right() - 12.0, pill_rect.center().y),
        2.5,
        Color32::from_rgb(62, 166, 255),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(channel_name)
}

pub(crate) fn paint_thumbnail_overlays(
    ui: &egui::Ui,
    rect: egui::Rect,
    duration: &str,
    progress_ratio: Option<f32>,
    is_music: bool,
) {
    let trimmed = duration.trim();
    if !trimmed.is_empty() {
        let badge_text = if is_music {
            format!("\u{266b} {trimmed}")
        } else {
            trimmed.to_owned()
        };
        let char_count = badge_text.chars().count() as f32;
        let badge_w = (char_count * 6.8 + 12.0).max(34.0);
        let badge_h = 20.0;
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(
                (rect.right() - badge_w - 8.0).max(rect.left() + 4.0),
                (rect.bottom() - badge_h - 8.0).max(rect.top() + 4.0),
            ),
            egui::vec2(badge_w, badge_h),
        );
        ui.painter().rect_filled(
            badge_rect,
            4.0,
            Color32::from_rgba_unmultiplied(15, 15, 15, 220),
        );
        ui.painter().text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            badge_text,
            egui::FontId::proportional(11.5),
            Color32::from_rgb(241, 241, 241),
        );
    }
    if let Some(ratio) = progress_ratio {
        let bar_h = 3.5;
        let bg_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 2.0, rect.bottom() - bar_h),
            egui::pos2(rect.right() - 2.0, rect.bottom()),
        );
        ui.painter().rect_filled(
            bg_rect,
            2.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, 70),
        );
        let fill_w = bg_rect.width() * ratio.clamp(0.0, 1.0);
        let fill_rect = egui::Rect::from_min_max(
            bg_rect.min,
            egui::pos2(bg_rect.min.x + fill_w, bg_rect.max.y),
        );
        ui.painter()
            .rect_filled(fill_rect, 2.0, Color32::from_rgb(255, 0, 51));
    }
}

pub(crate) fn paint_playlist_badge_overlay(ui: &egui::Ui, rect: egui::Rect, label: &str) {
    let text_w = (label.chars().count() as f32) * 6.8;
    let badge_w = (text_w + 30.0).max(52.0);
    let badge_h = 21.0;
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(
            (rect.right() - badge_w - 8.0).max(rect.left() + 4.0),
            (rect.bottom() - badge_h - 8.0).max(rect.top() + 4.0),
        ),
        egui::vec2(badge_w, badge_h),
    );
    ui.painter().rect_filled(
        badge_rect,
        4.0,
        Color32::from_rgba_unmultiplied(15, 15, 15, 224),
    );
    let fg = Color32::from_rgb(241, 241, 241);
    let icon_x = badge_rect.left() + 8.0;
    let cy = badge_rect.center().y;
    for (idx, dy) in [-3.2_f32, 0.0, 3.2].into_iter().enumerate() {
        let len = if idx == 2 { 5.2 } else { 8.5 };
        ui.painter().line_segment(
            [
                egui::pos2(icon_x, cy + dy),
                egui::pos2(icon_x + len, cy + dy),
            ],
            egui::Stroke::new(1.4, fg),
        );
    }
    let tri = vec![
        egui::pos2(icon_x + 6.4, cy + 1.0),
        egui::pos2(icon_x + 6.4, cy + 5.2),
        egui::pos2(icon_x + 9.8, cy + 3.1),
    ];
    ui.painter()
        .add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
    ui.painter().text(
        egui::pos2(icon_x + 14.0, cy),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.5),
        fg,
    );
}

pub(crate) fn paint_thumbnail_novidade_badge(ui: &egui::Ui, rect: egui::Rect) {
    let badge_w = 58.0;
    let badge_h = 18.0;
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 6.0, rect.top() + 6.0),
        egui::vec2(badge_w, badge_h),
    );
    ui.painter().rect_filled(
        badge_rect,
        4.0,
        Color32::from_rgba_unmultiplied(241, 241, 241, 235),
    );
    ui.painter().text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Novidade",
        egui::FontId::proportional(11.0),
        Color32::from_rgb(15, 15, 15),
    );
}

pub(crate) fn truncate_text_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let prefix: String = trimmed.chars().take(keep).collect();
    format!("{}…", prefix.trim_end())
}

pub(crate) fn format_youtube_card_subline(
    duration: &str,
    metadata: Option<&VideoMetadata>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(views) = metadata.and_then(|meta| meta.view_count) {
        parts.push(format!(
            "▷ {}",
            recommendations::format_youtube_compact_views(views)
        ));
    } else if let Some(view_text) = metadata
        .and_then(|meta| meta.view_count_text.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("▷ {view_text}"));
    }
    if let Some(rel_date) = metadata
        .and_then(|meta| meta.upload_date.as_deref())
        .and_then(|date| recommendations::format_youtube_relative_time(date, None))
        .or_else(|| {
            metadata
                .and_then(|meta| meta.published_time_text.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        })
    {
        parts.push(rel_date);
    }
    if parts.is_empty() && !duration.trim().is_empty() {
        parts.push(duration.trim().to_owned());
    }
    (!parts.is_empty()).then(|| parts.join(" \u{00b7} "))
}

#[derive(Default)]
pub(crate) struct SearchCardAction {
    pub(crate) open_clicked: bool,
    pub(crate) enqueue_clicked: bool,
    pub(crate) toggle_watch_later: bool,
    pub(crate) open_save_modal: bool,
    pub(crate) share_clicked: bool,
    pub(crate) open_report_modal: bool,
    pub(crate) download_format: Option<downloads::DownloadFormat>,
    pub(crate) open_channel: Option<ChannelTarget>,
    pub(crate) filter_channel: Option<String>,
}

type FeatureFilterToggle = fn(&mut search::SearchFeatureFilters);
type FeatureFilterSpec = (&'static str, bool, FeatureFilterToggle);

pub(crate) fn feature_filter_items(f: &search::SearchFeatureFilters) -> [FeatureFilterSpec; 11] {
    [
        ("Ao vivo", f.live, |s| s.live = !s.live),
        ("4K", f.four_k, |s| s.four_k = !s.four_k),
        ("Alta Defini\u{00e7}\u{00e3}o", f.hd, |s| s.hd = !s.hd),
        ("Legendas/CC", f.subtitles, |s| s.subtitles = !s.subtitles),
        ("Creative Commons", f.creative_commons, |s| {
            s.creative_commons = !s.creative_commons;
        }),
        ("360\u{00b0}", f.three_sixty, |s| {
            s.three_sixty = !s.three_sixty
        }),
        ("VR180", f.vr180, |s| s.vr180 = !s.vr180),
        ("3D", f.three_d, |s| s.three_d = !s.three_d),
        ("HDR", f.hdr, |s| s.hdr = !s.hdr),
        ("Local", f.location, |s| s.location = !s.location),
        ("Comprado", f.purchased, |s| s.purchased = !s.purchased),
    ]
}

#[derive(Clone, Copy)]
pub(crate) enum SearchMenuIcon {
    Queue,
    Clock,
    Bookmark,
    Share,
    Flag,
}

pub(crate) fn search_menu_item_ui(
    ui: &mut egui::Ui,
    icon: SearchMenuIcon,
    label: &str,
    palette: &ThemePalette,
) -> egui::Response {
    let desired_w = ui.available_width().max(220.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(desired_w, 36.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, palette.header_button_bg);
    }
    let c = egui::pos2(rect.left() + 20.0, rect.center().y);
    let fg = palette.header_button_fg;
    let stroke = egui::Stroke::new(1.5, fg);
    let painter = ui.painter();

    match icon {
        SearchMenuIcon::Queue => {
            for (idx, dy) in [-4.5_f32, 0.0, 4.5].into_iter().enumerate() {
                let x_end = if idx == 2 { c.x + 2.0 } else { c.x + 7.5 };
                painter.line_segment(
                    [egui::pos2(c.x - 7.5, c.y + dy), egui::pos2(x_end, c.y + dy)],
                    stroke,
                );
            }
            let tri = vec![
                egui::pos2(c.x + 4.0, c.y + 1.8),
                egui::pos2(c.x + 4.0, c.y + 7.2),
                egui::pos2(c.x + 8.5, c.y + 4.5),
            ];
            painter.add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
        }
        SearchMenuIcon::Clock => {
            painter.circle_stroke(c, 7.2, stroke);
            painter.line_segment([c, egui::pos2(c.x, c.y - 4.0)], stroke);
            painter.line_segment([c, egui::pos2(c.x + 3.2, c.y + 2.0)], stroke);
        }
        SearchMenuIcon::Bookmark => {
            let pts = vec![
                egui::pos2(c.x - 5.0, c.y - 7.0),
                egui::pos2(c.x + 5.0, c.y - 7.0),
                egui::pos2(c.x + 5.0, c.y + 7.0),
                egui::pos2(c.x, c.y + 3.2),
                egui::pos2(c.x - 5.0, c.y + 7.0),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
        SearchMenuIcon::Share => {
            let arrow = vec![
                egui::pos2(c.x + 1.5, c.y - 6.5),
                egui::pos2(c.x + 7.8, c.y - 0.8),
                egui::pos2(c.x + 1.5, c.y + 4.8),
            ];
            painter.add(egui::Shape::line(arrow, stroke));
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(c.x - 7.5, c.y + 5.8),
                    egui::pos2(c.x - 5.5, c.y + 0.8),
                    egui::pos2(c.x - 1.5, c.y - 0.8),
                    egui::pos2(c.x + 7.0, c.y - 0.8),
                ],
                stroke,
            ));
        }
        SearchMenuIcon::Flag => {
            painter.line_segment(
                [
                    egui::pos2(c.x - 5.2, c.y - 7.2),
                    egui::pos2(c.x - 5.2, c.y + 7.5),
                ],
                stroke,
            );
            let flag = vec![
                egui::pos2(c.x - 5.2, c.y - 6.8),
                egui::pos2(c.x + 5.8, c.y - 6.8),
                egui::pos2(c.x + 5.8, c.y + 1.2),
                egui::pos2(c.x - 5.2, c.y + 1.2),
            ];
            painter.add(egui::Shape::closed_line(flag, stroke));
        }
    }

    painter.text(
        egui::pos2(rect.left() + 42.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        fg,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn paint_search_filters_button(
    ui: &mut egui::Ui,
    open: bool,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let desired = egui::vec2(92.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let fill = if active {
        palette.active_button_bg
    } else if open || response.hovered() {
        palette.header_button_bg
    } else {
        Color32::TRANSPARENT
    };
    let fg = if active {
        palette.active_button_fg
    } else {
        palette.header_button_fg
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 16.0, fill);
    }

    let painter = ui.painter();
    painter.text(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Filtros",
        egui::FontId::proportional(13.5),
        fg,
    );

    let icon_c = egui::pos2(rect.right() - 20.0, rect.center().y);
    let stroke = egui::Stroke::new(1.5, fg);
    let top_y = icon_c.y - 3.5;
    let bot_y = icon_c.y + 3.5;
    painter.line_segment(
        [
            egui::pos2(icon_c.x - 7.0, top_y),
            egui::pos2(icon_c.x + 7.0, top_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(icon_c.x - 7.0, bot_y),
            egui::pos2(icon_c.x + 7.0, bot_y),
        ],
        stroke,
    );
    let knob_bg = if fill != Color32::TRANSPARENT {
        fill
    } else {
        palette.card_bg
    };
    painter.circle_filled(egui::pos2(icon_c.x + 2.5, top_y), 2.5, knob_bg);
    painter.circle_stroke(egui::pos2(icon_c.x + 2.5, top_y), 2.5, stroke);
    painter.circle_filled(egui::pos2(icon_c.x - 2.5, bot_y), 2.5, knob_bg);
    painter.circle_stroke(egui::pos2(icon_c.x - 2.5, bot_y), 2.5, stroke);

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn format_search_card_subline(
    video: &Video,
    metadata: Option<&VideoMetadata>,
    is_channel: bool,
    is_playlist: bool,
    is_live: bool,
) -> Option<String> {
    if is_channel {
        let mut parts = Vec::new();
        let handle = video.channel.trim();
        if !handle.is_empty() && !handle.eq_ignore_ascii_case(video.title.trim()) {
            parts.push(handle.to_owned());
        }
        if let Some(subs) = metadata
            .and_then(|m| m.subscriber_count_text.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(subs.to_owned());
        } else if let Some(text) = metadata
            .and_then(|m| m.view_count_text.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(text.to_owned());
        }
        return (!parts.is_empty()).then(|| parts.join(" \u{2022} "));
    }

    if is_playlist {
        let mut parts = Vec::new();
        if !video.channel.trim().is_empty() {
            parts.push(video.channel.trim().to_owned());
        }
        parts.push("Playlist".to_owned());
        if let Some(count) = metadata
            .and_then(|m| m.playlist_count.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(count.to_owned());
        }
        return Some(parts.join(" \u{2022} "));
    }

    if is_live {
        if let Some(text) = metadata
            .and_then(|m| m.view_count_text.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(text.to_owned());
        }
        if let Some(count) = metadata.and_then(|m| m.view_count) {
            let formatted = if count >= 1_000 {
                format!(
                    "{} assistindo",
                    recommendations::format_youtube_compact_views(count)
                )
            } else {
                format!("{count} assistindo")
            };
            return Some(formatted);
        }
        return Some("Transmitindo ao vivo".to_owned());
    }

    let mut parts = Vec::new();
    if let Some(text) = metadata
        .and_then(|m| m.view_count_text.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(text.to_owned());
    } else if let Some(views) = metadata.and_then(|m| m.view_count) {
        parts.push(recommendations::format_youtube_compact_views(views));
    }

    if let Some(pub_text) = metadata
        .and_then(|m| m.published_time_text.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(pub_text.to_owned());
    } else if let Some(rel_date) = metadata
        .and_then(|m| m.upload_date.as_deref())
        .and_then(|date| recommendations::format_youtube_relative_time(date, None))
    {
        parts.push(rel_date);
    }

    (!parts.is_empty()).then(|| parts.join(" \u{2022} "))
}

pub(crate) fn paint_live_now_badge(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(76.0, 20.0), egui::Sense::hover());
    let red = Color32::from_rgb(204, 0, 0);
    ui.painter().rect_filled(rect, 4.0, red);
    let c = egui::pos2(rect.left() + 13.0, rect.center().y);
    let fg = Color32::WHITE;
    ui.painter().circle_filled(c, 1.8, fg);
    let stroke = egui::Stroke::new(1.2, fg);
    for (r, start, end) in [
        (3.8, -55.0_f32, 55.0_f32),
        (3.8, 125.0, 235.0),
        (6.0, -52.0, 52.0),
        (6.0, 128.0, 232.0),
    ] {
        let arc = paint_arc_points(c, r, start, end, 6);
        ui.painter().add(egui::Shape::line(arc, stroke));
    }
    ui.painter().text(
        egui::pos2(rect.left() + 24.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "AO VIVO",
        egui::FontId::proportional(10.5),
        fg,
    );
}

pub(crate) fn paint_search_metadata_badge(ui: &mut egui::Ui, label: &str, palette: &ThemePalette) {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return;
    }
    let width = (trimmed.chars().count() as f32 * 6.5 + 14.0).max(28.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, palette.header_button_bg);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        trimmed,
        egui::FontId::proportional(11.0),
        palette.neutral_status_fg,
    );
}

pub(crate) fn paint_short_tag_overlay(ui: &egui::Ui, rect: egui::Rect) {
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + 8.0,
            (rect.bottom() - 28.0).max(rect.top() + 4.0),
        ),
        egui::vec2(56.0, 20.0),
    );
    ui.painter().rect_filled(
        badge_rect,
        4.0,
        Color32::from_rgba_unmultiplied(15, 15, 15, 224),
    );
    ui.painter().text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        "SHORTS",
        egui::FontId::proportional(10.5),
        Color32::WHITE,
    );
}

pub(crate) fn format_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}
