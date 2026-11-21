use eframe::egui::{self, Color32};
use std::time::Duration;

const REPAINT_INTERVAL: Duration = Duration::from_millis(28);

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

pub(crate) fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        lerp_u8(a.r(), b.r(), t),
        lerp_u8(a.g(), b.g(), t),
        lerp_u8(a.b(), b.b(), t),
        lerp_u8(a.a(), b.a(), t),
    )
}

pub(crate) fn skeleton_palette(dark_theme: bool) -> (Color32, Color32) {
    if dark_theme {
        (Color32::from_rgb(35, 35, 39), Color32::from_rgb(56, 56, 62))
    } else {
        (
            Color32::from_rgb(227, 227, 232),
            Color32::from_rgb(243, 243, 247),
        )
    }
}

/// Computes a smooth traveling wave intensity `[0.0, 1.0]` for the given rectangle and time.
pub(crate) fn shimmer_factor(time_secs: f64, rect: egui::Rect) -> f32 {
    let phase = (time_secs as f32) * 2.8 - (rect.left() * 0.0028 + rect.top() * 0.0018);
    let wave = phase.sin() * 0.5 + 0.5;
    wave.powf(1.5)
}

pub(crate) fn shimmer_color_for_rect(ui: &egui::Ui, rect: egui::Rect, dark_theme: bool) -> Color32 {
    let time = ui.input(|i| i.time);
    ui.ctx().request_repaint_after(REPAINT_INTERVAL);
    let (base, highlight) = skeleton_palette(dark_theme);
    lerp_color(base, highlight, shimmer_factor(time, rect))
}

pub(crate) fn paint_skeleton_rect(
    ui: &egui::Ui,
    rect: egui::Rect,
    corner_radius: f32,
    dark_theme: bool,
) {
    let color = shimmer_color_for_rect(ui, rect, dark_theme);
    ui.painter().rect_filled(rect, corner_radius, color);
}

pub(crate) fn skeleton_box(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    corner_radius: f32,
    dark_theme: bool,
) -> egui::Rect {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    paint_skeleton_rect(ui, rect, corner_radius, dark_theme);
    rect
}

pub(crate) fn skeleton_circle(ui: &mut egui::Ui, diameter: f32, dark_theme: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
    let color = shimmer_color_for_rect(ui, rect, dark_theme);
    ui.painter()
        .circle_filled(rect.center(), diameter * 0.5, color);
}

pub(crate) fn skeleton_text_line(ui: &mut egui::Ui, width: f32, height: f32, dark_theme: bool) {
    let _ = skeleton_box(
        ui,
        egui::vec2(width.max(12.0), height.max(6.0)),
        (height * 0.45).min(6.0),
        dark_theme,
    );
}

pub(crate) fn home_grid_skeleton(
    ui: &mut egui::Ui,
    available_width: f32,
    columns: usize,
    gap: f32,
    dark_theme: bool,
) {
    let columns = columns.max(1);
    let available = available_width.max(220.0);
    let width =
        ((available - gap * (columns.saturating_sub(1) as f32)) / columns as f32).max(160.0);
    let thumb_height = (width * 9.0 / 16.0).round();

    for _row in 0..3 {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for _col in 0..columns {
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.set_max_width(width);
                    let _ = skeleton_box(ui, egui::vec2(width, thumb_height), 12.0, dark_theme);
                    ui.add_space(10.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        skeleton_circle(ui, 36.0, dark_theme);
                        let line_w = (width - 48.0).max(80.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 7.0;
                            skeleton_text_line(ui, line_w * 0.90, 14.0, dark_theme);
                            skeleton_text_line(ui, line_w * 0.62, 12.0, dark_theme);
                            skeleton_text_line(ui, line_w * 0.44, 11.0, dark_theme);
                        });
                    });
                });
            }
        });
        ui.add_space(24.0);
    }
}

pub(crate) fn shorts_grid_skeleton(ui: &mut egui::Ui, available_width: f32, dark_theme: bool) {
    let gap = 14.0;
    let available = available_width.max(200.0);
    let columns = (((available + gap) / 210.0).floor() as usize).clamp(2, 5);
    let card_w =
        ((available - gap * (columns.saturating_sub(1) as f32)) / columns as f32).max(140.0);
    let card_h = (card_w * 16.0 / 9.0).min(360.0).round();

    for _row in 0..2 {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for _col in 0..columns {
                ui.vertical(|ui| {
                    ui.set_width(card_w);
                    ui.set_max_width(card_w);
                    let _ = skeleton_box(ui, egui::vec2(card_w, card_h), 12.0, dark_theme);
                    ui.add_space(8.0);
                    skeleton_text_line(ui, card_w * 0.85, 13.0, dark_theme);
                    ui.add_space(5.0);
                    skeleton_text_line(ui, card_w * 0.52, 11.0, dark_theme);
                });
            }
        });
        ui.add_space(18.0);
    }
}

