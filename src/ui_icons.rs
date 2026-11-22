use crate::*;
use eframe::egui::{self, Color32, RichText};

#[allow(dead_code)]
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
    Movies,
    Playables,
    YouTubeLabs,
    ReportHistory,
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
    Explore,
    Incognito,
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
        YtSidebarIcon::Movies => {
            let body = egui::Rect::from_center_size(
                egui::pos2(center.x, center.y + 1.2),
                egui::vec2(14.5, 10.5),
            );
            if filled {
                painter.rect_filled(body, 2.0, color);
                let top = egui::Rect::from_min_max(
                    egui::pos2(body.left(), center.y - 6.6),
                    egui::pos2(body.right(), center.y - 3.8),
                );
                painter.rect_filled(top, 1.2, color);
            } else {
                painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
                painter.line_segment(
                    [
                        egui::pos2(body.left(), body.top() - 2.6),
                        egui::pos2(body.right(), body.top() - 2.6),
                    ],
                    thin,
                );
                for x_off in [-4.0_f32, 0.5, 5.0] {
                    painter.line_segment(
                        [
                            egui::pos2(center.x + x_off, body.top() - 2.6),
                            egui::pos2(center.x + x_off - 2.2, body.top()),
                        ],
                        thin,
                    );
                }
            }
        }
        YtSidebarIcon::Playables => {
            // Geometric tile grid inspired by YouTube Playables ("Sala de Jogos").
            let offsets = [
                (-4.6_f32, -4.6_f32),
                (0.0, -4.6),
                (4.6, -4.6),
                (-4.6, 0.0),
                (0.0, 0.0),
                (-4.6, 4.6),
                (0.0, 4.6),
                (4.6, 4.6),
            ];
            for (dx, dy) in offsets {
                let tile = egui::Rect::from_center_size(
                    egui::pos2(center.x + dx, center.y + dy),
                    egui::vec2(3.2, 3.2),
                );
                if filled {
                    painter.rect_filled(tile, 0.6, color);
                } else {
                    painter.rect_stroke(
                        tile,
                        0.6,
                        egui::Stroke::new(1.25, color),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            let tilted = egui::Shape::convex_polygon(
                vec![
                    egui::pos2(center.x + 4.8, center.y - 2.4),
                    egui::pos2(center.x + 7.2, center.y),
                    egui::pos2(center.x + 4.8, center.y + 2.4),
                    egui::pos2(center.x + 2.4, center.y),
                ],
                if filled { color } else { Color32::TRANSPARENT },
                if filled {
                    egui::Stroke::NONE
                } else {
                    egui::Stroke::new(1.25, color)
                },
            );
            painter.add(tilted);
        }
        YtSidebarIcon::YouTubeLabs => {
            // Erlenmeyer flask with bubbling accent.
            let flask = vec![
                egui::pos2(center.x - 2.2, center.y - 6.8),
                egui::pos2(center.x + 2.2, center.y - 6.8),
                egui::pos2(center.x + 2.2, center.y - 2.8),
                egui::pos2(center.x + 6.8, center.y + 5.0),
                egui::pos2(center.x + 5.6, center.y + 6.8),
                egui::pos2(center.x - 5.6, center.y + 6.8),
                egui::pos2(center.x - 6.8, center.y + 5.0),
                egui::pos2(center.x - 2.2, center.y - 2.8),
            ];
            painter.line_segment(
                [
                    egui::pos2(center.x - 3.4, center.y - 6.8),
                    egui::pos2(center.x + 3.4, center.y - 6.8),
                ],
                stroke,
            );
            if filled {
                painter.add(egui::Shape::convex_polygon(
                    flask,
                    color,
                    egui::Stroke::NONE,
                ));
            } else {
                painter.add(egui::Shape::closed_line(flask, stroke));
                painter.line_segment(
                    [
                        egui::pos2(center.x - 4.0, center.y + 2.4),
                        egui::pos2(center.x + 4.0, center.y + 2.4),
                    ],
                    thin,
                );
            }
        }
        YtSidebarIcon::ReportHistory => {
            // Flag icon for report history.
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.8, center.y - 6.8),
                    egui::pos2(center.x - 4.8, center.y + 7.2),
                ],
                stroke,
            );
            let flag = vec![
                egui::pos2(center.x - 4.8, center.y - 6.4),
                egui::pos2(center.x + 5.8, center.y - 6.4),
                egui::pos2(center.x + 5.8, center.y + 1.2),
                egui::pos2(center.x - 4.8, center.y + 1.2),
            ];
            if filled {
                painter.add(egui::Shape::convex_polygon(flag, color, egui::Stroke::NONE));
            } else {
                painter.add(egui::Shape::closed_line(flag, stroke));
            }
        }
        YtSidebarIcon::Explore => {
            paint_explore_icon(painter, center, color);
        }
        YtSidebarIcon::Incognito => {
            paint_incognito_icon(painter, center, color);
        }
    }
}

pub(crate) fn paint_settings_gear_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    let thin = egui::Stroke::new(1.3, color);
    painter.circle_stroke(center, 2.8, stroke);
    painter.circle_stroke(center, 6.2, thin);
    for i in 0..8 {
        let a = (i as f32 * 45.0).to_radians();
        let dx = a.cos();
        let dy = a.sin();
        painter.line_segment(
            [
                egui::pos2(center.x + dx * 6.0, center.y + dy * 6.0),
                egui::pos2(center.x + dx * 8.2, center.y + dy * 8.2),
            ],
            stroke,
        );
    }
}

pub(crate) fn paint_notifications_bell_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
    unread_count: usize,
) {
    paint_yt_icon(painter, center, YtSidebarIcon::Bell, color, false);
    if unread_count > 0 {
        let badge_label = if unread_count > 9 {
            "9+".to_owned()
        } else {
            unread_count.to_string()
        };
        let badge_w = if unread_count > 9 { 18.0 } else { 14.0 };
        let badge_h = 14.0;
        let badge_center = egui::pos2(center.x + 6.0, center.y - 5.5);
        let badge_rect = egui::Rect::from_center_size(badge_center, egui::vec2(badge_w, badge_h));
        painter.rect_filled(badge_rect, 7.0, Color32::from_rgb(204, 0, 0));
        painter.text(
            badge_center,
            egui::Align2::CENTER_CENTER,
            badge_label,
            egui::FontId::proportional(9.5),
            Color32::WHITE,
        );
    }
}

pub(crate) fn paint_grid_view_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = egui::Stroke::new(1.4, color);
    for (dx, dy) in [(-3.6_f32, -3.6_f32), (3.6, -3.6), (-3.6, 3.6), (3.6, 3.6)] {
        let cell = egui::Rect::from_center_size(
            egui::pos2(center.x + dx, center.y + dy),
            egui::vec2(5.4, 5.4),
        );
        painter.rect_stroke(cell, 1.2, stroke, egui::StrokeKind::Inside);
    }
}

pub(crate) fn paint_list_view_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    for dy in [-4.6_f32, 0.0, 4.6] {
        painter.circle_filled(egui::pos2(center.x - 6.0, center.y + dy), 1.35, color);
        painter.line_segment(
            [
                egui::pos2(center.x - 2.6, center.y + dy),
                egui::pos2(center.x + 6.8, center.y + dy),
            ],
            stroke,
        );
    }
}

pub(crate) fn paint_incognito_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = egui::Stroke::new(1.45, color);
    let crown = vec![
        egui::pos2(center.x - 5.2, center.y - 1.2),
        egui::pos2(center.x - 3.4, center.y - 6.8),
        egui::pos2(center.x, center.y - 5.6),
        egui::pos2(center.x + 3.4, center.y - 6.8),
        egui::pos2(center.x + 5.2, center.y - 1.2),
    ];
    painter.add(egui::Shape::closed_line(crown, stroke));
    painter.line_segment(
        [
            egui::pos2(center.x - 8.2, center.y - 1.0),
            egui::pos2(center.x + 8.2, center.y - 1.0),
        ],
        egui::Stroke::new(1.7, color),
    );
    let left_lens = egui::pos2(center.x - 3.6, center.y + 4.0);
    let right_lens = egui::pos2(center.x + 3.6, center.y + 4.0);
    painter.circle_stroke(left_lens, 2.5, stroke);
    painter.circle_stroke(right_lens, 2.5, stroke);
    painter.line_segment(
        [
            egui::pos2(left_lens.x + 2.5, left_lens.y - 0.3),
            egui::pos2(right_lens.x - 2.5, right_lens.y - 0.3),
        ],
        stroke,
    );
}

pub(crate) fn paint_explore_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = egui::Stroke::new(1.45, color);
    painter.circle_stroke(center, 7.6, stroke);
    let needle = vec![
        egui::pos2(center.x + 3.8, center.y - 3.8),
        egui::pos2(center.x + 1.4, center.y + 1.4),
        egui::pos2(center.x - 3.8, center.y + 3.8),
        egui::pos2(center.x - 1.4, center.y - 1.4),
    ];
    painter.add(egui::Shape::closed_line(needle, stroke));
    painter.circle_filled(center, 1.1, color);
}

pub(crate) fn paint_channel_bell_all_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    let thin = egui::Stroke::new(1.25, color);
    let bell = vec![
        egui::pos2(center.x - 5.6, center.y + 3.2),
        egui::pos2(center.x - 4.2, center.y + 1.0),
        egui::pos2(center.x - 4.0, center.y - 2.6),
        egui::pos2(center.x - 2.4, center.y - 5.6),
        egui::pos2(center.x, center.y - 6.6),
        egui::pos2(center.x + 2.4, center.y - 5.6),
        egui::pos2(center.x + 4.0, center.y - 2.6),
        egui::pos2(center.x + 4.2, center.y + 1.0),
        egui::pos2(center.x + 5.6, center.y + 3.2),
    ];
    painter.add(egui::Shape::convex_polygon(bell, color, egui::Stroke::NONE));
    let clapper = paint_arc_points(egui::pos2(center.x, center.y + 3.8), 2.1, 10.0, 170.0, 8);
    painter.add(egui::Shape::line(clapper, stroke));
    let left_wave = paint_arc_points(
        egui::pos2(center.x - 2.2, center.y - 2.0),
        5.8,
        145.0,
        225.0,
        7,
    );
    let right_wave = paint_arc_points(
        egui::pos2(center.x + 2.2, center.y - 2.0),
        5.8,
        -45.0,
        35.0,
        7,
    );
    painter.add(egui::Shape::line(left_wave, thin));
    painter.add(egui::Shape::line(right_wave, thin));
}

pub(crate) fn paint_channel_bell_personalized_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    paint_yt_icon(painter, center, YtSidebarIcon::Bell, color, false);
}

pub(crate) fn paint_channel_bell_none_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    paint_yt_icon(painter, center, YtSidebarIcon::Bell, color, false);
    painter.line_segment(
        [
            egui::pos2(center.x - 6.6, center.y - 6.2),
            egui::pos2(center.x + 6.6, center.y + 6.2),
        ],
        egui::Stroke::new(1.6, color),
    );
}

pub(crate) fn paint_channel_notify_bell_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
    level: settings::ChannelNotifyLevel,
) {
    match level {
        settings::ChannelNotifyLevel::All => paint_channel_bell_all_icon(painter, center, color),
        settings::ChannelNotifyLevel::Personalized => {
            paint_channel_bell_personalized_icon(painter, center, color);
        }
        settings::ChannelNotifyLevel::None => paint_channel_bell_none_icon(painter, center, color),
    }
}

pub(crate) fn paint_focus_ring_if_focused(
    ui: &egui::Ui,
    response: &egui::Response,
    rounding: impl Into<egui::CornerRadius>,
) {
    if !response.has_focus() {
        return;
    }
    let corner_radius: egui::CornerRadius = rounding.into();
    let ring_color = if ui.visuals().dark_mode {
        Color32::from_rgb(62, 166, 255)
    } else {
        Color32::from_rgb(6, 95, 212)
    };
    ui.painter().rect_stroke(
        response.rect.expand(1.5),
        corner_radius,
        egui::Stroke::new(2.0, ring_color),
        egui::StrokeKind::Outside,
    );
}

pub(crate) fn layout_clamped_galley(
    ui: &egui::Ui,
    text: &str,
    max_width: f32,
    font_id: egui::FontId,
    color: egui::Color32,
    max_lines: usize,
) -> std::sync::Arc<egui::Galley> {
    let max_lines = max_lines.max(1);
    let wrap_w = max_width.max(24.0);
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let layout_with = |candidate: String, break_anywhere: bool| {
        let mut job = egui::text::LayoutJob::simple(candidate, font_id.clone(), color, wrap_w);
        job.wrap.max_width = wrap_w;
        job.wrap.max_rows = usize::MAX;
        job.wrap.break_anywhere = break_anywhere;
        egui::WidgetText::from(job).into_galley(
            ui,
            Some(egui::TextWrapMode::Wrap),
            wrap_w,
            egui::FontSelection::Default,
        )
    };

    let full = layout_with(normalized.clone(), false);
    if full.rows.len() <= max_lines && full.size().x <= wrap_w + 2.0 {
        return full;
    }

    let chars: Vec<char> = normalized.chars().collect();
    let break_anywhere = full.size().x > wrap_w + 2.0;
    let mut lo = 1usize;
    let mut hi = chars.len();
    let mut best_text = String::from("…");
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let prefix: String = chars[..mid].iter().collect();
        let candidate = format!("{}…", prefix.trim_end());
        let galley = layout_with(candidate.clone(), break_anywhere);
        if galley.rows.len() <= max_lines && galley.size().x <= wrap_w + 2.0 {
            best_text = candidate;
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    layout_with(best_text, break_anywhere)
}

pub(crate) fn layout_two_line_clamped_galley(
    ui: &egui::Ui,
    text: &str,
    max_width: f32,
    font_id: egui::FontId,
    color: egui::Color32,
) -> std::sync::Arc<egui::Galley> {
    layout_clamped_galley(ui, text, max_width, font_id, color, 2)
}

pub(crate) fn card_hover_wash_shapes(
    rect: egui::Rect,
    corner_radius: f32,
    hover_t: f32,
    palette: &ThemePalette,
) -> (egui::Shape, egui::Shape) {
    let t = hover_t.clamp(0.0, 1.0);
    if t <= 0.01 {
        return (egui::Shape::Noop, egui::Shape::Noop);
    }
    let wash_rect = rect.expand(6.0 * t);
    let wash_fill = palette.card_alt_bg.gamma_multiply(0.72 * t);
    let wash_stroke = palette.card_alt_stroke.gamma_multiply(0.75 * t);
    let cr = egui::CornerRadius::from(corner_radius + 4.0);
    (
        egui::Shape::rect_filled(wash_rect, cr, wash_fill),
        egui::Shape::rect_stroke(
            wash_rect,
            cr,
            egui::Stroke::new(1.0, wash_stroke),
            egui::StrokeKind::Inside,
        ),
    )
}

#[allow(dead_code)]
pub(crate) fn paint_card_hover_wash(
    ui: &egui::Ui,
    rect: egui::Rect,
    corner_radius: f32,
    hover_t: f32,
    palette: &ThemePalette,
) {
    let (fill_shape, stroke_shape) = card_hover_wash_shapes(rect, corner_radius, hover_t, palette);
    if !matches!(fill_shape, egui::Shape::Noop) {
        ui.painter().add(fill_shape);
        ui.painter().add(stroke_shape);
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
    paint_focus_ring_if_focused(ui, &response, 8.0);
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
    paint_focus_ring_if_focused(ui, &response, 13.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn paint_filter_sliders_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    for (dy, knob_dx) in [(-4.2_f32, -2.6_f32), (0.0, 2.8), (4.2, -1.6)] {
        let y = center.y + dy;
        painter.line_segment(
            [egui::pos2(center.x - 6.5, y), egui::pos2(center.x + 6.5, y)],
            stroke,
        );
        painter.circle_filled(egui::pos2(center.x + knob_dx, y), 1.9, color);
    }
}

pub(crate) fn paint_shorts_heart_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
    filled: bool,
) {
    let left_lobe = egui::pos2(center.x - 4.1, center.y - 2.3);
    let right_lobe = egui::pos2(center.x + 4.1, center.y - 2.3);
    let lobe_r = 4.55;
    if filled {
        painter.circle_filled(left_lobe, lobe_r, color);
        painter.circle_filled(right_lobe, lobe_r, color);
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(center.x - 8.4, center.y - 1.1),
                egui::pos2(center.x - 3.6, center.y - 5.2),
                egui::pos2(center.x, center.y - 2.6),
                egui::pos2(center.x + 3.6, center.y - 5.2),
                egui::pos2(center.x + 8.4, center.y - 1.1),
                egui::pos2(center.x, center.y + 8.0),
            ],
            color,
            egui::Stroke::NONE,
        ));
    } else {
        let mut pts = Vec::with_capacity(28);
        pts.push(egui::pos2(center.x, center.y - 3.8));
        // Left upper arch counter-clockwise toward outer cheek
        for step in 1..=8 {
            let t = step as f32 / 8.0;
            let angle = std::f32::consts::PI * (0.12 + t * 0.96);
            pts.push(egui::pos2(
                left_lobe.x + lobe_r * angle.cos(),
                left_lobe.y - lobe_r * angle.sin(),
            ));
        }
        pts.push(egui::pos2(center.x - 7.8, center.y + 0.6));
        pts.push(egui::pos2(center.x, center.y + 7.9));
        pts.push(egui::pos2(center.x + 7.8, center.y + 0.6));
        // Right upper arch back to center cleft
        for step in (1..=8).rev() {
            let t = step as f32 / 8.0;
            let angle = std::f32::consts::PI * (0.12 + t * 0.96);
            pts.push(egui::pos2(
                right_lobe.x - lobe_r * angle.cos(),
                right_lobe.y - lobe_r * angle.sin(),
            ));
        }
        painter.add(egui::Shape::closed_line(
            pts,
            egui::Stroke::new(1.85, color),
        ));
    }
}