pub(crate) fn search_results_skeleton(ui: &mut egui::Ui, available_width: f32, dark_theme: bool) {
    let avail = available_width.max(260.0);
    let stacked = avail < 640.0;
    let thumb_w = if stacked {
        avail.min(420.0)
    } else {
        (avail * 0.34).clamp(240.0, 360.0)
    };
    let thumb_h = (thumb_w * 9.0 / 16.0).round();

    for idx in 0..5 {
        if stacked {
            ui.vertical(|ui| {
                let _ = skeleton_box(ui, egui::vec2(thumb_w, thumb_h), 12.0, dark_theme);
                ui.add_space(10.0);
                let text_w = avail.max(160.0);
                skeleton_text_line(ui, text_w * 0.82, 16.0, dark_theme);
                ui.add_space(6.0);
                skeleton_text_line(ui, text_w * 0.45, 12.0, dark_theme);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    skeleton_circle(ui, 24.0, dark_theme);
                    skeleton_text_line(ui, 120.0, 12.0, dark_theme);
                });
            });
        } else {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let _ = skeleton_box(ui, egui::vec2(thumb_w, thumb_h), 12.0, dark_theme);
                let text_w = (avail - thumb_w - 28.0).max(160.0);
                let title_scale = if idx % 2 == 0 { 0.78 } else { 0.64 };
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    skeleton_text_line(ui, text_w * title_scale, 16.0, dark_theme);
                    skeleton_text_line(ui, text_w * 0.36, 12.0, dark_theme);
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        skeleton_circle(ui, 24.0, dark_theme);
                        skeleton_text_line(ui, 130.0, 12.0, dark_theme);
                    });
                    ui.add_space(4.0);
                    skeleton_text_line(ui, text_w * 0.88, 11.0, dark_theme);
                    skeleton_text_line(ui, text_w * 0.66, 11.0, dark_theme);
                });
            });
        }
        ui.add_space(16.0);
    }
}

pub(crate) fn sidebar_subscriptions_skeleton(ui: &mut egui::Ui, dark_theme: bool) {
    let widths = [128.0_f32, 106.0, 136.0, 118.0, 96.0];
    for w in widths {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.x = 10.0;
            skeleton_circle(ui, 24.0, dark_theme);
            skeleton_text_line(ui, w, 12.0, dark_theme);
        });
        ui.add_space(6.0);
    }
}

pub(crate) fn channel_tab_grid_skeleton(
    ui: &mut egui::Ui,
    max_width: f32,
    compact: bool,
    is_shorts: bool,
    rows: usize,
    dark_theme: bool,
) {
    let gap = 16.0;
    let avail = max_width.max(200.0);
    let cols = if compact {
        1
    } else if is_shorts {
        ((avail + gap) / 210.0).floor().clamp(2.0, 5.0) as usize
    } else {
        ((avail + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
    };
    let card_w = ((avail - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(160.0);
    let thumb_h = if is_shorts {
        (card_w * 16.0 / 9.0).min(360.0).round()
    } else {
        (card_w * 9.0 / 16.0).round()
    };

    for _ in 0..rows.max(1) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for _ in 0..cols {
                ui.vertical(|ui| {
                    ui.set_width(card_w);
                    ui.set_max_width(card_w);
                    let _ = skeleton_box(ui, egui::vec2(card_w, thumb_h), 12.0, dark_theme);
                    ui.add_space(8.0);
                    skeleton_text_line(ui, card_w * 0.86, 13.5, dark_theme);
                    ui.add_space(5.0);
                    skeleton_text_line(ui, card_w * 0.54, 11.5, dark_theme);
                });
            }
        });
        ui.add_space(18.0);
    }
}

pub(crate) fn channel_page_skeleton(
    ui: &mut egui::Ui,
    max_width: f32,
    compact: bool,
    dark_theme: bool,
) {
    let w = max_width.max(240.0);
    let banner_h = if compact { 96.0 } else { 156.0 };
    let _ = skeleton_box(ui, egui::vec2(w, banner_h), 14.0, dark_theme);
    ui.add_space(16.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        let avatar_d = if compact { 56.0 } else { 88.0 };
        skeleton_circle(ui, avatar_d, dark_theme);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            skeleton_text_line(ui, (w * 0.34).clamp(140.0, 260.0), 22.0, dark_theme);
            skeleton_text_line(ui, (w * 0.26).clamp(120.0, 220.0), 13.0, dark_theme);
            ui.add_space(2.0);
            let _ = skeleton_box(ui, egui::vec2(118.0, 32.0), 16.0, dark_theme);
        });
    });

    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        for tab_w in [64.0_f32, 68.0, 62.0, 82.0, 76.0] {
            let _ = skeleton_box(ui, egui::vec2(tab_w, 22.0), 6.0, dark_theme);
        }
    });
    ui.add_space(16.0);
    channel_tab_grid_skeleton(ui, w, compact, false, 2, dark_theme);
}

pub(crate) fn watch_sidebar_skeleton(ui: &mut egui::Ui, width: f32, dark_theme: bool) {
    let target_w = width.max(220.0);
    let thumb_w = 160.0_f32.min(target_w * 0.45);
    let thumb_h = (thumb_w * 9.0 / 16.0).round();
    let text_w = (target_w - thumb_w - 12.0).max(80.0);

    for idx in 0..6 {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let _ = skeleton_box(ui, egui::vec2(thumb_w, thumb_h), 8.0, dark_theme);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let w1 = if idx % 2 == 0 { 0.92 } else { 0.78 };
                skeleton_text_line(ui, text_w * w1, 13.0, dark_theme);
                skeleton_text_line(ui, text_w * 0.65, 13.0, dark_theme);
                ui.add_space(2.0);
                skeleton_text_line(ui, text_w * 0.50, 11.0, dark_theme);
                skeleton_text_line(ui, text_w * 0.38, 11.0, dark_theme);
            });
        });
        ui.add_space(10.0);
    }
}

pub(crate) fn comments_skeleton(ui: &mut egui::Ui, width: f32, count: usize, dark_theme: bool) {
    let avail = width.max(200.0);
    for idx in 0..count.max(1) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            skeleton_circle(ui, 36.0, dark_theme);
            let line_w = (avail - 56.0).max(100.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                skeleton_text_line(ui, (line_w * 0.30).clamp(90.0, 170.0), 12.0, dark_theme);
                skeleton_text_line(
                    ui,
                    line_w * if idx % 2 == 0 { 0.92 } else { 0.80 },
                    13.0,
                    dark_theme,
                );
                skeleton_text_line(
                    ui,
                    line_w * if idx % 2 == 0 { 0.58 } else { 0.44 },
                    13.0,
                    dark_theme,
                );
            });
        });
        ui.add_space(14.0);
    }
}

pub(crate) fn transcript_skeleton(ui: &mut egui::Ui, width: f32, dark_theme: bool) {
    let avail = width.max(180.0);
    for idx in 0..7 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let _ = skeleton_box(ui, egui::vec2(46.0, 20.0), 6.0, dark_theme);
            let frac = if idx % 3 == 0 {
                0.85
            } else if idx % 3 == 1 {
                0.70
            } else {
                0.56
            };
            skeleton_text_line(ui, (avail - 64.0).max(80.0) * frac, 13.0, dark_theme);
        });
        ui.add_space(8.0);
    }
}

pub(crate) fn live_chat_skeleton(ui: &mut egui::Ui, width: f32, dark_theme: bool) {
    let avail = width.max(160.0);
    for idx in 0..6 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            skeleton_circle(ui, 20.0, dark_theme);
            let frac = if idx % 2 == 0 { 0.72 } else { 0.56 };
            skeleton_text_line(ui, (avail - 36.0).max(80.0) * frac, 12.0, dark_theme);
        });
        ui.add_space(8.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shimmer_factor_stays_bounded_and_varies_over_time_and_space() {
        let r1 = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 112.0));
        let r2 = egui::Rect::from_min_size(egui::pos2(360.0, 220.0), egui::vec2(200.0, 112.0));
        let f1_t0 = shimmer_factor(0.0, r1);
        let f1_t1 = shimmer_factor(0.6, r1);
        let f2_t0 = shimmer_factor(0.0, r2);
        assert!((0.0..=1.0).contains(&f1_t0));
        assert!((0.0..=1.0).contains(&f1_t1));
        assert!((0.0..=1.0).contains(&f2_t0));
        assert!((f1_t0 - f1_t1).abs() > 0.01);
        assert!((f1_t0 - f2_t0).abs() > 0.01);
    }

    #[test]
    fn lerp_color_interpolates_endpoints() {
        let a = Color32::from_rgb(35, 35, 39);
        let b = Color32::from_rgb(56, 56, 62);
        assert_eq!(lerp_color(a, b, 0.0), a);
        assert_eq!(lerp_color(a, b, 1.0), b);
        let mid = lerp_color(a, b, 0.5);
        assert!(mid.r() > a.r() && mid.r() < b.r());
    }
}