pub(crate) fn paint_shorts_comment_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.75, color);
    let bubble = vec![
        egui::pos2(center.x - 6.8, center.y - 7.0),
        egui::pos2(center.x + 6.8, center.y - 7.0),
        egui::pos2(center.x + 8.0, center.y - 5.8),
        egui::pos2(center.x + 8.0, center.y + 3.2),
        egui::pos2(center.x + 6.8, center.y + 4.4),
        egui::pos2(center.x - 2.2, center.y + 4.4),
        egui::pos2(center.x - 6.2, center.y + 8.0),
        egui::pos2(center.x - 6.2, center.y + 4.4),
        egui::pos2(center.x - 6.8, center.y + 4.4),
        egui::pos2(center.x - 8.0, center.y + 3.2),
        egui::pos2(center.x - 8.0, center.y - 5.8),
    ];
    painter.add(egui::Shape::closed_line(bubble, stroke));
    painter.line_segment(
        [
            egui::pos2(center.x - 4.2, center.y - 2.6),
            egui::pos2(center.x + 4.2, center.y - 2.6),
        ],
        egui::Stroke::new(1.5, color),
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 4.2, center.y + 0.8),
            egui::pos2(center.x + 1.8, center.y + 0.8),
        ],
        egui::Stroke::new(1.5, color),
    );
}

pub(crate) fn paint_shorts_share_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    // Forward curved arrow pointing right
    let curve = vec![
        egui::pos2(center.x - 7.2, center.y + 6.8),
        egui::pos2(center.x - 6.2, center.y + 2.2),
        egui::pos2(center.x - 3.6, center.y - 0.6),
        egui::pos2(center.x + 0.4, center.y - 1.6),
        egui::pos2(center.x + 2.6, center.y - 1.6),
        egui::pos2(center.x + 2.6, center.y - 6.4),
        egui::pos2(center.x + 8.6, center.y - 0.2),
        egui::pos2(center.x + 2.6, center.y + 5.8),
        egui::pos2(center.x + 2.6, center.y + 1.4),
        egui::pos2(center.x - 0.8, center.y + 1.4),
        egui::pos2(center.x - 4.2, center.y + 2.8),
    ];
    painter.add(egui::Shape::closed_line(
        curve,
        egui::Stroke::new(1.65, color),
    ));
}

pub(crate) fn paint_shorts_arrow_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
    up: bool,
) {
    let dir = if up { -1.0 } else { 1.0 };
    let stroke = egui::Stroke::new(1.85, color);
    let tip = egui::pos2(center.x, center.y + dir * 6.5);
    let tail = egui::pos2(center.x, center.y - dir * 6.5);
    painter.line_segment([tail, tip], stroke);
    painter.line_segment(
        [egui::pos2(center.x - 4.8, center.y + dir * 1.4), tip],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(center.x + 4.8, center.y + dir * 1.4), tip],
        stroke,
    );
}

pub(crate) fn paint_shorts_more_dots_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    for dy in [-4.5_f32, 0.0, 4.5] {
        painter.circle_filled(egui::pos2(center.x, center.y + dy), 1.55, color);
    }
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

pub(crate) fn paint_queued_badge(ui: &egui::Ui, rect: egui::Rect, offset_for_novo: bool) {
    let x = rect.left() + if offset_for_novo { 50.0 } else { 8.0 };
    let badge_rect =
        egui::Rect::from_min_size(egui::pos2(x, rect.top() + 8.0), egui::vec2(48.0, 20.0));
    if badge_rect.right() > rect.right() - 8.0 {
        return;
    }
    ui.painter().rect_filled(
        badge_rect,
        4.0,
        Color32::from_rgba_unmultiplied(28, 110, 210, 226),
    );
    ui.painter().text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Na fila",
        egui::FontId::proportional(11.0),
        Color32::WHITE,
    );
}

pub(crate) fn paint_card_hover_highlight(
    ui: &egui::Ui,
    rect: egui::Rect,
    corner_radius: f32,
    hover_t: f32,
    palette: &ThemePalette,
) {
    if hover_t <= 0.01 {
        return;
    }
    ui.painter().rect_stroke(
        rect.expand(1.5),
        corner_radius + 1.5,
        egui::Stroke::new(
            1.4,
            palette
                .info_accent_fg
                .gamma_multiply(0.45 * hover_t.clamp(0.0, 1.0)),
        ),
        egui::StrokeKind::Outside,
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

pub(crate) fn paint_youtube_logo_badge_at(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(255, 0, 51));
    let center = rect.center();
    let triangle = vec![
        egui::pos2(center.x - 3.6, center.y - 4.6),
        egui::pos2(center.x - 3.6, center.y + 4.6),
        egui::pos2(center.x + 5.0, center.y),
    ];
    painter.add(egui::Shape::convex_polygon(
        triangle,
        Color32::WHITE,
        egui::Stroke::NONE,
    ));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowControlKind {
    Minimize,
    Maximize,
    Restore,
    Close,
}

pub(crate) fn paint_window_control_button(
    ui: &mut egui::Ui,
    kind: WindowControlKind,
    tooltip: &str,
    palette: &ThemePalette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 32.0), egui::Sense::click());
    let hovered = response.hovered();
    if hovered {
        let hover_bg = if kind == WindowControlKind::Close {
            Color32::from_rgb(232, 17, 35)
        } else {
            palette.header_button_bg
        };
        ui.painter().rect_filled(rect, 6.0, hover_bg);
    }
    let icon_color = if hovered && kind == WindowControlKind::Close {
        Color32::WHITE
    } else if hovered {
        palette.header_button_fg
    } else {
        palette.icon_fg
    };
    let c = rect.center();
    let stroke = egui::Stroke::new(1.25, icon_color);
    match kind {
        WindowControlKind::Minimize => {
            ui.painter().line_segment(
                [
                    egui::pos2(c.x - 5.0, c.y + 0.5),
                    egui::pos2(c.x + 5.0, c.y + 0.5),
                ],
                stroke,
            );
        }
        WindowControlKind::Maximize => {
            let box_rect = egui::Rect::from_center_size(c, egui::vec2(9.5, 9.5));
            ui.painter()
                .rect_stroke(box_rect, 1.2, stroke, egui::StrokeKind::Inside);
        }
        WindowControlKind::Restore => {
            let front = egui::Rect::from_center_size(
                egui::pos2(c.x - 1.2, c.y + 1.2),
                egui::vec2(7.6, 7.6),
            );
            ui.painter()
                .rect_stroke(front, 1.0, stroke, egui::StrokeKind::Inside);
            ui.painter().add(egui::Shape::line(
                vec![
                    egui::pos2(front.left() + 2.0, front.top()),
                    egui::pos2(front.left() + 2.0, front.top() - 2.2),
                    egui::pos2(front.right() + 2.2, front.top() - 2.2),
                    egui::pos2(front.right() + 2.2, front.bottom() - 2.0),
                    egui::pos2(front.right(), front.bottom() - 2.0),
                ],
                egui::Stroke::new(1.2, icon_color),
            ));
        }
        WindowControlKind::Close => {
            let close_stroke = egui::Stroke::new(1.35, icon_color);
            ui.painter().line_segment(
                [
                    egui::pos2(c.x - 4.5, c.y - 4.5),
                    egui::pos2(c.x + 4.5, c.y + 4.5),
                ],
                close_stroke,
            );
            ui.painter().line_segment(
                [
                    egui::pos2(c.x + 4.5, c.y - 4.5),
                    egui::pos2(c.x - 4.5, c.y + 4.5),
                ],
                close_stroke,
            );
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

#[allow(dead_code)]
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
    paint_subtle_avatar_ring(ui, rect.center(), diameter * 0.5);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional((diameter * 0.39).clamp(10.0, 16.0)),
        Color32::WHITE,
    );
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
    paint_focus_ring_if_focused(ui, &response, 10.0);
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
        egui::pos2(pill_rect.left() + 24.0, pill_rect.center().y),
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
    paint_focus_ring_if_focused(ui, &response, 10.0);
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
            rect.shrink2(egui::vec2(2.0, 4.0)),
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
    paint_focus_ring_if_focused(ui, &response, 10.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarChannelIndicator {
    None,
    Unseen,
    Live,
}

pub(crate) fn resolve_sidebar_channel_indicator(
    has_unseen: bool,
    is_live: bool,
) -> SidebarChannelIndicator {
    if is_live {
        SidebarChannelIndicator::Live
    } else if has_unseen {
        SidebarChannelIndicator::Unseen
    } else {
        SidebarChannelIndicator::None
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sidebar_channel_item(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    avatars: &mut thumbnails::AvatarCache,
    avatar_url: Option<&str>,
    channel_name: &str,
    active: bool,
    has_unseen: bool,
    is_live: bool,
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
    paint_subtle_avatar_ring(ui, avatar_center, 12.0);
    let display_name = truncate_text_chars(channel_name, 16);
    ui.painter().text(
        egui::pos2(pill_rect.left() + 48.0, pill_rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_name,
        egui::FontId::proportional(14.0),
        palette.channel_fg,
    );
    let indicator_center = egui::pos2(pill_rect.right() - 12.0, pill_rect.center().y);
    match resolve_sidebar_channel_indicator(has_unseen, is_live) {
        SidebarChannelIndicator::None => {}
        SidebarChannelIndicator::Unseen => {
            ui.painter()
                .circle_filled(indicator_center, 2.5, Color32::from_rgb(62, 166, 255));
        }
        SidebarChannelIndicator::Live => {
            let live_red = Color32::from_rgb(255, 0, 51);
            let arc_stroke = egui::Stroke::new(1.2, live_red);
            ui.painter().circle_filled(indicator_center, 2.2, live_red);
            ui.painter().add(egui::Shape::line(
                paint_arc_points(indicator_center, 4.8, -45.0, 45.0, 6),
                arc_stroke,
            ));
            ui.painter().add(egui::Shape::line(
                paint_arc_points(indicator_center, 4.8, 135.0, 225.0, 6),
                arc_stroke,
            ));
        }
    }
    paint_focus_ring_if_focused(ui, &response, 10.0);
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

pub(crate) fn paint_playlist_stack_tab(
    ui: &egui::Ui,
    rect: egui::Rect,
    stack_color: Option<Color32>,
    dark_theme: bool,
) {
    let tab_color = stack_color.unwrap_or_else(|| {
        if dark_theme {
            Color32::from_rgb(76, 76, 80)
        } else {
            Color32::from_rgb(196, 196, 202)
        }
    });
    let tab_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 8.0, rect.top()),
        egui::pos2(
            (rect.right() - 8.0).max(rect.left() + 10.0),
            (rect.bottom() - 1.5).max(rect.top() + 2.0),
        ),
    );
    ui.painter().rect_filled(
        tab_rect,
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        tab_color,
    );
}

pub(crate) fn paint_playlist_badge_overlay(ui: &egui::Ui, rect: egui::Rect, label: &str) {
    let fg = Color32::from_rgb(241, 241, 241);
    let font = egui::FontId::proportional(11.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), fg);
    let text_w = galley.size().x;
    let badge_w = (text_w + 26.0).max(46.0);
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
        Color32::from_rgba_unmultiplied(15, 15, 15, 212),
    );
    let icon_x = badge_rect.left() + 7.0;
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
    ui.painter().galley(
        egui::pos2(icon_x + 13.0, cy - galley.size().y * 0.5),
        galley,
        fg,
    );
}

pub(crate) fn paint_mix_badge_overlay(ui: &egui::Ui, rect: egui::Rect, label: &str) {
    let fg = Color32::from_rgb(241, 241, 241);
    let font = egui::FontId::proportional(11.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), fg);
    let text_w = galley.size().x;
    let badge_w = (text_w + 26.0).max(46.0);
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
        Color32::from_rgba_unmultiplied(15, 15, 15, 212),
    );
    let icon_x = badge_rect.left() + 7.0;
    let cy = badge_rect.center().y;
    let tri = vec![
        egui::pos2(icon_x, cy - 3.6),
        egui::pos2(icon_x, cy + 3.6),
        egui::pos2(icon_x + 5.4, cy),
    ];
    ui.painter()
        .add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
    ui.painter().line_segment(
        [
            egui::pos2(icon_x + 7.2, cy - 3.2),
            egui::pos2(icon_x + 7.2, cy + 3.2),
        ],
        egui::Stroke::new(1.3, fg),
    );
    ui.painter().line_segment(
        [
            egui::pos2(icon_x + 9.8, cy - 4.2),
            egui::pos2(icon_x + 9.8, cy + 4.2),
        ],
        egui::Stroke::new(1.3, fg),
    );
    ui.painter().galley(
        egui::pos2(icon_x + 13.0, cy - galley.size().y * 0.5),
        galley,
        fg,
    );
}

pub(crate) fn paint_playlist_loop_icon(ui: &egui::Ui, rect: egui::Rect, fg: Color32) {
    let p = ui.painter();
    let c = rect.center();
    let s = (rect.width().min(rect.height()) * 0.42).max(5.0);
    let stroke = egui::Stroke::new(1.7, fg);
    p.line_segment(
        [
            egui::pos2(c.x - s * 0.75, c.y - s * 0.45),
            egui::pos2(c.x + s * 0.65, c.y - s * 0.45),
        ],
        stroke,
    );
    p.line_segment(
        [
            egui::pos2(c.x - s * 0.75, c.y - s * 0.45),
            egui::pos2(c.x - s * 0.75, c.y - s * 0.05),
        ],
        stroke,
    );
    let top_arrow = vec![
        egui::pos2(c.x + s * 0.45, c.y - s * 0.85),
        egui::pos2(c.x + s * 0.45, c.y - s * 0.05),
        egui::pos2(c.x + s * 0.92, c.y - s * 0.45),
    ];
    p.add(egui::Shape::convex_polygon(
        top_arrow,
        fg,
        egui::Stroke::NONE,
    ));

    p.line_segment(
        [
            egui::pos2(c.x - s * 0.65, c.y + s * 0.45),
            egui::pos2(c.x + s * 0.75, c.y + s * 0.45),
        ],
        stroke,
    );
    p.line_segment(
        [
            egui::pos2(c.x + s * 0.75, c.y + s * 0.05),
            egui::pos2(c.x + s * 0.75, c.y + s * 0.45),
        ],
        stroke,
    );
    let bot_arrow = vec![
        egui::pos2(c.x - s * 0.45, c.y + s * 0.05),
        egui::pos2(c.x - s * 0.45, c.y + s * 0.85),
        egui::pos2(c.x - s * 0.92, c.y + s * 0.45),
    ];
    p.add(egui::Shape::convex_polygon(
        bot_arrow,
        fg,
        egui::Stroke::NONE,
    ));
}

pub(crate) fn paint_playlist_shuffle_icon(ui: &egui::Ui, rect: egui::Rect, fg: Color32) {
    let p = ui.painter();
    let c = rect.center();
    let s = (rect.width().min(rect.height()) * 0.42).max(5.0);
    let stroke = egui::Stroke::new(1.7, fg);
    // Upper path crossing to lower-right
    p.line_segment(
        [
            egui::pos2(c.x - s * 0.9, c.y - s * 0.45),
            egui::pos2(c.x - s * 0.3, c.y - s * 0.45),
        ],
        stroke,
    );
    p.line_segment(
        [
            egui::pos2(c.x - s * 0.3, c.y - s * 0.45),
            egui::pos2(c.x + s * 0.3, c.y + s * 0.45),
        ],
        stroke,
    );
    p.line_segment(
        [
            egui::pos2(c.x + s * 0.3, c.y + s * 0.45),
            egui::pos2(c.x + s * 0.7, c.y + s * 0.45),
        ],
        stroke,
    );
    // Lower path crossing to upper-right
    p.line_segment(
        [
            egui::pos2(c.x - s * 0.9, c.y + s * 0.45),
            egui::pos2(c.x - s * 0.3, c.y + s * 0.45),
        ],
        stroke,
    );
    p.line_segment(
        [
            egui::pos2(c.x - s * 0.3, c.y + s * 0.45),
            egui::pos2(c.x + s * 0.3, c.y - s * 0.45),
        ],
        stroke,
    );
    p.line_segment(
        [
            egui::pos2(c.x + s * 0.3, c.y - s * 0.45),
            egui::pos2(c.x + s * 0.7, c.y - s * 0.45),
        ],
        stroke,
    );
    let upper_arrow = vec![
        egui::pos2(c.x + s * 0.5, c.y - s * 0.8),
        egui::pos2(c.x + s * 0.5, c.y - s * 0.1),
        egui::pos2(c.x + s * 0.95, c.y - s * 0.45),
    ];
    let lower_arrow = vec![
        egui::pos2(c.x + s * 0.5, c.y + s * 0.1),
        egui::pos2(c.x + s * 0.5, c.y + s * 0.8),
        egui::pos2(c.x + s * 0.95, c.y + s * 0.45),
    ];
    p.add(egui::Shape::convex_polygon(
        upper_arrow,
        fg,
        egui::Stroke::NONE,
    ));
    p.add(egui::Shape::convex_polygon(
        lower_arrow,
        fg,
        egui::Stroke::NONE,
    ));
}

pub(crate) fn paint_lock_icon(ui: &egui::Ui, rect: egui::Rect, fg: Color32) {
    let p = ui.painter();
    let c = rect.center();
    let s = (rect.width().min(rect.height()) * 0.42).max(4.5);
    let body = egui::Rect::from_center_size(
        egui::pos2(c.x, c.y + s * 0.25),
        egui::vec2(s * 1.35, s * 1.05),
    );
    p.rect_stroke(
        body,
        2.0,
        egui::Stroke::new(1.3, fg),
        egui::StrokeKind::Middle,
    );
    let shackle_w = s * 0.75;
    let shackle_top = c.y - s * 0.75;
    let shackle_bot = body.top();
    let pts = [
        egui::pos2(c.x - shackle_w * 0.5, shackle_bot),
        egui::pos2(c.x - shackle_w * 0.5, shackle_top + s * 0.2),
        egui::pos2(c.x, shackle_top),
        egui::pos2(c.x + shackle_w * 0.5, shackle_top + s * 0.2),
        egui::pos2(c.x + shackle_w * 0.5, shackle_bot),
    ];
    for pair in pts.windows(2) {
        p.line_segment([pair[0], pair[1]], egui::Stroke::new(1.3, fg));
    }
}

pub(crate) fn paint_unlisted_link_icon(ui: &egui::Ui, rect: egui::Rect, fg: Color32) {
    let p = ui.painter();
    let c = rect.center();
    let s = (rect.width().min(rect.height()) * 0.44).max(4.5);
    let stroke = egui::Stroke::new(1.3, fg);
    let left_ring = egui::Rect::from_center_size(
        egui::pos2(c.x - s * 0.35, c.y),
        egui::vec2(s * 0.9, s * 0.65),
    );
    let right_ring = egui::Rect::from_center_size(
        egui::pos2(c.x + s * 0.35, c.y),
        egui::vec2(s * 0.9, s * 0.65),
    );
    p.rect_stroke(left_ring, 3.0, stroke, egui::StrokeKind::Middle);
    p.rect_stroke(right_ring, 3.0, stroke, egui::StrokeKind::Middle);
}

pub(crate) fn paint_small_play_triangle(
    ui: &egui::Ui,
    center: egui::Pos2,
    radius: f32,
    fg: Color32,
) {
    let tri = vec![
        egui::pos2(center.x - radius * 0.65, center.y - radius * 0.9),
        egui::pos2(center.x - radius * 0.65, center.y + radius * 0.9),
        egui::pos2(center.x + radius * 0.95, center.y),
    ];
    ui.painter()
        .add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
}

pub(crate) fn paint_close_x_icon(ui: &egui::Ui, rect: egui::Rect, fg: Color32) {
    let c = rect.center();
    let r = (rect.width().min(rect.height()) * 0.30).max(4.0);
    let stroke = egui::Stroke::new(1.7, fg);
    ui.painter().line_segment(
        [egui::pos2(c.x - r, c.y - r), egui::pos2(c.x + r, c.y + r)],
        stroke,
    );
    ui.painter().line_segment(
        [egui::pos2(c.x - r, c.y + r), egui::pos2(c.x + r, c.y - r)],
        stroke,
    );
}

pub(crate) fn paint_chevron_down_icon(ui: &egui::Ui, rect: egui::Rect, fg: Color32, up: bool) {
    let c = rect.center();
    let r = (rect.width().min(rect.height()) * 0.28).max(3.5);
    let dy = if up { -r * 0.55 } else { r * 0.55 };
    let stroke = egui::Stroke::new(1.7, fg);
    ui.painter().line_segment(
        [egui::pos2(c.x - r, c.y - dy), egui::pos2(c.x, c.y + dy)],
        stroke,
    );
    ui.painter().line_segment(
        [egui::pos2(c.x, c.y + dy), egui::pos2(c.x + r, c.y - dy)],
        stroke,
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

#[allow(dead_code)]
type FeatureFilterToggle = fn(&mut search::SearchFeatureFilters);
#[allow(dead_code)]
type FeatureFilterSpec = (&'static str, bool, FeatureFilterToggle);

#[allow(dead_code)]
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
    Download,
    Flag,
    Trash,
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
        SearchMenuIcon::Trash => {
            paint_history_trash_icon(painter, c, fg);
        }
        SearchMenuIcon::Download => {
            painter.line_segment(
                [egui::pos2(c.x, c.y - 6.5), egui::pos2(c.x, c.y + 3.0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(c.x - 3.5, c.y - 0.5), egui::pos2(c.x, c.y + 3.0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(c.x + 3.5, c.y - 0.5), egui::pos2(c.x, c.y + 3.0)],
                stroke,
            );
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(c.x - 6.5, c.y + 3.5),
                    egui::pos2(c.x - 6.5, c.y + 6.5),
                    egui::pos2(c.x + 6.5, c.y + 6.5),
                    egui::pos2(c.x + 6.5, c.y + 3.5),
                ],
                stroke,
            ));
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

pub(crate) fn paint_history_trash_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    let thin = egui::Stroke::new(1.3, color);
    painter.line_segment(
        [
            egui::pos2(center.x - 6.5, center.y - 5.2),
            egui::pos2(center.x + 6.5, center.y - 5.2),
        ],
        stroke,
    );
    painter.add(egui::Shape::line(
        vec![
            egui::pos2(center.x - 2.6, center.y - 5.2),
            egui::pos2(center.x - 2.0, center.y - 7.6),
            egui::pos2(center.x + 2.0, center.y - 7.6),
            egui::pos2(center.x + 2.6, center.y - 5.2),
        ],
        thin,
    ));
    let body = egui::Rect::from_min_max(
        egui::pos2(center.x - 4.8, center.y - 5.2),
        egui::pos2(center.x + 4.8, center.y + 7.2),
    );
    painter.rect_stroke(body, 1.5, stroke, egui::StrokeKind::Inside);
    for dx in [-1.8_f32, 1.8_f32] {
        painter.line_segment(
            [
                egui::pos2(center.x + dx, center.y - 2.4),
                egui::pos2(center.x + dx, center.y + 4.4),
            ],
            thin,
        );
    }
}

pub(crate) fn paint_history_pause_bars_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    for dx in [-3.4_f32, 3.4_f32] {
        let bar = egui::Rect::from_center_size(
            egui::pos2(center.x + dx, center.y),
            egui::vec2(3.6, 12.6),
        );
        painter.rect_stroke(bar, 0.8, stroke, egui::StrokeKind::Inside);
    }
}

pub(crate) fn paint_history_play_icon(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    let pts = vec![
        egui::pos2(center.x - 4.2, center.y - 6.4),
        egui::pos2(center.x + 6.0, center.y),
        egui::pos2(center.x - 4.2, center.y + 6.4),
    ];
    painter.add(egui::Shape::closed_line(pts, stroke));
}

pub(crate) fn paint_history_search_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    color: Color32,
) {
    let stroke = egui::Stroke::new(1.6, color);
    let lens = egui::pos2(center.x - 1.4, center.y - 1.4);
    painter.circle_stroke(lens, 5.8, stroke);
    painter.line_segment(
        [
            egui::pos2(lens.x + 4.2, lens.y + 4.2),
            egui::pos2(lens.x + 8.2, lens.y + 8.2),
        ],
        stroke,
    );
}

pub(crate) fn paint_report_history_hero_illustration(ui: &mut egui::Ui, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter_at(rect);
    let card = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 8.0, rect.top() + 26.0),
        egui::vec2((size.x - 36.0).max(80.0), (size.y - 36.0).max(60.0)),
    );
    // Blue player card base.
    p.rect_filled(card, 14.0, Color32::from_rgb(0, 92, 230));
    // Aqua hillside accent waves inside the frame.
    let wave_top = vec![
        egui::pos2(card.left() + 8.0, card.bottom() - 18.0),
        egui::pos2(card.left() + 26.0, card.top() + 18.0),
        egui::pos2(card.center().x + 6.0, card.top() + 34.0),
        egui::pos2(card.right() - 10.0, card.top() + 14.0),
        egui::pos2(card.right() - 10.0, card.bottom() - 18.0),
    ];
    p.add(egui::Shape::convex_polygon(
        wave_top,
        Color32::from_rgb(0, 178, 198),
        egui::Stroke::NONE,
    ));
    let wave_front = vec![
        egui::pos2(card.left() + 10.0, card.bottom() - 14.0),
        egui::pos2(card.center().x - 12.0, card.center().y - 2.0),
        egui::pos2(card.right() - 14.0, card.bottom() - 28.0),
        egui::pos2(card.right() - 14.0, card.bottom() - 14.0),
    ];
    p.add(egui::Shape::convex_polygon(
        wave_front,
        Color32::from_rgb(42, 212, 168),
        egui::Stroke::NONE,
    ));
    // Red scrubber bar + white playhead knob at the bottom of the frame.
    let track_y = card.bottom() - 11.0;
    let track_l = card.left() + 14.0;
    let track_r = card.right() - 14.0;
    let knob_x = track_l + (track_r - track_l) * 0.44;
    p.line_segment(
        [egui::pos2(knob_x, track_y), egui::pos2(track_r, track_y)],
        egui::Stroke::new(3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 110)),
    );
    p.line_segment(
        [egui::pos2(track_l, track_y), egui::pos2(knob_x, track_y)],
        egui::Stroke::new(3.2, Color32::from_rgb(255, 0, 51)),
    );
    p.circle_filled(
        egui::pos2(knob_x, track_y),
        4.6,
        Color32::from_rgb(255, 0, 51),
    );
    p.circle_filled(egui::pos2(knob_x, track_y), 2.3, Color32::WHITE);

    // Purple magnifier overlapping top-right corner of the video card.
    let lens_center = egui::pos2(card.right() - 12.0, card.top() + 8.0);
    let lens_radius = 27.0;
    p.circle_filled(
        lens_center,
        lens_radius,
        Color32::from_rgba_unmultiplied(15, 36, 115, 235),
    );
    p.circle_filled(
        egui::pos2(lens_center.x - 10.0, lens_center.y + 6.0),
        5.5,
        Color32::WHITE,
    );
    p.circle_filled(
        egui::pos2(lens_center.x - 2.0, lens_center.y - 3.0),
        2.5,
        Color32::WHITE,
    );
    p.circle_stroke(
        lens_center,
        lens_radius,
        egui::Stroke::new(6.0, Color32::from_rgb(122, 44, 214)),
    );
    p.line_segment(
        [
            egui::pos2(lens_center.x + 19.0, lens_center.y + 19.0),
            egui::pos2(lens_center.x + 33.0, lens_center.y + 37.0),
        ],
        egui::Stroke::new(7.5, Color32::from_rgb(122, 44, 214)),
    );
}

#[allow(dead_code)]
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

pub(crate) fn paint_live_now_badge_localized(ui: &mut egui::Ui, pt: bool) {
    let label = if pt { "AO VIVO" } else { "LIVE" };
    let width = if pt { 76.0 } else { 58.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::hover());
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
        label,
        egui::FontId::proportional(10.5),
        fg,
    );
}

pub(crate) fn paint_live_now_badge(ui: &mut egui::Ui) {
    paint_live_now_badge_localized(ui, true);
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

pub(crate) fn youtube_chip_button(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    with_chevron: bool,
    _palette: &ThemePalette,
    dark_theme: bool,
) -> egui::Response {
    let fg = match (active, dark_theme) {
        (true, true) => Color32::from_rgb(15, 15, 15),
        (true, false) => Color32::WHITE,
        (false, true) => Color32::from_rgb(241, 241, 241),
        (false, false) => Color32::from_rgb(15, 15, 15),
    };
    let font = egui::FontId::proportional(13.5);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, fg);
    let pad_l = 12.0;
    let pad_r = if with_chevron { 10.0 } else { 12.0 };
    let chevron_extra = if with_chevron { 16.0 } else { 0.0 };
    let chip_w = (pad_l + galley.size().x + chevron_extra + pad_r).ceil();
    let chip_h = 32.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(chip_w, chip_h), egui::Sense::click());
    let bg = match (active, dark_theme, response.hovered()) {
        (true, true, _) => Color32::from_rgb(241, 241, 241),
        (true, false, _) => Color32::from_rgb(15, 15, 15),
        (false, true, true) => Color32::from_rgb(63, 63, 63),
        (false, true, false) => Color32::from_rgb(39, 39, 39),
        (false, false, true) => Color32::from_rgb(229, 229, 229),
        (false, false, false) => Color32::from_rgb(242, 242, 242),
    };
    ui.painter().rect_filled(rect, 8.0, bg);
    let text_pos = egui::pos2(rect.left() + pad_l, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, fg);
    if with_chevron {
        let cx = rect.right() - pad_r - 5.0;
        let cy = rect.center().y + 1.0;
        ui.painter().add(egui::Shape::line(
            vec![
                egui::pos2(cx - 4.0, cy - 2.2),
                egui::pos2(cx, cy + 2.0),
                egui::pos2(cx + 4.0, cy - 2.2),
            ],
            egui::Stroke::new(1.5, fg),
        ));
    }
    paint_focus_ring_if_focused(ui, &response, 8.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn styled_pill_button(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let (bg, fg, stroke_col) = if active {
        (
            palette.active_button_bg,
            palette.active_button_fg,
            palette.active_button_bg,
        )
    } else {
        (
            palette.header_button_bg,
            palette.header_button_fg,
            palette.card_alt_stroke,
        )
    };
    let btn = egui::Button::new(RichText::new(label).size(13.0).strong().color(fg))
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, stroke_col))
        .corner_radius(16)
        .min_size(egui::vec2(0.0, 30.0));
    let resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
    paint_focus_ring_if_focused(ui, &resp, 16.0);
    resp
}

pub(crate) fn styled_compact_button(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let (bg, fg, stroke_col) = if active {
        (
            palette.active_button_bg,
            palette.active_button_fg,
            palette.active_button_bg,
        )
    } else {
        (
            palette.header_button_bg,
            palette.header_button_fg,
            palette.card_alt_stroke,
        )
    };
    let btn = egui::Button::new(RichText::new(label).size(12.0).strong().color(fg))
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, stroke_col))
        .corner_radius(13)
        .min_size(egui::vec2(0.0, 25.0));
    let resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
    paint_focus_ring_if_focused(ui, &resp, 13.0);
    resp
}

pub(crate) fn styled_compact_button_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let (bg, fg, stroke_col) = if active {
        (
            palette.active_button_bg,
            palette.active_button_fg,
            palette.active_button_bg,
        )
    } else {
        (
            palette.header_button_bg,
            palette.header_button_fg,
            palette.card_alt_stroke,
        )
    };
    let text_color = if enabled {
        fg
    } else {
        palette.neutral_status_fg
    };
    let btn = egui::Button::new(RichText::new(label).size(12.0).strong().color(text_color))
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, stroke_col))
        .corner_radius(13)
        .min_size(egui::vec2(0.0, 25.0));
    let resp = ui.add_enabled(enabled, btn);
    paint_focus_ring_if_focused(ui, &resp, 13.0);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub(crate) fn styled_danger_button(
    ui: &mut egui::Ui,
    label: &str,
    palette: &ThemePalette,
) -> egui::Response {
    let btn = egui::Button::new(
        RichText::new(label)
            .size(12.0)
            .strong()
            .color(palette.error_banner_fg),
    )
    .fill(palette.error_bg)
    .stroke(egui::Stroke::new(1.0, palette.error_stroke))
    .corner_radius(14)
    .min_size(egui::vec2(0.0, 26.0));
    let resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
    paint_focus_ring_if_focused(ui, &resp, 14.0);
    resp
}

pub(crate) fn styled_ghost_button(
    ui: &mut egui::Ui,
    label: &str,
    palette: &ThemePalette,
) -> egui::Response {
    let btn = egui::Button::new(
        RichText::new(label)
            .size(12.0)
            .strong()
            .color(palette.neutral_status_fg),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
    .corner_radius(13)
    .min_size(egui::vec2(0.0, 25.0));
    let resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
    paint_focus_ring_if_focused(ui, &resp, 13.0);
    resp
}

pub(crate) fn styled_icon_chip_button(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    palette: &ThemePalette,
) -> egui::Response {
    let fg = if enabled {
        palette.header_button_fg
    } else {
        palette.neutral_status_fg
    };
    let btn = egui::Button::new(RichText::new(label).size(11.5).strong().color(fg))
        .fill(palette.header_button_bg)
        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
        .corner_radius(8)
        .min_size(egui::vec2(26.0, 24.0));
    let resp = ui.add_enabled(enabled, btn);
    paint_focus_ring_if_focused(ui, &resp, 8.0);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub(crate) fn is_actionable_inline_message(msg: &str) -> bool {
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let progress_or_info_prefixes = [
        "pesquisando",
        "pesquise ",
        "carregando",
        "consultando ",
        "atualizando",
        "sincronizando",
        "enviando ",
        "velocidade:",
        "adicionado ",
        "você gostou",
        "você não gostou",
        "avaliação removida",
        "inscrito ",
        "inscrição removida",
        "loop a-b limpo",
        "captura salva:",
        "link do canal copiado:",
        "searching",
        "loading",
        "fetching",
        "refreshing",
        "syncing",
        "added to queue",
        "speed:",
    ];
    if progress_or_info_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return false;
    }
    let error_or_warning_terms = [
        "erro",
        "falha",
        "falhou",
        "não foi possível",
        "nao foi possivel",
        "indisponível",
        "indisponivel",
        "inválid",
        "invalid",
        "expir",
        "somente no windows",
        "entre com google",
        "sign in",
        "limite de 100",
        "error",
        "failed",
        "unavailable",
        "unable to",
        "could not",
        "cannot",
        "aviso",
        "warning",
    ];
    error_or_warning_terms
        .iter()
        .any(|term| lower.contains(term))
}

pub(crate) fn inline_error_banner_ui(ui: &mut egui::Ui, msg: &str, palette: &ThemePalette) -> bool {
    let mut dismissed = false;
    egui::Frame::new()
        .fill(palette.error_bg)
        .stroke(egui::Stroke::new(1.0, palette.error_stroke))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(msg).size(12.5).color(palette.error_banner_fg));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close_btn = egui::Button::new(
                        RichText::new("✕").size(11.0).color(palette.error_banner_fg),
                    )
                    .fill(Color32::TRANSPARENT)
                    .frame(false);
                    if ui
                        .add(close_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        dismissed = true;
                    }
                });
            });
        });
    dismissed
}

pub(crate) fn page_header_ui(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: Option<&str>,
    palette: &ThemePalette,
    add_actions: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(
                RichText::new(title)
                    .size(20.0)
                    .strong()
                    .color(palette.channel_fg),
            );
            if let Some(sub) = subtitle.map(str::trim).filter(|s| !s.is_empty()) {
                ui.label(
                    RichText::new(sub)
                        .size(12.5)
                        .color(palette.neutral_status_fg),
                );
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            add_actions(ui);
        });
    });
    ui.add_space(10.0);
    let divider_w = ui.available_width().max(1.0);
    let (div_rect, _) = ui.allocate_exact_size(egui::vec2(divider_w, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(div_rect, 0.0, palette.card_alt_stroke.gamma_multiply(0.7));
    ui.add_space(12.0);
}

pub(crate) fn empty_state_ui(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    title: &str,
    subtitle: &str,
    cta_label: Option<&str>,
    palette: &ThemePalette,
) -> bool {
    let mut cta_clicked = false;
    let available_w = ui.available_width().max(1.0);
    let card_w = (available_w - 24.0).clamp(160.0, 460.0).min(available_w);
    let side_margin = ((available_w - card_w) * 0.5).max(0.0);
    let inner_w = (card_w - 56.0).max(120.0);

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(side_margin);
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(28, 24))
            .show(ui, |ui| {
                ui.set_width(inner_w);
                ui.set_max_width(inner_w);
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    if let Some(icon) = icon {
                        let badge_size = 48.0;
                        let (badge_rect, _) = ui.allocate_exact_size(
                            egui::vec2(badge_size, badge_size),
                            egui::Sense::hover(),
                        );
                        ui.painter().circle_filled(
                            badge_rect.center(),
                            badge_size * 0.5,
                            palette.info_box_bg,
                        );
                        ui.painter().circle_stroke(
                            badge_rect.center(),
                            badge_size * 0.5,
                            egui::Stroke::new(1.0, palette.card_alt_stroke),
                        );
                        let icon_rect = egui::Rect::from_center_size(
                            badge_rect.center(),
                            egui::vec2(22.0, 22.0),
                        );
                        ui.put(icon_rect, icon.image(palette.neutral_status_fg, 22.0));
                        ui.add_space(2.0);
                    }
                    ui.label(
                        RichText::new(title)
                            .size(15.5)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    if !subtitle.trim().is_empty() {
                        ui.label(
                            RichText::new(subtitle)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                    if let Some(label) = cta_label.map(str::trim).filter(|s| !s.is_empty()) {
                        ui.add_space(6.0);
                        if styled_pill_button(ui, label, true, palette).clicked() {
                            cta_clicked = true;
                        }
                    }
                });
            });
    });
    ui.add_space(16.0);
    cta_clicked
}

pub(crate) fn app_modal_window(
    ctx: &egui::Context,
    id: egui::Id,
    title: &str,
    open: &mut bool,
    default_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let screen = ctx.content_rect();
    let scrim_id = id.with("modal_scrim_fade");
    let fade_t = ctx.animate_bool_responsive(scrim_id, *open);
    if !*open && fade_t <= 0.01 {
        return;
    }

    let scrim_alpha = (140.0 * fade_t).round().clamp(0.0, 255.0) as u8;
    let mut backdrop_clicked = false;
    egui::Area::new(id.with("modal_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(*open)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(
                rect,
                0.0,
                Color32::from_rgba_unmultiplied(0, 0, 0, scrim_alpha),
            );
            if *open && resp.clicked() {
                backdrop_clicked = true;
            }
        });

    if !*open {
        return;
    }

    let dark_mode = ctx.global_style().visuals.dark_mode;
    let palette = ThemePalette::for_theme(if dark_mode { Theme::Dark } else { Theme::Light });

    let modal_w = default_width
        .min((screen.width() - 48.0).max(240.0))
        .max(240.0);
    let inner_w = (modal_w - 40.0).max(180.0);
    let mut close_clicked = false;
    let close_btn_id = id.with("modal_close_btn");
    let any_popup_open = egui::Popup::is_any_open(ctx);

    let win_resp = egui::Window::new(title)
        .id(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .default_width(modal_w)
        .max_width(modal_w)
        .order(egui::Order::Foreground)
        .frame(
            egui::Frame::NONE
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(20, 16))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_rgba_unmultiplied(0, 0, 0, 115),
                }),
        )
        .show(ctx, |ui| {
            ui.set_width(inner_w);
            ui.horizontal(|ui| {
                let title_max_w = (inner_w - 36.0).max(120.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(title_max_w, 26.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(title)
                                    .size(18.0)
                                    .strong()
                                    .color(palette.channel_fg),
                            )
                            .truncate(),
                        );
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.push_id(close_btn_id, |ui| {
                        if styled_icon_chip_button(ui, true, "✕", &palette).clicked() {
                            close_clicked = true;
                        }
                    });
                });
            });
            ui.add_space(10.0);
            let (div_rect, _) =
                ui.allocate_exact_size(egui::vec2(inner_w, 1.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(div_rect, 0.0, palette.card_alt_stroke.gamma_multiply(0.7));
            ui.add_space(12.0);
            add_contents(ui);
        });

    if let Some(inner) = win_resp {
        let modal_layer = inner.response.layer_id;
        ctx.move_to_top(modal_layer);
        // Trap keyboard focus & input within the active modal layer.
        ctx.memory_mut(|mem| mem.set_modal_layer(modal_layer));
    }

    let esc_pressed = !any_popup_open && ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if close_clicked || backdrop_clicked || esc_pressed {
        *open = false;
    }
}

pub(crate) fn styled_menu_item_button(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let fg = if active {
        palette.channel_fg
    } else {
        palette.header_button_fg
    };
    let text = if active {
        RichText::new(label).size(13.0).strong().color(fg)
    } else {
        RichText::new(label).size(13.0).color(fg)
    };
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        visuals.widgets.hovered.weak_bg_fill = palette.header_button_bg;
        visuals.widgets.hovered.bg_fill = palette.header_button_bg;
        visuals.widgets.active.weak_bg_fill = palette.header_button_bg;
        visuals.widgets.active.bg_fill = palette.header_button_bg;

        let mut btn = egui::Button::new(text)
            .corner_radius(8)
            .min_size(egui::vec2(0.0, 30.0));
        if active {
            btn = btn
                .fill(palette.info_accent_fg.gamma_multiply(0.16))
                .stroke(egui::Stroke::new(
                    1.0,
                    palette.info_accent_fg.gamma_multiply(0.32),
                ));
        } else {
            btn = btn.stroke(egui::Stroke::NONE);
        }
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            let resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
            paint_focus_ring_if_focused(ui, &resp, 8.0);
            resp
        })
        .inner
    })
    .inner
}

pub(crate) fn paint_subtle_avatar_ring(ui: &egui::Ui, center: egui::Pos2, radius: f32) {
    ui.painter().circle_stroke(
        center,
        radius,
        egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(128, 128, 128, 45)),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        SidebarChannelIndicator, is_actionable_inline_message, resolve_sidebar_channel_indicator,
    };

    #[test]
    fn sidebar_channel_indicator_distinguishes_none_unseen_and_live() {
        assert_eq!(
            resolve_sidebar_channel_indicator(false, false),
            SidebarChannelIndicator::None
        );
        assert_eq!(
            resolve_sidebar_channel_indicator(true, false),
            SidebarChannelIndicator::Unseen
        );
        assert_eq!(
            resolve_sidebar_channel_indicator(false, true),
            SidebarChannelIndicator::Live
        );
        assert_eq!(
            resolve_sidebar_channel_indicator(true, true),
            SidebarChannelIndicator::Live
        );
    }

    #[test]
    fn suppresses_empty_and_progress_status_strings() {
        assert!(!is_actionable_inline_message(""));
        assert!(!is_actionable_inline_message("   "));
        assert!(!is_actionable_inline_message("Pesquisando \"rust\"..."));
        assert!(!is_actionable_inline_message("Carregando inscrições..."));
        assert!(!is_actionable_inline_message(
            "Consultando inscrições: 5 de 80..."
        ));
        assert!(!is_actionable_inline_message(
            "Sincronizando em segundo plano..."
        ));
        assert!(!is_actionable_inline_message("Velocidade: 1.50x"));
        assert!(!is_actionable_inline_message(
            "Adicionado à fila: Meu vídeo"
        ));
        assert!(!is_actionable_inline_message("Loop A-B limpo."));
        assert!(!is_actionable_inline_message("Captura salva: shot.png"));
    }

    #[test]
    fn surfaces_actionable_errors_and_warnings() {
        assert!(is_actionable_inline_message(
            "Erro ao carregar feed inicial: timeout"
        ));
        assert!(is_actionable_inline_message(
            "Não foi possível abrir o vídeo."
        ));
        assert!(is_actionable_inline_message(
            "Entre com Google nas Configurações para curtir vídeos."
        ));
        assert!(is_actionable_inline_message(
            "Loop A-B indisponível em transmissões ao vivo."
        ));
        assert!(is_actionable_inline_message("Failed to open video stream"));
    }

    #[test]
    fn two_line_clamped_galley_never_exceeds_two_lines_and_appends_ellipsis_on_overflow() {
        let ctx = super::egui::Context::default();
        let mut short_rows = 0usize;
        let mut short_text = String::new();
        let mut long_rows = 0usize;
        let mut long_text = String::new();

        let mut out = ctx.run_ui(super::egui::RawInput::default(), |ui| {
            super::egui::CentralPanel::default().show(ui, |ui| {
                let font = super::egui::FontId::proportional(15.0);
                let short = super::layout_two_line_clamped_galley(
                    ui,
                    "Short video title",
                    220.0,
                    font.clone(),
                    super::Color32::WHITE,
                );
                short_rows = short.rows.len();
                short_text = short.job.text.clone();

                let long = super::layout_two_line_clamped_galley(
                    ui,
                    "This is an extraordinarily long YouTube video title that will definitely wrap past two full lines inside a narrow video grid card width",
                    160.0,
                    font,
                    super::Color32::WHITE,
                );
                long_rows = long.rows.len();
                long_text = long.job.text.clone();
            });
        });
        out.textures_delta.clear();

        assert!(short_rows <= 2);
        assert_eq!(short_text, "Short video title");
        assert!(!short_text.ends_with('…'));

        assert_eq!(long_rows, 2);
        assert!(long_text.ends_with('…'));
    }

    #[test]
    fn app_modal_window_renders_headlessly_and_closes_on_escape() {
        let ctx = super::egui::Context::default();
        let modal_id = super::egui::Id::new("test_modal_window");
        let mut open = true;
        let mut body_rendered = false;

        let mut out = ctx.run_ui(super::egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            super::app_modal_window(&ctx, modal_id, "Test Modal", &mut open, 420.0, |ui| {
                body_rendered = true;
                ui.label("Modal contents");
            });
        });
        out.textures_delta.clear();
        assert!(body_rendered);
        assert!(open);

        let mut esc_input = super::egui::RawInput::default();
        esc_input.events.push(super::egui::Event::Key {
            key: super::egui::Key::Escape,
            physical_key: Some(super::egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: super::egui::Modifiers::NONE,
        });
        let mut esc_out = ctx.run_ui(esc_input, |ui| {
            let ctx = ui.ctx().clone();
            super::app_modal_window(&ctx, modal_id, "Test Modal", &mut open, 420.0, |ui| {
                ui.label("Modal contents");
            });
        });
        esc_out.textures_delta.clear();
        assert!(!open);
    }

    #[test]
    fn focus_ring_and_wave3_vector_icons_paint_headlessly_without_panicking() {
        let ctx = super::egui::Context::default();
        let mut out = ctx.run_ui(super::egui::RawInput::default(), |ui| {
            super::egui::CentralPanel::default().show(ui, |ui| {
                let (rect, resp) = ui.allocate_exact_size(
                    super::egui::vec2(24.0, 24.0),
                    super::egui::Sense::click(),
                );
                resp.request_focus();
                super::paint_focus_ring_if_focused(ui, &resp, 8.0);
                let c = rect.center();
                let fg = super::Color32::WHITE;
                super::paint_settings_gear_icon(ui.painter(), c, fg);
                super::paint_notifications_bell_icon(ui.painter(), c, fg, 0);
                super::paint_notifications_bell_icon(ui.painter(), c, fg, 4);
                super::paint_notifications_bell_icon(ui.painter(), c, fg, 15);
                super::paint_grid_view_icon(ui.painter(), c, fg);
                super::paint_list_view_icon(ui.painter(), c, fg);
                super::paint_incognito_icon(ui.painter(), c, fg);
                super::paint_explore_icon(ui.painter(), c, fg);
                super::paint_channel_notify_bell_icon(
                    ui.painter(),
                    c,
                    fg,
                    crate::settings::ChannelNotifyLevel::All,
                );
                super::paint_channel_notify_bell_icon(
                    ui.painter(),
                    c,
                    fg,
                    crate::settings::ChannelNotifyLevel::Personalized,
                );
                super::paint_channel_notify_bell_icon(
                    ui.painter(),
                    c,
                    fg,
                    crate::settings::ChannelNotifyLevel::None,
                );
                super::paint_filter_sliders_icon(ui.painter(), c, fg);
                super::paint_shorts_heart_icon(ui.painter(), c, fg, false);
                super::paint_shorts_heart_icon(ui.painter(), c, fg, true);
                super::paint_shorts_comment_icon(ui.painter(), c, fg);
                super::paint_shorts_share_icon(ui.painter(), c, fg);
                super::paint_shorts_arrow_icon(ui.painter(), c, fg, true);
                super::paint_shorts_arrow_icon(ui.painter(), c, fg, false);
                super::paint_shorts_more_dots_icon(ui.painter(), c, fg);
            });
        });
        out.textures_delta.clear();
    }
}
