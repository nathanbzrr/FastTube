use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke,
    Vec2, epaint::Vertex,
};

pub const OVERLAY_HEIGHT: f32 = 64.0;
pub const OVERLAY_HIDE_DELAY_SECS: f64 = 2.5;
pub const SPEED_PRESETS: [f64; 10] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
const BAR_SIDE_PADDING: f32 = 12.0;
const PROGRESS_HIT_HEIGHT: f32 = 16.0;
const BUTTON_ROW_HEIGHT: f32 = 38.0;

pub fn step_speed_preset(current_speed: f64, increase: bool) -> f64 {
    if !current_speed.is_finite() || current_speed <= 0.0 {
        return 1.0;
    }
    if increase {
        SPEED_PRESETS
            .iter()
            .copied()
            .find(|&preset| preset > current_speed + 0.01)
            .unwrap_or(*SPEED_PRESETS.last().unwrap_or(&3.0))
    } else {
        SPEED_PRESETS
            .iter()
            .rev()
            .copied()
            .find(|&preset| preset < current_speed - 0.01)
            .unwrap_or(SPEED_PRESETS[0])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayMenu {
    Root,
    Speed,
    Quality,
    Subtitles,
    Chapters,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayChapter {
    pub title: String,
    pub time_secs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlaySponsorSegment {
    pub start_secs: f64,
    pub end_secs: f64,
    pub category_label: &'static str,
    pub color: Color32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlaySubtitleOption {
    pub id: String,
    pub label: String,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct OverlayTelemetry {
    pub title: String,
    pub paused: bool,
    pub ended: bool,
    pub muted: bool,
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub buffered_secs: Option<f64>,
    pub volume_percent: f32,
    pub speed: f64,
    pub seek_step_secs: u32,
    pub current_height: Option<u32>,
    pub selected_quality: Option<u32>,
    pub qualities: Vec<u32>,
    pub qualities_loading: bool,
    pub has_queued: bool,
    pub has_active_subtitle: bool,
    pub active_subtitle_text: Option<String>,
    pub subtitles: Vec<OverlaySubtitleOption>,
    pub subtitles_loading: bool,
    pub chapters: Vec<OverlayChapter>,
    pub sponsor_segments: Vec<OverlaySponsorSegment>,
    pub active_sponsor_segment: Option<OverlaySponsorSegment>,
    pub sponsor_skip_toast: Option<String>,
    pub theater: bool,
    pub fullscreen: bool,
    pub mini_player: bool,
    pub autoplay: bool,
    pub loop_video: bool,
    pub loop_a_secs: Option<f64>,
    pub loop_b_secs: Option<f64>,
    pub audio_only: bool,
    pub can_bookmark: bool,
    pub is_live: bool,
    pub is_live_dvr: bool,
    pub at_live_edge: bool,
    pub live_behind_secs: f64,
    pub menu: Option<OverlayMenu>,
}

impl Default for OverlayTelemetry {
    fn default() -> Self {
        Self {
            title: String::new(),
            paused: true,
            ended: false,
            muted: false,
            position_secs: 0.0,
            duration_secs: None,
            buffered_secs: None,
            volume_percent: 100.0,
            speed: 1.0,
            seek_step_secs: 10,
            current_height: None,
            selected_quality: None,
            qualities: Vec::new(),
            qualities_loading: false,
            has_queued: false,
            has_active_subtitle: false,
            active_subtitle_text: None,
            subtitles: Vec::new(),
            subtitles_loading: false,
            chapters: Vec::new(),
            sponsor_segments: Vec::new(),
            active_sponsor_segment: None,
            sponsor_skip_toast: None,
            theater: false,
            fullscreen: false,
            mini_player: false,
            autoplay: false,
            loop_video: false,
            loop_a_secs: None,
            loop_b_secs: None,
            audio_only: false,
            can_bookmark: false,
            is_live: false,
            is_live_dvr: false,
            at_live_edge: false,
            live_behind_secs: 0.0,
            menu: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OverlayStyle {
    pub panel: Color32,
    pub icon: Color32,
    pub text: Color32,
    pub accent: Color32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OverlayAction {
    TogglePlay,
    SeekBack10,
    SeekForward10,
    SeekTo(f64),
    SkipSponsorSegment(f64),
    GoLive,
    ToggleMute,
    SetVolume(f32),
    Next,
    ToggleAutoplay,
    ToggleLoop,
    SetLoopA,
    SetLoopB,
    ClearLoopAB,
    ToggleAudioOnly,
    CaptureScreenshot,
    ToggleCaptions,
    SelectSubtitle(Option<String>),
    SetSpeed(f64),
    SelectQuality(Option<u32>),
    SeekChapter(f64),
    AddBookmark,
    OpenAppSettings,
    SetMenu(Option<OverlayMenu>),
    ToggleMiniPlayer,
    ToggleTheater,
    ToggleFullscreen,
    Close,
}

pub fn overlay_visible(
    pointer_over_video: bool,
    paused: bool,
    ended: bool,
    seconds_since_activity: f64,
) -> bool {
    pointer_over_video || paused || ended || seconds_since_activity < OVERLAY_HIDE_DELAY_SECS
}

pub fn overlay_visible_with_state(
    pointer_over_video: bool,
    paused: bool,
    ended: bool,
    menu_open: bool,
    scrubbing: bool,
    seconds_since_activity: f64,
) -> bool {
    menu_open
        || scrubbing
        || overlay_visible(pointer_over_video, paused, ended, seconds_since_activity)
}

pub fn format_time(seconds: f64) -> String {
    let clamped = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor() as u64
    } else {
        0
    };
    let hours = clamped / 3600;
    let minutes = (clamped % 3600) / 60;
    let secs = clamped % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

const LIVE_EDGE_SNAP_FRACTION: f64 = 0.985;
const LIVE_EDGE_COLOR: Color32 = Color32::from_rgb(255, 0, 51);
const LIVE_BEHIND_DOT_COLOR: Color32 = Color32::from_rgb(160, 160, 166);

pub fn format_live_behind(behind_secs: f64) -> String {
    let secs = if behind_secs.is_finite() && behind_secs > 0.0 {
        behind_secs
    } else {
        0.0
    };
    format!("-{}", format_time(secs))
}

fn format_elapsed_total(position_secs: f64, duration_secs: Option<f64>) -> String {
    let elapsed = format_time(position_secs);
    let total = match duration_secs {
        Some(d) if d.is_finite() && d > 0.0 => format_time(d),
        _ => "--:--".to_owned(),
    };
    format!("{elapsed} / {total}")
}

fn compute_bar_rect(video_rect: Rect) -> Option<Rect> {
    let vw = video_rect.width();
    let vh = video_rect.height();
    if !vw.is_finite() || !vh.is_finite() || vw <= 1.0 || vh <= 1.0 {
        return None;
    }

    let height = OVERLAY_HEIGHT.min(vh);
    let top = (video_rect.max.y - height).max(video_rect.min.y);
    let bar_rect =
        Rect::from_min_max(Pos2::new(video_rect.min.x, top), video_rect.max).intersect(video_rect);

    if bar_rect.width() <= 0.0 || bar_rect.height() <= 0.0 {
        None
    } else {
        Some(bar_rect)
    }
}

fn action_for_progress_click(fraction: f64, duration_secs: Option<f64>) -> Option<OverlayAction> {
    let duration = duration_secs?;
    if !duration.is_finite() || duration <= 0.0 || !fraction.is_finite() {
        return None;
    }
    let clamped = fraction.clamp(0.0, 1.0);
    Some(OverlayAction::SeekTo(clamped * duration))
}

fn action_for_live_progress_click(
    fraction: f64,
    duration_secs: Option<f64>,
    is_live_dvr: bool,
) -> Option<OverlayAction> {
    if !is_live_dvr || !fraction.is_finite() {
        return None;
    }
    let clamped = fraction.clamp(0.0, 1.0);
    if clamped >= LIVE_EDGE_SNAP_FRACTION {
        return Some(OverlayAction::GoLive);
    }
    let duration = duration_secs?;
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    Some(OverlayAction::SeekTo(clamped * duration))
}

pub(crate) fn buffered_progress_ratio(
    position_secs: f64,
    buffered_ahead_secs: Option<f64>,
    duration_secs: Option<f64>,
) -> f32 {
    let Some(duration) = duration_secs.filter(|d| d.is_finite() && *d > 0.0) else {
        return 0.0;
    };
    let Some(ahead) = buffered_ahead_secs.filter(|a| a.is_finite() && *a > 0.0) else {
        return 0.0;
    };
    let pos = if position_secs.is_finite() && position_secs > 0.0 {
        position_secs
    } else {
        0.0
    };
    ((pos + ahead) / duration).clamp(0.0, 1.0) as f32
}

pub(crate) fn ab_loop_marker_fractions(
    loop_a_secs: Option<f64>,
    loop_b_secs: Option<f64>,
    duration_secs: Option<f64>,
) -> (Option<f32>, Option<f32>, Option<(f32, f32)>) {
    let Some(duration) = duration_secs.filter(|d| d.is_finite() && *d > 0.0) else {
        return (None, None, None);
    };
    let a = loop_a_secs
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| (v / duration).clamp(0.0, 1.0) as f32);
    let b = loop_b_secs
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| (v / duration).clamp(0.0, 1.0) as f32);
    let range = match (loop_a_secs, loop_b_secs, a, b) {
        (Some(av), Some(bv), Some(af), Some(bf))
            if av.is_finite() && bv.is_finite() && av < bv && af < bf =>
        {
            Some((af, bf))
        }
        _ => None,
    };
    (a, b, range)
}

pub(crate) fn compute_hover_pill_rect(
    pointer_x: f32,
    strip_top: f32,
    pill_size: Vec2,
    video_rect: Rect,
) -> Rect {
    let margin = 6.0;
    let clamped_w = pill_size
        .x
        .min((video_rect.width() - margin * 2.0).max(20.0));
    let clamped_h = pill_size
        .y
        .min((video_rect.height() - margin * 2.0).max(16.0));
    let size = Vec2::new(clamped_w, clamped_h);
    let min_cx = video_rect.min.x + size.x * 0.5 + margin;
    let max_cx = (video_rect.max.x - size.x * 0.5 - margin).max(min_cx);
    let cx = pointer_x.clamp(min_cx, max_cx);
    let min_cy = video_rect.min.y + size.y * 0.5 + margin;
    let cy = (strip_top - size.y * 0.5 - 6.0).max(min_cy);
    Rect::from_center_size(Pos2::new(cx, cy), size)
}

fn paint_vertical_gradient(
    painter: &egui::Painter,
    rect: Rect,
    top_color: Color32,
    bottom_color: Color32,
) {
    let mut mesh = egui::Mesh::default();
    let idx = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex {
        pos: rect.left_top(),
        uv: egui::epaint::WHITE_UV,
        color: top_color,
    });
    mesh.vertices.push(Vertex {
        pos: rect.right_top(),
        uv: egui::epaint::WHITE_UV,
        color: top_color,
    });
    mesh.vertices.push(Vertex {
        pos: rect.right_bottom(),
        uv: egui::epaint::WHITE_UV,
        color: bottom_color,
    });
    mesh.vertices.push(Vertex {
        pos: rect.left_bottom(),
        uv: egui::epaint::WHITE_UV,
        color: bottom_color,
    });
    mesh.add_triangle(idx, idx + 1, idx + 2);
    mesh.add_triangle(idx, idx + 2, idx + 3);
    painter.add(Shape::mesh(mesh));
}

#[derive(Clone, Copy)]
enum VectorGlyph {
    Play,
    Pause,
    Replay,
    Next,
    SkipBack(u32),
    SkipForward(u32),
    Volume(bool, f32),
    Autoplay(bool),
    AudioOnly(bool),
    Screenshot,
    Captions(bool),
    Settings(bool),
    MiniPlayer(bool),
    Theater(bool),
    Fullscreen(bool),
    Close,
}

fn icon_button(
    ui: &mut egui::Ui,
    size: Vec2,
    glyph: VectorGlyph,
    style: &OverlayStyle,
    enabled: bool,
    active: bool,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    };

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if enabled && (response.hovered() || active) {
            let bg = if active {
                Color32::from_rgba_unmultiplied(255, 255, 255, 36)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 24)
            };
            painter.rect_filled(rect, CornerRadius::same(6), bg);
        }

        let fg = if !enabled {
            style.icon.gamma_multiply(0.35)
        } else if response.hovered() || active {
            Color32::WHITE
        } else {
            style.icon
        };
        let c = rect.center();

        match glyph {
            VectorGlyph::Play => {
                let pts = vec![
                    Pos2::new(c.x - 5.0, c.y - 7.0),
                    Pos2::new(c.x - 5.0, c.y + 7.0),
                    Pos2::new(c.x + 7.0, c.y),
                ];
                painter.add(Shape::convex_polygon(pts, fg, Stroke::NONE));
            }
            VectorGlyph::Pause => {
                let left = Rect::from_min_max(
                    Pos2::new(c.x - 5.5, c.y - 6.5),
                    Pos2::new(c.x - 1.5, c.y + 6.5),
                );
                let right = Rect::from_min_max(
                    Pos2::new(c.x + 1.5, c.y - 6.5),
                    Pos2::new(c.x + 5.5, c.y + 6.5),
                );
                painter.rect_filled(left, 1.0, fg);
                painter.rect_filled(right, 1.0, fg);
            }
            VectorGlyph::Replay => {
                let r = 6.5_f32;
                let segments = 14;
                let start_angle = -std::f32::consts::FRAC_PI_2;
                let end_angle = std::f32::consts::PI * 1.15;
                let pts: Vec<Pos2> = (0..=segments)
                    .map(|i| {
                        let t = i as f32 / segments as f32;
                        let a = start_angle + (end_angle - start_angle) * t;
                        Pos2::new(c.x + a.cos() * r, c.y + a.sin() * r)
                    })
                    .collect();
                painter.add(Shape::line(pts, Stroke::new(1.8, fg)));
                let arrow = vec![
                    Pos2::new(c.x - 4.5, c.y - 6.5),
                    Pos2::new(c.x + 1.0, c.y - 9.5),
                    Pos2::new(c.x + 1.0, c.y - 3.5),
                ];
                painter.add(Shape::convex_polygon(arrow, fg, Stroke::NONE));
            }
            VectorGlyph::Next => {
                let pts = vec![
                    Pos2::new(c.x - 6.5, c.y - 6.0),
                    Pos2::new(c.x - 6.5, c.y + 6.0),
                    Pos2::new(c.x + 2.5, c.y),
                ];
                painter.add(Shape::convex_polygon(pts, fg, Stroke::NONE));
                let bar = Rect::from_min_max(
                    Pos2::new(c.x + 4.0, c.y - 6.0),
                    Pos2::new(c.x + 6.5, c.y + 6.0),
                );
                painter.rect_filled(bar, 1.0, fg);
            }
            VectorGlyph::SkipBack(step) => {
                painter.text(
                    Pos2::new(c.x, c.y + 0.5),
                    egui::Align2::CENTER_CENTER,
                    format!("↺{step}"),
                    FontId::proportional(11.0),
                    fg,
                );
            }
            VectorGlyph::SkipForward(step) => {
                painter.text(
                    Pos2::new(c.x, c.y + 0.5),
                    egui::Align2::CENTER_CENTER,
                    format!("{step}↻"),
                    FontId::proportional(11.0),
                    fg,
                );
            }
            VectorGlyph::Volume(muted, volume) => {
                let speaker = vec![
                    Pos2::new(c.x - 7.5, c.y - 2.5),
                    Pos2::new(c.x - 4.0, c.y - 2.5),
                    Pos2::new(c.x + 0.5, c.y - 6.5),
                    Pos2::new(c.x + 0.5, c.y + 6.5),
                    Pos2::new(c.x - 4.0, c.y + 2.5),
                    Pos2::new(c.x - 7.5, c.y + 2.5),
                ];
                painter.add(Shape::convex_polygon(speaker, fg, Stroke::NONE));
                if muted || volume <= 0.5 {
                    painter.line_segment(
                        [
                            Pos2::new(c.x + 3.0, c.y - 3.5),
                            Pos2::new(c.x + 8.0, c.y + 3.5),
                        ],
                        Stroke::new(1.8, fg),
                    );
                    painter.line_segment(
                        [
                            Pos2::new(c.x + 8.0, c.y - 3.5),
                            Pos2::new(c.x + 3.0, c.y + 3.5),
                        ],
                        Stroke::new(1.8, fg),
                    );
                } else {
                    let arc1 = [
                        Pos2::new(c.x + 3.0, c.y - 3.0),
                        Pos2::new(c.x + 4.5, c.y),
                        Pos2::new(c.x + 3.0, c.y + 3.0),
                    ];
                    painter.add(Shape::line(arc1.to_vec(), Stroke::new(1.6, fg)));
                    if volume > 45.0 {
                        let arc2 = [
                            Pos2::new(c.x + 5.5, c.y - 5.5),
                            Pos2::new(c.x + 8.0, c.y),
                            Pos2::new(c.x + 5.5, c.y + 5.5),
                        ];
                        painter.add(Shape::line(arc2.to_vec(), Stroke::new(1.6, fg)));
                    }
                }
            }
            VectorGlyph::Autoplay(is_on) => {
                let pill = Rect::from_center_size(c, Vec2::new(20.0, 9.0));
                let track_bg = if is_on {
                    style.accent.gamma_multiply(0.85)
                } else {
                    Color32::from_rgba_unmultiplied(255, 255, 255, 70)
                };
                painter.rect_filled(pill, CornerRadius::same(5), track_bg);
                let knob_x = if is_on {
                    pill.right() - 5.0
                } else {
                    pill.left() + 5.0
                };
                let knob_center = Pos2::new(knob_x, c.y);
                painter.circle_filled(knob_center, 5.8, Color32::WHITE);
                if is_on {
                    let tri = vec![
                        Pos2::new(knob_center.x - 1.5, knob_center.y - 2.6),
                        Pos2::new(knob_center.x - 1.5, knob_center.y + 2.6),
                        Pos2::new(knob_center.x + 2.5, knob_center.y),
                    ];
                    painter.add(Shape::convex_polygon(
                        tri,
                        Color32::from_rgb(24, 24, 28),
                        Stroke::NONE,
                    ));
                } else {
                    painter.line_segment(
                        [
                            Pos2::new(knob_center.x - 1.5, knob_center.y - 2.4),
                            Pos2::new(knob_center.x - 1.5, knob_center.y + 2.4),
                        ],
                        Stroke::new(1.3, Color32::from_rgb(24, 24, 28)),
                    );
                    painter.line_segment(
                        [
                            Pos2::new(knob_center.x + 1.5, knob_center.y - 2.4),
                            Pos2::new(knob_center.x + 1.5, knob_center.y + 2.4),
                        ],
                        Stroke::new(1.3, Color32::from_rgb(24, 24, 28)),
                    );
                }
            }
            VectorGlyph::AudioOnly(is_on) => {
                let color = if is_on { style.accent } else { fg };
                let r = 6.5_f32;
                let segments = 10;
                let arc: Vec<Pos2> = (0..=segments)
                    .map(|i| {
                        let a = std::f32::consts::PI
                            + (i as f32 / segments as f32) * std::f32::consts::PI;
                        Pos2::new(c.x + a.cos() * r, c.y + 0.5 + a.sin() * r)
                    })
                    .collect();
                painter.add(Shape::line(arc, Stroke::new(1.8, color)));
                let left_cup = Rect::from_min_max(
                    Pos2::new(c.x - 8.0, c.y - 0.5),
                    Pos2::new(c.x - 4.5, c.y + 6.0),
                );
                let right_cup = Rect::from_min_max(
                    Pos2::new(c.x + 4.5, c.y - 0.5),
                    Pos2::new(c.x + 8.0, c.y + 6.0),
                );
                painter.rect_filled(left_cup, 1.5, color);
                painter.rect_filled(right_cup, 1.5, color);
            }
            VectorGlyph::Screenshot => {
                let body = Rect::from_center_size(Pos2::new(c.x, c.y + 1.0), Vec2::new(17.0, 11.5));
                painter.rect_stroke(
                    body,
                    CornerRadius::same(2),
                    Stroke::new(1.5, fg),
                    egui::StrokeKind::Inside,
                );
                let top_bump = Rect::from_min_max(
                    Pos2::new(c.x - 3.5, body.top() - 2.5),
                    Pos2::new(c.x + 3.5, body.top() + 0.5),
                );
                painter.rect_filled(top_bump, 1.0, fg);
                painter.circle_stroke(body.center(), 3.0, Stroke::new(1.5, fg));
            }
            VectorGlyph::Captions(is_on) => {
                let box_rect = Rect::from_center_size(c, Vec2::new(18.0, 13.0));
                painter.rect_stroke(
                    box_rect,
                    CornerRadius::same(2),
                    Stroke::new(1.5, fg),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    Pos2::new(c.x, c.y - 0.5),
                    egui::Align2::CENTER_CENTER,
                    "CC",
                    FontId::proportional(8.5),
                    fg,
                );
                if is_on {
                    painter.line_segment(
                        [
                            Pos2::new(box_rect.left() + 2.0, box_rect.bottom() + 2.5),
                            Pos2::new(box_rect.right() - 2.0, box_rect.bottom() + 2.5),
                        ],
                        Stroke::new(2.0, style.accent),
                    );
                }
            }
            VectorGlyph::Settings(open) => {
                let outer_r = 6.8_f32;
                let inner_r = 2.7_f32;
                let rotation = if open { 0.35_f32 } else { 0.0_f32 };
                for i in 0..8 {
                    let angle = rotation + (i as f32) * std::f32::consts::FRAC_PI_4;
                    let p1 = Pos2::new(c.x + angle.cos() * 4.5, c.y + angle.sin() * 4.5);
                    let p2 = Pos2::new(c.x + angle.cos() * outer_r, c.y + angle.sin() * outer_r);
                    painter.line_segment([p1, p2], Stroke::new(2.2, fg));
                }
                painter.circle_stroke(c, 4.4, Stroke::new(2.0, fg));
                painter.circle_filled(c, inner_r * 0.45, Color32::TRANSPARENT);
            }
            VectorGlyph::MiniPlayer(is_on) => {
                let outer = Rect::from_center_size(c, Vec2::new(18.0, 13.0));
                painter.rect_stroke(
                    outer,
                    CornerRadius::same(2),
                    Stroke::new(1.5, fg),
                    egui::StrokeKind::Inside,
                );
                let pip = Rect::from_min_max(
                    Pos2::new(outer.right() - 8.5, outer.bottom() - 6.0),
                    Pos2::new(outer.right() - 2.0, outer.bottom() - 2.0),
                );
                if is_on {
                    painter.rect_filled(pip, 1.0, style.accent);
                } else {
                    painter.rect_filled(pip, 1.0, fg);
                }
            }
            VectorGlyph::Theater(is_on) => {
                let w = if is_on { 15.0 } else { 18.0 };
                let h = if is_on { 11.0 } else { 11.5 };
                let box_rect = Rect::from_center_size(c, Vec2::new(w, h));
                painter.rect_stroke(
                    box_rect,
                    CornerRadius::same(2),
                    Stroke::new(1.6, if is_on { style.accent } else { fg }),
                    egui::StrokeKind::Inside,
                );
            }
            VectorGlyph::Fullscreen(is_fs) => {
                let d = 6.5_f32;
                let arm = 4.2_f32;
                let s = Stroke::new(1.8, fg);
                if is_fs {
                    let corners = [
                        (Pos2::new(c.x - d + arm, c.y - d + arm), -1.0_f32, -1.0_f32),
                        (Pos2::new(c.x + d - arm, c.y - d + arm), 1.0, -1.0),
                        (Pos2::new(c.x - d + arm, c.y + d - arm), -1.0, 1.0),
                        (Pos2::new(c.x + d - arm, c.y + d - arm), 1.0, 1.0),
                    ];
                    for (p, sx, sy) in corners {
                        painter.line_segment([p, Pos2::new(p.x + sx * arm, p.y)], s);
                        painter.line_segment([p, Pos2::new(p.x, p.y + sy * arm)], s);
                    }
                } else {
                    let corners = [
                        (Pos2::new(c.x - d, c.y - d), 1.0_f32, 1.0_f32),
                        (Pos2::new(c.x + d, c.y - d), -1.0, 1.0),
                        (Pos2::new(c.x - d, c.y + d), 1.0, -1.0),
                        (Pos2::new(c.x + d, c.y + d), -1.0, -1.0),
                    ];
                    for (p, sx, sy) in corners {
                        painter.line_segment([p, Pos2::new(p.x + sx * arm, p.y)], s);
                        painter.line_segment([p, Pos2::new(p.x, p.y + sy * arm)], s);
                    }
                }
            }
            VectorGlyph::Close => {
                let d = 5.0_f32;
                let s = Stroke::new(1.8, fg);
                painter.line_segment(
                    [Pos2::new(c.x - d, c.y - d), Pos2::new(c.x + d, c.y + d)],
                    s,
                );
                painter.line_segment(
                    [Pos2::new(c.x + d, c.y - d), Pos2::new(c.x - d, c.y + d)],
                    s,
                );
            }
        }
    }

    response.on_hover_text(tooltip)
}

fn volume_slider(
    ui: &mut egui::Ui,
    width: f32,
    volume_percent: f32,
    muted: bool,
    style: &OverlayStyle,
) -> (Option<f32>, bool) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click_and_drag());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);

    let effective = if muted {
        0.0
    } else {
        volume_percent.clamp(0.0, 100.0)
    };
    let ratio = (effective / 100.0).clamp(0.0, 1.0);

    let mut new_volume = None;
    let interacting = response.dragged() || response.clicked();
    if interacting
        && rect.width() > 8.0
        && let Some(pos) = response.interact_pointer_pos()
    {
        let track_left = rect.left() + 4.0;
        let track_right = rect.right() - 4.0;
        let frac = ((pos.x - track_left) / (track_right - track_left)).clamp(0.0, 1.0);
        new_volume = Some((frac * 100.0).round());
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let track_left = rect.left() + 4.0;
        let track_right = rect.right() - 4.0;
        let cy = rect.center().y;
        let track_rect = Rect::from_min_max(
            Pos2::new(track_left, cy - 1.5),
            Pos2::new(track_right, cy + 1.5),
        );
        painter.rect_filled(
            track_rect,
            1.5,
            Color32::from_rgba_unmultiplied(255, 255, 255, 75),
        );

        let shown_ratio = new_volume.map(|v| v / 100.0).unwrap_or(ratio);
        let fill_x = track_left + (track_right - track_left) * shown_ratio;
        if fill_x > track_left {
            let fill_rect =
                Rect::from_min_max(Pos2::new(track_left, cy - 1.5), Pos2::new(fill_x, cy + 1.5));
            painter.rect_filled(fill_rect, 1.5, style.icon);
        }
        let thumb_radius = if response.hovered() || response.dragged() {
            5.5
        } else {
            4.5
        };
        painter.circle_filled(Pos2::new(fill_x, cy), thumb_radius, Color32::WHITE);
    }

    let resp = response.on_hover_text(format!("Volume: {:.0}%", effective));
    (new_volume, resp.hovered() || resp.dragged())
}

fn current_chapter_title(chapters: &[OverlayChapter], position_secs: f64) -> Option<&str> {
    let mut best: Option<&OverlayChapter> = None;
    for chapter in chapters {
        if chapter.time_secs <= position_secs + 0.25 {
            match best {
                Some(prev) if prev.time_secs > chapter.time_secs => {}
                _ => best = Some(chapter),
            }
        }
    }
    best.map(|c| c.title.as_str())
}

fn speed_label(speed: f64) -> String {
    if (speed - 1.0).abs() < 0.01 {
        "Normal".to_owned()
    } else if (speed * 4.0 - (speed * 4.0).round()).abs() < 0.01 {
        let s = format!("{speed:.2}");
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        format!("{trimmed}x")
    } else {
        format!("{speed:.2}x")
    }
}

fn quality_summary(telemetry: &OverlayTelemetry) -> String {
    match telemetry.selected_quality {
        Some(h) => format!("{h}p"),
        None => match telemetry.current_height {
            Some(h) => format!("Auto ({h}p)"),
            None => "Automática".to_owned(),
        },
    }
}

fn subtitle_summary(telemetry: &OverlayTelemetry) -> String {
    if !telemetry.has_active_subtitle {
        return "Desativadas".to_owned();
    }
    telemetry
        .subtitles
        .iter()
        .find(|s| s.active)
        .map(|s| s.label.clone())
        .unwrap_or_else(|| "Ativadas".to_owned())
}

fn menu_row(
    ui: &mut egui::Ui,
    label: &str,
    value: Option<&str>,
    selected: bool,
    has_chevron: bool,
    style: &OverlayStyle,
) -> egui::Response {
    let width = ui.available_width().max(120.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(
                rect,
                CornerRadius::same(6),
                Color32::from_rgba_unmultiplied(255, 255, 255, 26),
            );
        }
        let left_pad = if selected { 26.0 } else { 10.0 };
        if selected {
            painter.text(
                Pos2::new(rect.left() + 10.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                "✓",
                FontId::proportional(13.0),
                style.accent,
            );
        }
        painter.text(
            Pos2::new(rect.left() + left_pad, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.5),
            style.text,
        );
        let right_x = rect.right() - if has_chevron { 20.0 } else { 10.0 };
        if let Some(val) = value {
            painter.text(
                Pos2::new(right_x, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                val,
                FontId::proportional(11.5),
                style.text.gamma_multiply(0.72),
            );
        }
        if has_chevron {
            painter.text(
                Pos2::new(rect.right() - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                "›",
                FontId::proportional(14.0),
                style.text.gamma_multiply(0.75),
            );
        }
    }
    response
}

fn compact_pill_button(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    enabled: bool,
    style: &OverlayStyle,
    tooltip: &str,
) -> egui::Response {
    let text_color = if !enabled {
        style.text.gamma_multiply(0.35)
    } else if active {
        Color32::WHITE
    } else {
        style.text
    };
    let fill = if active && enabled {
        Color32::from_rgba_unmultiplied(62, 166, 255, 190)
    } else if enabled {
        Color32::from_rgba_unmultiplied(255, 255, 255, 24)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 10)
    };
    let resp = ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(11.0).color(text_color))
            .fill(fill)
            .corner_radius(5)
            .min_size(Vec2::new(0.0, 24.0)),
    );
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(tooltip)
    } else {
        resp.on_hover_text(tooltip)
    }
}

fn ab_loop_summary(telemetry: &OverlayTelemetry) -> String {
    match (telemetry.loop_a_secs, telemetry.loop_b_secs) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() && a < b => {
            format!("{} – {}", format_time(a), format_time(b))
        }
        (Some(a), _) if a.is_finite() => format!("A: {}", format_time(a)),
        (_, Some(b)) if b.is_finite() => format!("B: {}", format_time(b)),
        _ => "Desativado".to_owned(),
    }
}

fn show_settings_popup(
    ui: &mut egui::Ui,
    video_rect: Rect,
    bar_rect: Rect,
    menu: OverlayMenu,
    telemetry: &OverlayTelemetry,
    style: &OverlayStyle,
) -> (Option<OverlayAction>, bool) {
    let popup_w = (video_rect.width() - 16.0).clamp(180.0, 276.0);
    let max_h = (bar_rect.min.y - video_rect.min.y - 12.0).clamp(110.0, 360.0);
    let est_h = match menu {
        OverlayMenu::Root => {
            let base_rows = if telemetry.is_live { 6 } else { 10 };
            let rows = base_rows
                + usize::from(!telemetry.chapters.is_empty())
                + usize::from(telemetry.can_bookmark);
            (rows as f32 * 34.0 + 18.0).min(max_h)
        }
        OverlayMenu::Speed => ((SPEED_PRESETS.len() as f32 + 1.0) * 32.0 + 44.0).min(max_h),
        OverlayMenu::Quality => {
            let rows = telemetry.qualities.len().max(1) + 2;
            (rows as f32 * 32.0 + 44.0).min(max_h)
        }
        OverlayMenu::Subtitles => {
            let rows = telemetry.subtitles.len().max(1) + 2;
            (rows as f32 * 32.0 + 44.0).min(max_h)
        }
        OverlayMenu::Chapters => {
            let rows = telemetry.chapters.len().max(1) + 1;
            (rows as f32 * 32.0 + 44.0).min(max_h)
        }
    };

    let right = (video_rect.max.x - 10.0).max(video_rect.min.x + popup_w);
    let left = (right - popup_w).max(video_rect.min.x + 8.0);
    let bottom = (bar_rect.min.y - 6.0).max(video_rect.min.y + est_h);
    let top = (bottom - est_h).max(video_rect.min.y + 8.0);
    let popup_rect = Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom));

    let pointer_inside = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|pos| popup_rect.contains(pos));

    let mut action: Option<OverlayAction> = None;

    ui.scope_builder(egui::UiBuilder::new().max_rect(popup_rect), |ui| {
        ui.set_clip_rect(popup_rect.intersect(video_rect));
        // Absorb clicks on the popup backdrop so they do not toggle playback behind it.
        let bg_id = ui.make_persistent_id("fasttube_overlay_popup_bg");
        let _ = ui.interact(popup_rect, bg_id, Sense::click());

        let painter = ui.painter();
        painter.rect_filled(popup_rect, CornerRadius::same(12), style.panel);
        painter.rect_stroke(
            popup_rect,
            CornerRadius::same(12),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 34)),
            egui::StrokeKind::Inside,
        );

        let inner = popup_rect.shrink2(Vec2::new(8.0, 8.0));
        if inner.width() <= 10.0 || inner.height() <= 10.0 {
            return;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
            match menu {
                OverlayMenu::Root => {
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_root_scroll")
                        .max_height(inner.height().max(40.0))
                        .show(ui, |ui| {
                            if !telemetry.is_live {
                                if menu_row(
                                    ui,
                                    "Reprodu\u{00e7}\u{00e3}o autom\u{00e1}tica",
                                    Some(if telemetry.autoplay {
                                        "Ativada"
                                    } else {
                                        "Desativada"
                                    }),
                                    telemetry.autoplay,
                                    false,
                                    style,
                                )
                                .clicked()
                                {
                                    action = Some(OverlayAction::ToggleAutoplay);
                                }
                                if menu_row(
                                    ui,
                                    "Repetir v\u{00ed}deo",
                                    Some(if telemetry.loop_video {
                                        "Ativado"
                                    } else {
                                        "Desativado"
                                    }),
                                    telemetry.loop_video,
                                    false,
                                    style,
                                )
                                .clicked()
                                {
                                    action = Some(OverlayAction::ToggleLoop);
                                }
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new("Loop A-B").size(12.0).color(style.text),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.add_space(8.0);
                                            ui.label(
                                                RichText::new(ab_loop_summary(telemetry))
                                                    .size(11.0)
                                                    .color(style.text.gamma_multiply(0.75)),
                                            );
                                        },
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.add_space(10.0);
                                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                                    if compact_pill_button(
                                        ui,
                                        "Definir A",
                                        telemetry.loop_a_secs.is_some(),
                                        true,
                                        style,
                                        "Definir ponto A ([)",
                                    )
                                    .clicked()
                                    {
                                        action = Some(OverlayAction::SetLoopA);
                                    }
                                    if compact_pill_button(
                                        ui,
                                        "Definir B",
                                        telemetry.loop_b_secs.is_some(),
                                        true,
                                        style,
                                        "Definir ponto B (])",
                                    )
                                    .clicked()
                                    {
                                        action = Some(OverlayAction::SetLoopB);
                                    }
                                    let has_ab = telemetry.loop_a_secs.is_some()
                                        || telemetry.loop_b_secs.is_some();
                                    if compact_pill_button(
                                        ui,
                                        "Limpar",
                                        false,
                                        has_ab,
                                        style,
                                        "Limpar loop A-B (\\)",
                                    )
                                    .clicked()
                                    {
                                        action = Some(OverlayAction::ClearLoopAB);
                                    }
                                });
                                ui.add_space(2.0);
                            }
                            if menu_row(
                                ui,
                                "Modo somente \u{00e1}udio",
                                Some(if telemetry.audio_only {
                                    "Ativado"
                                } else {
                                    "Desativado"
                                }),
                                telemetry.audio_only,
                                false,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::ToggleAudioOnly);
                            }
                            if !telemetry.audio_only
                                && menu_row(
                                    ui,
                                    "Capturar quadro",
                                    Some("PNG (S)"),
                                    false,
                                    false,
                                    style,
                                )
                                .clicked()
                            {
                                action = Some(OverlayAction::CaptureScreenshot);
                            }
                            if menu_row(
                                ui,
                                "Velocidade",
                                Some(&speed_label(telemetry.speed)),
                                false,
                                true,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Speed)));
                            }
                            if menu_row(
                                ui,
                                "Qualidade",
                                Some(&quality_summary(telemetry)),
                                false,
                                true,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Quality)));
                            }
                            if menu_row(
                                ui,
                                "Legendas",
                                Some(&subtitle_summary(telemetry)),
                                false,
                                true,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Subtitles)));
                            }
                            if !telemetry.chapters.is_empty() {
                                let ch_count = format!("{}", telemetry.chapters.len());
                                if menu_row(
                                    ui,
                                    "Cap\u{00ed}tulos",
                                    Some(&ch_count),
                                    false,
                                    true,
                                    style,
                                )
                                .clicked()
                                {
                                    action =
                                        Some(OverlayAction::SetMenu(Some(OverlayMenu::Chapters)));
                                }
                            }
                            if telemetry.can_bookmark {
                                let ts = format_time(telemetry.position_secs);
                                if menu_row(
                                    ui,
                                    "Adicionar marcador",
                                    Some(&ts),
                                    false,
                                    false,
                                    style,
                                )
                                .clicked()
                                {
                                    action = Some(OverlayAction::AddBookmark);
                                }
                            }
                            ui.add_space(2.0);
                            let sep_y = ui.cursor().top();
                            ui.painter().line_segment(
                                [
                                    Pos2::new(inner.left(), sep_y),
                                    Pos2::new(inner.right(), sep_y),
                                ],
                                Stroke::new(
                                    1.0,
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 24),
                                ),
                            );
                            ui.add_space(4.0);
                            if menu_row(
                                ui,
                                "Configura\u{00e7}\u{00f5}es do app...",
                                None,
                                false,
                                false,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::OpenAppSettings);
                            }
                        });
                }
                OverlayMenu::Speed => {
                    if menu_row(ui, "‹  Velocidade da reprodução", None, false, false, style)
                        .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_speed_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            for choice in SPEED_PRESETS {
                                let is_cur = (telemetry.speed - choice).abs() < 0.02;
                                let label = if (choice - 1.0).abs() < 0.01 {
                                    "Normal (1x)".to_owned()
                                } else {
                                    speed_label(choice)
                                };
                                if menu_row(ui, &label, None, is_cur, false, style).clicked() {
                                    action = Some(OverlayAction::SetSpeed(choice));
                                }
                            }
                        });
                }
                OverlayMenu::Quality => {
                    if menu_row(ui, "‹  Qualidade", None, false, false, style).clicked() {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_quality_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            let auto_label = match telemetry.current_height {
                                Some(h) if telemetry.selected_quality.is_none() => {
                                    format!("Automática ({h}p)")
                                }
                                _ => "Automática".to_owned(),
                            };
                            if menu_row(
                                ui,
                                &auto_label,
                                None,
                                telemetry.selected_quality.is_none(),
                                false,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SelectQuality(None));
                            }
                            if telemetry.qualities.is_empty() && telemetry.qualities_loading {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("Consultando resoluções...")
                                        .size(11.5)
                                        .color(style.text.gamma_multiply(0.7)),
                                );
                            }
                            for &height in &telemetry.qualities {
                                let selected = telemetry.selected_quality == Some(height);
                                if menu_row(ui, &format!("{height}p"), None, selected, false, style)
                                    .clicked()
                                {
                                    action = Some(OverlayAction::SelectQuality(Some(height)));
                                }
                            }
                        });
                }
                OverlayMenu::Subtitles => {
                    if menu_row(ui, "‹  Legendas", None, false, false, style).clicked() {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_subtitles_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            if menu_row(
                                ui,
                                "Desativadas",
                                None,
                                !telemetry.has_active_subtitle,
                                false,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SelectSubtitle(None));
                            }
                            if telemetry.subtitles.is_empty() && telemetry.subtitles_loading {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("Buscando faixas de legenda...")
                                        .size(11.5)
                                        .color(style.text.gamma_multiply(0.7)),
                                );
                            }
                            for track in &telemetry.subtitles {
                                if menu_row(ui, &track.label, None, track.active, false, style)
                                    .clicked()
                                {
                                    action =
                                        Some(OverlayAction::SelectSubtitle(Some(track.id.clone())));
                                }
                            }
                        });
                }
                OverlayMenu::Chapters => {
                    if menu_row(ui, "‹  Capítulos", None, false, false, style).clicked() {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_chapters_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            let active_title =
                                current_chapter_title(&telemetry.chapters, telemetry.position_secs);
                            for chapter in &telemetry.chapters {
                                let ts = format_time(chapter.time_secs);
                                let selected = active_title == Some(chapter.title.as_str());
                                if menu_row(ui, &chapter.title, Some(&ts), selected, false, style)
                                    .clicked()
                                {
                                    action = Some(OverlayAction::SeekChapter(chapter.time_secs));
                                }
                            }
                        });
                }
            }
        });
    });

    (action, pointer_inside)
}

pub fn show(
    ui: &mut egui::Ui,
    video_rect: Rect,
    telemetry: &OverlayTelemetry,
    style: &OverlayStyle,
    visible: bool,
) -> (Option<OverlayAction>, bool) {
    let prev_clip = ui.clip_rect();
    let clipped_video = video_rect.intersect(prev_clip);
    if clipped_video.width() <= 0.0 || clipped_video.height() <= 0.0 {
        return (None, false);
    }

    let bar_rect = compute_bar_rect(video_rect);

    if let Some(sub_text) = telemetry
        .active_subtitle_text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let font_px = (video_rect.height() * 0.042).clamp(14.0, 26.0);
        let max_sub_width = (video_rect.width() * 0.84).max(120.0);
        let painter = ui.painter().with_clip_rect(clipped_video);
        let galley = painter.layout(
            sub_text.to_owned(),
            FontId::proportional(font_px),
            Color32::WHITE,
            max_sub_width,
        );
        let text_size = galley.size();
        let pad_x = 12.0;
        let pad_y = 6.0;
        let bottom_y = if visible {
            bar_rect
                .map(|b| b.min.y - 10.0)
                .unwrap_or(video_rect.max.y - 20.0)
        } else {
            video_rect.max.y - 22.0
        };
        let box_w = (text_size.x + pad_x * 2.0).min(video_rect.width() - 16.0);
        let box_h = text_size.y + pad_y * 2.0;
        let box_rect = Rect::from_min_size(
            Pos2::new(video_rect.center().x - box_w * 0.5, bottom_y - box_h),
            Vec2::new(box_w, box_h),
        );
        painter.rect_filled(
            box_rect,
            CornerRadius::same(6),
            Color32::from_rgba_unmultiplied(8, 8, 10, 210),
        );
        let text_pos = Pos2::new(
            box_rect.center().x - text_size.x * 0.5,
            box_rect.min.y + pad_y,
        );
        painter.galley(text_pos, galley, Color32::WHITE);
    }

    let mut sponsor_skip_action: Option<OverlayAction> = None;
    let mut over_sponsor_pill = false;
    if !telemetry.is_live && telemetry.menu.is_none() && video_rect.height() >= 110.0 {
        let base_bottom_y = if visible {
            bar_rect
                .map(|b| b.min.y - 12.0)
                .unwrap_or(video_rect.max.y - 20.0)
        } else {
            video_rect.max.y - 18.0
        };
        if let Some(seg) = &telemetry.active_sponsor_segment {
            let label = format!("Pular {} ⏭", seg.category_label);
            let font = FontId::proportional(12.5);
            let galley = ui.painter().layout_no_wrap(label, font, Color32::WHITE);
            let pad = Vec2::new(12.0, 6.0);
            let pill_size = Vec2::new(
                (galley.size().x + pad.x * 2.0 + 12.0).min((video_rect.width() - 24.0).max(80.0)),
                galley.size().y + pad.y * 2.0,
            );
            let pill_rect = Rect::from_min_size(
                Pos2::new(
                    (video_rect.max.x - 16.0 - pill_size.x).max(video_rect.min.x + 12.0),
                    (base_bottom_y - pill_size.y).max(video_rect.min.y + 12.0),
                ),
                pill_size,
            );
            if clipped_video.intersects(pill_rect) {
                let resp = ui
                    .interact(
                        pill_rect,
                        ui.make_persistent_id("sponsorblock_manual_skip"),
                        Sense::click(),
                    )
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Pular segmento marcado pelo SponsorBlock");
                let bg = if resp.hovered() {
                    Color32::from_rgba_unmultiplied(36, 36, 42, 245)
                } else {
                    Color32::from_rgba_unmultiplied(18, 18, 22, 232)
                };
                let painter = ui.painter().with_clip_rect(clipped_video);
                painter.rect_filled(pill_rect, CornerRadius::same(8), bg);
                painter.rect_stroke(
                    pill_rect,
                    CornerRadius::same(8),
                    Stroke::new(1.2, seg.color.gamma_multiply(0.9)),
                    egui::StrokeKind::Inside,
                );
                let dot_center = Pos2::new(pill_rect.left() + 10.0, pill_rect.center().y);
                painter.circle_filled(dot_center, 3.5, seg.color);
                painter.galley(
                    Pos2::new(
                        dot_center.x + 8.0,
                        pill_rect.center().y - galley.size().y * 0.5,
                    ),
                    galley,
                    Color32::WHITE,
                );
                if resp.hovered() {
                    over_sponsor_pill = true;
                }
                if resp.clicked() {
                    sponsor_skip_action = Some(OverlayAction::SkipSponsorSegment(seg.end_secs));
                    over_sponsor_pill = true;
                }
            }
        } else if let Some(toast_text) = telemetry
            .sponsor_skip_toast
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            let font = FontId::proportional(11.5);
            let galley = ui
                .painter()
                .layout_no_wrap(toast_text.to_owned(), font, Color32::WHITE);
            let pad = Vec2::new(10.0, 5.0);
            let pill_size = Vec2::new(
                (galley.size().x + pad.x * 2.0).min((video_rect.width() - 24.0).max(80.0)),
                galley.size().y + pad.y * 2.0,
            );
            let pill_rect = Rect::from_min_size(
                Pos2::new(
                    (video_rect.max.x - 16.0 - pill_size.x).max(video_rect.min.x + 12.0),
                    (base_bottom_y - pill_size.y).max(video_rect.min.y + 12.0),
                ),
                pill_size,
            );
            if clipped_video.intersects(pill_rect) {
                let painter = ui.painter().with_clip_rect(clipped_video);
                painter.rect_filled(
                    pill_rect,
                    CornerRadius::same(7),
                    Color32::from_rgba_unmultiplied(18, 18, 22, 218),
                );
                painter.rect_stroke(
                    pill_rect,
                    CornerRadius::same(7),
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 212, 110, 140)),
                    egui::StrokeKind::Inside,
                );
                painter.galley(
                    Pos2::new(pill_rect.left() + pad.x, pill_rect.top() + pad.y),
                    galley,
                    Color32::WHITE,
                );
            }
        }
    }

    if !visible {
        return (sponsor_skip_action, over_sponsor_pill);
    }

    let Some(bar_rect) = bar_rect else {
        return (sponsor_skip_action, over_sponsor_pill);
    };

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let mut over_controls = pointer_pos.is_some_and(|pos| bar_rect.contains(pos));

    let mut action_top: Option<OverlayAction> = None;
    let mut action_center: Option<OverlayAction> = None;
    let mut action_popup: Option<OverlayAction> = None;
    let mut action_play: Option<OverlayAction> = None;
    let mut action_next: Option<OverlayAction> = None;
    let mut action_seek_btn: Option<OverlayAction> = None;
    let mut action_progress: Option<OverlayAction> = None;
    let mut action_mute: Option<OverlayAction> = None;
    let mut action_volume: Option<OverlayAction> = None;
    let mut action_live_pill: Option<OverlayAction> = None;
    let mut action_chapter_pill: Option<OverlayAction> = None;
    let mut action_autoplay: Option<OverlayAction> = None;
    let mut action_audio_only: Option<OverlayAction> = None;
    let mut action_screenshot: Option<OverlayAction> = None;
    let mut action_captions: Option<OverlayAction> = None;
    let mut action_settings: Option<OverlayAction> = None;
    let mut action_mini: Option<OverlayAction> = None;
    let mut action_theater: Option<OverlayAction> = None;
    let mut action_fullscreen: Option<OverlayAction> = None;

    if video_rect.height() >= 180.0 {
        let top_h = 46.0_f32.min(video_rect.height() * 0.25);
        let top_rect = Rect::from_min_max(
            video_rect.min,
            Pos2::new(video_rect.max.x, video_rect.min.y + top_h),
        );
        paint_vertical_gradient(
            ui.painter(),
            top_rect,
            Color32::from_rgba_unmultiplied(0, 0, 0, 155),
            Color32::TRANSPARENT,
        );

        let close_rect = Rect::from_min_size(
            Pos2::new(video_rect.max.x - 42.0, video_rect.min.y + 8.0),
            Vec2::new(32.0, 32.0),
        );
        if pointer_pos.is_some_and(|pos| close_rect.contains(pos)) {
            over_controls = true;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(top_rect), |ui| {
            ui.set_clip_rect(top_rect.intersect(clipped_video));
            if telemetry.fullscreen && !telemetry.title.is_empty() && top_rect.width() >= 220.0 {
                let title_area = Rect::from_min_max(
                    Pos2::new(top_rect.left() + 14.0, top_rect.top() + 8.0),
                    Pos2::new(
                        (close_rect.left() - 12.0).max(top_rect.left() + 20.0),
                        top_rect.top() + 38.0,
                    ),
                );
                ui.painter().with_clip_rect(title_area).text(
                    Pos2::new(title_area.left(), title_area.center().y),
                    egui::Align2::LEFT_CENTER,
                    &telemetry.title,
                    FontId::proportional(14.5),
                    Color32::WHITE,
                );
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(close_rect), |ui| {
                if icon_button(
                    ui,
                    Vec2::new(32.0, 32.0),
                    VectorGlyph::Close,
                    style,
                    true,
                    false,
                    "Fechar vídeo",
                )
                .clicked()
                {
                    action_top = Some(OverlayAction::Close);
                }
            });
        });
    }

    if telemetry.ended
        && !telemetry.is_live
        && telemetry.menu.is_none()
        && video_rect.height() >= 140.0
    {
        let center_rect = Rect::from_center_size(video_rect.center(), Vec2::new(56.0, 56.0));
        if pointer_pos.is_some_and(|pos| center_rect.contains(pos)) {
            over_controls = true;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(center_rect), |ui| {
            ui.set_clip_rect(center_rect.intersect(clipped_video));
            let resp = ui
                .allocate_exact_size(Vec2::new(56.0, 56.0), Sense::click())
                .1
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Reproduzir novamente");
            let bg = if resp.hovered() {
                Color32::from_rgba_unmultiplied(0, 0, 0, 200)
            } else {
                Color32::from_rgba_unmultiplied(0, 0, 0, 145)
            };
            ui.painter().circle_filled(center_rect.center(), 26.0, bg);
            ui.painter().circle_stroke(
                center_rect.center(),
                26.0,
                Stroke::new(1.2, Color32::from_rgba_unmultiplied(255, 255, 255, 50)),
            );
            let pts = vec![
                Pos2::new(center_rect.center().x - 6.0, center_rect.center().y - 9.0),
                Pos2::new(center_rect.center().x - 6.0, center_rect.center().y + 9.0),
                Pos2::new(center_rect.center().x + 10.0, center_rect.center().y),
            ];
            ui.painter()
                .add(Shape::convex_polygon(pts, Color32::WHITE, Stroke::NONE));
            if resp.clicked() {
                action_center = Some(OverlayAction::TogglePlay);
            }
        });
    }

    if let Some(menu) = telemetry.menu {
        let (popup_act, over_popup) =
            show_settings_popup(ui, video_rect, bar_rect, menu, telemetry, style);
        action_popup = popup_act;
        if over_popup {
            over_controls = true;
        }
    }

    let scrim_h = (bar_rect.height() + 26.0).min(video_rect.height());
    let scrim_rect = Rect::from_min_max(
        Pos2::new(
            video_rect.min.x,
            (video_rect.max.y - scrim_h).max(video_rect.min.y),
        ),
        video_rect.max,
    );
    paint_vertical_gradient(
        ui.painter(),
        scrim_rect,
        Color32::TRANSPARENT,
        Color32::from_rgba_unmultiplied(0, 0, 0, 218),
    );

    let clipped_bar = bar_rect.intersect(clipped_video);
    if clipped_bar.width() > 0.0 && clipped_bar.height() > 0.0 {
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
            ui.set_clip_rect(clipped_video);

            let side_pad = BAR_SIDE_PADDING.min((bar_rect.width() * 0.04).max(4.0));
            let track_left = bar_rect.min.x + side_pad;
            let track_right = (bar_rect.max.x - side_pad).max(track_left + 1.0);
            let strip_rect = Rect::from_min_max(
                Pos2::new(track_left, bar_rect.min.y),
                Pos2::new(
                    track_right,
                    (bar_rect.min.y + PROGRESS_HIT_HEIGHT).min(bar_rect.max.y),
                ),
            );

            let live_seekable = telemetry.is_live_dvr
                && telemetry
                    .duration_secs
                    .is_some_and(|d| d.is_finite() && d > 0.0);
            let can_scrub = if telemetry.is_live {
                live_seekable
            } else {
                true
            };

            let progress_id = ui.make_persistent_id("fasttube_overlay_progress_strip");
            let progress_resp = ui.interact(
                strip_rect,
                progress_id,
                if can_scrub {
                    Sense::click_and_drag()
                } else {
                    Sense::hover()
                },
            );
            let progress_resp = if can_scrub {
                progress_resp.on_hover_cursor(CursorIcon::PointingHand)
            } else {
                progress_resp
            };

            let mut drag_fraction: Option<f64> = if can_scrub {
                ui.data(|d| d.get_temp(progress_id))
            } else {
                ui.data_mut(|d| d.remove::<f64>(progress_id));
                None
            };
            if can_scrub
                && progress_resp.dragged()
                && strip_rect.width() > 0.0
                && let Some(pos) = progress_resp
                    .interact_pointer_pos()
                    .or_else(|| ui.input(|i| i.pointer.interact_pos()))
            {
                let frac =
                    f64::from((pos.x - strip_rect.min.x) / strip_rect.width()).clamp(0.0, 1.0);
                drag_fraction = Some(frac);
                ui.data_mut(|d| d.insert_temp(progress_id, frac));
            }
            if can_scrub && progress_resp.drag_stopped() {
                if let Some(frac) = drag_fraction.take() {
                    action_progress = if telemetry.is_live {
                        action_for_live_progress_click(
                            frac,
                            telemetry.duration_secs,
                            telemetry.is_live_dvr,
                        )
                    } else {
                        action_for_progress_click(frac, telemetry.duration_secs)
                    };
                }
                ui.data_mut(|d| d.remove::<f64>(progress_id));
            } else if can_scrub
                && progress_resp.clicked()
                && strip_rect.width() > 0.0
                && let Some(pos) = progress_resp
                    .interact_pointer_pos()
                    .or_else(|| ui.input(|i| i.pointer.interact_pos()))
            {
                let fraction = f64::from((pos.x - strip_rect.min.x) / strip_rect.width());
                action_progress = if telemetry.is_live {
                    action_for_live_progress_click(
                        fraction,
                        telemetry.duration_secs,
                        telemetry.is_live_dvr,
                    )
                } else {
                    action_for_progress_click(fraction, telemetry.duration_secs)
                };
            }

            let is_progress_hot = can_scrub && (progress_resp.hovered() || progress_resp.dragged());
            if progress_resp.hovered() || progress_resp.dragged() {
                over_controls = true;
            }
            let track_h = if is_progress_hot { 5.0 } else { 3.0 };
            let track_cy = strip_rect.center().y + 1.0;
            let line_rect = Rect::from_min_max(
                Pos2::new(strip_rect.min.x, track_cy - track_h * 0.5),
                Pos2::new(strip_rect.max.x, track_cy + track_h * 0.5),
            );
            ui.painter().rect_filled(
                line_rect,
                track_h * 0.5,
                Color32::from_rgba_unmultiplied(255, 255, 255, 72),
            );

            if !telemetry.is_live || live_seekable {
                let buf_ratio = buffered_progress_ratio(
                    telemetry.position_secs,
                    telemetry.buffered_secs,
                    telemetry.duration_secs,
                );
                let buf_w = line_rect.width() * buf_ratio;
                if buf_w > 0.0 {
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            line_rect.min,
                            Pos2::new(line_rect.min.x + buf_w, line_rect.max.y),
                        ),
                        track_h * 0.5,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 118),
                    );
                }
            }

            let hover_fraction = if is_progress_hot && strip_rect.width() > 0.0 {
                pointer_pos.map(|p| ((p.x - strip_rect.min.x) / strip_rect.width()).clamp(0.0, 1.0))
            } else {
                None
            };

            if let Some(hf) = hover_fraction {
                let hover_w = line_rect.width() * hf;
                if hover_w > 0.0 {
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            line_rect.min,
                            Pos2::new(line_rect.min.x + hover_w, line_rect.max.y),
                        ),
                        track_h * 0.5,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 138),
                    );
                }
            }

            if telemetry.is_live && !live_seekable {
                ui.painter().rect_filled(
                    line_rect,
                    track_h * 0.5,
                    if telemetry.at_live_edge || !telemetry.is_live_dvr {
                        style.accent
                    } else {
                        LIVE_BEHIND_DOT_COLOR
                    },
                );
            } else if let Some(duration) = telemetry.duration_secs
                && duration.is_finite()
                && duration > 0.0
            {
                let ratio = drag_fraction.unwrap_or_else(|| {
                    if telemetry.is_live && telemetry.at_live_edge {
                        1.0
                    } else {
                        (telemetry.position_secs / duration).clamp(0.0, 1.0)
                    }
                }) as f32;
                let fill_w = line_rect.width() * ratio.clamp(0.0, 1.0);
                if fill_w > 0.0 {
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            line_rect.min,
                            Pos2::new(line_rect.min.x + fill_w, line_rect.max.y),
                        ),
                        track_h * 0.5,
                        style.accent,
                    );
                }
                if !telemetry.is_live {
                    for seg in &telemetry.sponsor_segments {
                        let start_frac = (seg.start_secs / duration).clamp(0.0, 1.0) as f32;
                        let end_frac = (seg.end_secs / duration).clamp(0.0, 1.0) as f32;
                        if end_frac > start_frac {
                            let sx = line_rect.min.x + line_rect.width() * start_frac;
                            let ex = (line_rect.min.x + line_rect.width() * end_frac).max(sx + 2.0);
                            let seg_rect = Rect::from_min_max(
                                Pos2::new(sx, line_rect.min.y),
                                Pos2::new(ex.min(line_rect.max.x), line_rect.max.y),
                            );
                            ui.painter().rect_filled(seg_rect, 0.0, seg.color);
                        }
                    }

                    let (ab_a, ab_b, ab_range) = ab_loop_marker_fractions(
                        telemetry.loop_a_secs,
                        telemetry.loop_b_secs,
                        telemetry.duration_secs,
                    );
                    let ab_color = Color32::from_rgb(66, 200, 255);
                    if let Some((af, bf)) = ab_range {
                        let sx = line_rect.min.x + line_rect.width() * af;
                        let ex = (line_rect.min.x + line_rect.width() * bf).max(sx + 2.0);
                        ui.painter().rect_filled(
                            Rect::from_min_max(
                                Pos2::new(sx, line_rect.min.y),
                                Pos2::new(ex.min(line_rect.max.x), line_rect.max.y),
                            ),
                            0.0,
                            Color32::from_rgba_unmultiplied(66, 200, 255, 200),
                        );
                    }
                    for (marker_frac, tag) in [(ab_a, "A"), (ab_b, "B")] {
                        if let Some(frac) = marker_frac {
                            let mx = (line_rect.min.x + line_rect.width() * frac)
                                .clamp(line_rect.min.x + 1.0, line_rect.max.x - 1.0);
                            ui.painter().rect_filled(
                                Rect::from_center_size(
                                    Pos2::new(mx, track_cy),
                                    Vec2::new(2.5, track_h + 7.0),
                                ),
                                CornerRadius::same(1),
                                ab_color,
                            );
                            let tag_rect = Rect::from_center_size(
                                Pos2::new(mx, track_cy - track_h * 0.5 - 9.0),
                                Vec2::new(13.0, 12.0),
                            );
                            ui.painter().rect_filled(
                                tag_rect,
                                CornerRadius::same(3),
                                Color32::from_rgba_unmultiplied(15, 20, 28, 232),
                            );
                            ui.painter().rect_stroke(
                                tag_rect,
                                CornerRadius::same(3),
                                Stroke::new(1.0, ab_color),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().text(
                                tag_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                tag,
                                FontId::proportional(8.5),
                                ab_color,
                            );
                        }
                    }
                }
                for chapter in &telemetry.chapters {
                    if chapter.time_secs > 1.0 && chapter.time_secs < duration - 1.0 {
                        let cx = line_rect.min.x
                            + line_rect.width() * (chapter.time_secs / duration) as f32;
                        ui.painter().rect_filled(
                            Rect::from_center_size(
                                Pos2::new(cx, track_cy),
                                Vec2::new(2.0, track_h + 1.0),
                            ),
                            0.0,
                            Color32::from_rgba_unmultiplied(0, 0, 0, 200),
                        );
                    }
                }
                if is_progress_hot {
                    let knob_x = line_rect.min.x + fill_w;
                    ui.painter()
                        .circle_filled(Pos2::new(knob_x, track_cy), 6.5, style.accent);
                }

                if let Some(hf) = hover_fraction
                    && let Some(p) = pointer_pos
                {
                    let preview_secs = f64::from(hf) * duration;
                    let chapter_title = current_chapter_title(&telemetry.chapters, preview_secs)
                        .map(str::trim)
                        .filter(|t| !t.is_empty());
                    let mut time_line = if telemetry.is_live {
                        if f64::from(hf) >= LIVE_EDGE_SNAP_FRACTION {
                            "Ao vivo".to_owned()
                        } else {
                            let behind = (duration - preview_secs).max(0.0);
                            format_live_behind(behind)
                        }
                    } else {
                        format_time(preview_secs)
                    };
                    if !telemetry.is_live
                        && let Some(seg) = telemetry.sponsor_segments.iter().find(|seg| {
                            preview_secs >= seg.start_secs && preview_secs <= seg.end_secs
                        })
                    {
                        time_line.push_str(&format!(" • [SponsorBlock: {}]", seg.category_label));
                    }

                    let max_text_w = (video_rect.width() - 32.0).clamp(64.0, 280.0);
                    let chapter_galley = chapter_title.map(|ch| {
                        ui.painter().layout(
                            ch.to_owned(),
                            FontId::proportional(11.0),
                            Color32::from_gray(230),
                            max_text_w,
                        )
                    });
                    let time_galley = ui.painter().layout(
                        time_line,
                        FontId::proportional(11.5),
                        Color32::WHITE,
                        max_text_w,
                    );

                    let pad = Vec2::new(9.0, 5.0);
                    let line_gap = if chapter_galley.is_some() { 2.0 } else { 0.0 };
                    let content_w = chapter_galley
                        .as_ref()
                        .map_or(0.0, |g| g.size().x)
                        .max(time_galley.size().x);
                    let content_h = chapter_galley
                        .as_ref()
                        .map_or(0.0, |g| g.size().y + line_gap)
                        + time_galley.size().y;
                    let pill_size = Vec2::new(content_w + pad.x * 2.0, content_h + pad.y * 2.0);
                    let pill_rect =
                        compute_hover_pill_rect(p.x, strip_rect.min.y, pill_size, video_rect);
                    let pill_painter = ui.painter().with_clip_rect(clipped_video);
                    pill_painter.rect_filled(
                        pill_rect,
                        CornerRadius::same(6),
                        Color32::from_rgba_unmultiplied(15, 15, 18, 236),
                    );
                    pill_painter.rect_stroke(
                        pill_rect,
                        CornerRadius::same(6),
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28)),
                        egui::StrokeKind::Inside,
                    );
                    let mut text_y = pill_rect.top() + pad.y;
                    if let Some(ch_galley) = chapter_galley {
                        let ch_x = (pill_rect.center().x - ch_galley.size().x * 0.5)
                            .max(pill_rect.left() + pad.x);
                        let ch_h = ch_galley.size().y;
                        pill_painter.with_clip_rect(pill_rect).galley(
                            Pos2::new(ch_x, text_y),
                            ch_galley,
                            Color32::from_gray(230),
                        );
                        text_y += ch_h + line_gap;
                    }
                    let time_x = (pill_rect.center().x - time_galley.size().x * 0.5)
                        .max(pill_rect.left() + pad.x);
                    pill_painter.with_clip_rect(pill_rect).galley(
                        Pos2::new(time_x, text_y),
                        time_galley,
                        Color32::WHITE,
                    );
                }
            }

            let row_top = (strip_rect.max.y + 2.0).min(bar_rect.max.y);
            let row_bottom = (row_top + BUTTON_ROW_HEIGHT).min(bar_rect.max.y);
            let row_rect = Rect::from_min_max(
                Pos2::new(track_left, row_top),
                Pos2::new(track_right, row_bottom),
            );
            if row_rect.width() <= 0.0 || row_rect.height() <= 0.0 {
                return;
            }

            let vw = bar_rect.width();
            let show_seek_10 = !telemetry.is_live && vw >= 440.0;
            let show_autoplay = !telemetry.is_live && vw >= 420.0;
            let show_audio_only = vw >= 480.0;
            let show_screenshot = vw >= 520.0 && !telemetry.audio_only;
            let show_mini = vw >= 360.0 || telemetry.mini_player;
            let show_theater = vw >= 320.0 && !telemetry.fullscreen && !telemetry.mini_player;
            let slider_w = if vw >= 400.0 {
                64.0
            } else if vw >= 300.0 {
                42.0
            } else {
                0.0
            };

            let right_slots = 3
                + usize::from(show_autoplay)
                + usize::from(show_audio_only)
                + usize::from(show_screenshot)
                + usize::from(show_mini)
                + usize::from(show_theater);
            let max_right_ratio = if telemetry.mini_player && vw < 320.0 {
                0.68
            } else {
                0.55
            };
            let right_w = (right_slots as f32 * 34.0).min(row_rect.width() * max_right_ratio);
            let left_rect = Rect::from_min_max(
                row_rect.min,
                Pos2::new(
                    (row_rect.max.x - right_w - 6.0).max(row_rect.min.x + 1.0),
                    row_rect.max.y,
                ),
            );
            let right_rect = Rect::from_min_max(
                Pos2::new(
                    (row_rect.max.x - right_w).max(row_rect.min.x),
                    row_rect.min.y,
                ),
                row_rect.max,
            );

            ui.scope_builder(egui::UiBuilder::new().max_rect(left_rect), |ui| {
                ui.set_clip_rect(left_rect.intersect(clipped_video));
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
                ui.horizontal_centered(|ui| {
                    let play_glyph = if telemetry.ended && !telemetry.is_live {
                        VectorGlyph::Replay
                    } else if telemetry.paused {
                        VectorGlyph::Play
                    } else {
                        VectorGlyph::Pause
                    };
                    let play_tip = if telemetry.ended && !telemetry.is_live {
                        "Reproduzir novamente"
                    } else if telemetry.paused {
                        "Reproduzir (K)"
                    } else {
                        "Pausar (K)"
                    };
                    if icon_button(
                        ui,
                        Vec2::new(32.0, 30.0),
                        play_glyph,
                        style,
                        true,
                        false,
                        play_tip,
                    )
                    .clicked()
                    {
                        action_play = Some(OverlayAction::TogglePlay);
                    }

                    if icon_button(
                        ui,
                        Vec2::new(30.0, 30.0),
                        VectorGlyph::Next,
                        style,
                        telemetry.has_queued,
                        false,
                        if telemetry.has_queued {
                            "Próximo vídeo da fila"
                        } else {
                            "Nenhum próximo vídeo na fila"
                        },
                    )
                    .clicked()
                    {
                        action_next = Some(OverlayAction::Next);
                    }

                    if show_seek_10 {
                        let seek_step = telemetry.seek_step_secs.max(1);
                        let back_tip = format!("Voltar {seek_step} segundos (←)");
                        let forward_tip = format!("Avançar {seek_step} segundos (→)");
                        if icon_button(
                            ui,
                            Vec2::new(32.0, 30.0),
                            VectorGlyph::SkipBack(seek_step),
                            style,
                            true,
                            false,
                            &back_tip,
                        )
                        .clicked()
                        {
                            action_seek_btn = Some(OverlayAction::SeekBack10);
                        }
                        if icon_button(
                            ui,
                            Vec2::new(32.0, 30.0),
                            VectorGlyph::SkipForward(seek_step),
                            style,
                            true,
                            false,
                            &forward_tip,
                        )
                        .clicked()
                            && action_seek_btn.is_none()
                        {
                            action_seek_btn = Some(OverlayAction::SeekForward10);
                        }
                    }

                    if icon_button(
                        ui,
                        Vec2::new(30.0, 30.0),
                        VectorGlyph::Volume(telemetry.muted, telemetry.volume_percent),
                        style,
                        true,
                        false,
                        if telemetry.muted {
                            "Ativar som (M)"
                        } else {
                            "Silenciar (M)"
                        },
                    )
                    .clicked()
                    {
                        action_mute = Some(OverlayAction::ToggleMute);
                    }

                    if slider_w > 0.0 {
                        let (vol_changed, vol_hot) = volume_slider(
                            ui,
                            slider_w,
                            telemetry.volume_percent,
                            telemetry.muted,
                            style,
                        );
                        if vol_hot {
                            over_controls = true;
                        }
                        if let Some(v) = vol_changed {
                            action_volume = Some(OverlayAction::SetVolume(v));
                        }
                    }

                    if telemetry.is_live {
                        ui.add_space(4.0);
                        let behind_now = if let Some(frac) = drag_fraction {
                            if frac >= LIVE_EDGE_SNAP_FRACTION {
                                0.0
                            } else if let Some(dur) = telemetry
                                .duration_secs
                                .filter(|d| d.is_finite() && *d > 0.0)
                            {
                                (dur * (1.0 - frac)).max(1.0)
                            } else {
                                telemetry.live_behind_secs
                            }
                        } else {
                            telemetry.live_behind_secs
                        };
                        let is_edge = telemetry.at_live_edge
                            && drag_fraction.is_none_or(|f| f >= LIVE_EDGE_SNAP_FRACTION)
                            && !telemetry.paused
                            && behind_now < 1.0;
                        let dot_color = if is_edge {
                            LIVE_EDGE_COLOR
                        } else {
                            LIVE_BEHIND_DOT_COLOR
                        };
                        let label_text = if !is_edge && behind_now >= 1.0 {
                            format!("Ao vivo ({})", format_live_behind(behind_now))
                        } else {
                            "Ao vivo".to_owned()
                        };
                        let font = FontId::proportional(12.0);
                        let galley = ui.painter().layout_no_wrap(label_text, font, style.text);
                        let dot_r = 3.6_f32;
                        let pad_x = 6.0_f32;
                        let gap = 6.0_f32;
                        let pill_w = pad_x * 2.0 + dot_r * 2.0 + gap + galley.size().x;
                        let pill_h = 24.0_f32;
                        let (pill_rect, pill_resp) =
                            ui.allocate_exact_size(Vec2::new(pill_w, pill_h), Sense::click());
                        let pill_resp = pill_resp
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text(if is_edge {
                                "Transmissão ao vivo"
                            } else {
                                "Ir para o momento ao vivo"
                            });
                        if ui.is_rect_visible(pill_rect) {
                            if pill_resp.hovered() {
                                ui.painter().rect_filled(
                                    pill_rect,
                                    CornerRadius::same(12),
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 24),
                                );
                            }
                            let dot_center =
                                Pos2::new(pill_rect.left() + pad_x + dot_r, pill_rect.center().y);
                            ui.painter().circle_filled(dot_center, dot_r, dot_color);
                            let text_pos = Pos2::new(
                                dot_center.x + dot_r + gap,
                                pill_rect.center().y - galley.size().y * 0.5,
                            );
                            ui.painter().galley(text_pos, galley, style.text);
                        }
                        if pill_resp.hovered() {
                            over_controls = true;
                        }
                        if pill_resp.clicked() {
                            action_live_pill = Some(OverlayAction::GoLive);
                        }
                    } else {
                        let shown_pos = drag_fraction
                            .and_then(|f| telemetry.duration_secs.map(|d| f * d))
                            .unwrap_or(telemetry.position_secs);
                        let time_label = format_elapsed_total(shown_pos, telemetry.duration_secs);
                        ui.add_space(4.0);
                        ui.label(RichText::new(time_label).color(style.text).size(12.0));
                    }

                    if vw >= 620.0
                        && let Some(ch_title) =
                            current_chapter_title(&telemetry.chapters, telemetry.position_secs)
                    {
                        ui.add_space(6.0);
                        let trimmed = if ch_title.chars().count() > 28 {
                            let prefix: String = ch_title.chars().take(27).collect();
                            format!("• {}… ›", prefix.trim_end())
                        } else {
                            format!("• {ch_title} ›")
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(trimmed)
                                        .color(style.text.gamma_multiply(0.9))
                                        .size(11.5),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text("Abrir capítulos")
                            .clicked()
                        {
                            let next_menu = if telemetry.menu == Some(OverlayMenu::Chapters) {
                                None
                            } else {
                                Some(OverlayMenu::Chapters)
                            };
                            action_chapter_pill = Some(OverlayAction::SetMenu(next_menu));
                        }
                    }
                });
            });

            ui.scope_builder(egui::UiBuilder::new().max_rect(right_rect), |ui| {
                ui.set_clip_rect(right_rect.intersect(clipped_video));
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(
                        ui,
                        Vec2::new(32.0, 30.0),
                        VectorGlyph::Fullscreen(telemetry.fullscreen),
                        style,
                        true,
                        false,
                        if telemetry.fullscreen {
                            "Sair da tela cheia (F)"
                        } else {
                            "Tela cheia (F)"
                        },
                    )
                    .clicked()
                    {
                        action_fullscreen = Some(OverlayAction::ToggleFullscreen);
                    }

                    if show_theater
                        && icon_button(
                            ui,
                            Vec2::new(32.0, 30.0),
                            VectorGlyph::Theater(telemetry.theater),
                            style,
                            true,
                            telemetry.theater,
                            if telemetry.theater {
                                "Visualização padrão (T)"
                            } else {
                                "Modo teatro (T)"
                            },
                        )
                        .clicked()
                    {
                        action_theater = Some(OverlayAction::ToggleTheater);
                    }

                    if show_mini
                        && icon_button(
                            ui,
                            Vec2::new(32.0, 30.0),
                            VectorGlyph::MiniPlayer(telemetry.mini_player),
                            style,
                            true,
                            telemetry.mini_player,
                            if telemetry.mini_player {
                                "Sair do miniplayer"
                            } else {
                                "Miniplayer"
                            },
                        )
                        .clicked()
                    {
                        action_mini = Some(OverlayAction::ToggleMiniPlayer);
                    }

                    let settings_resp = icon_button(
                        ui,
                        Vec2::new(32.0, 30.0),
                        VectorGlyph::Settings(telemetry.menu.is_some()),
                        style,
                        true,
                        telemetry.menu.is_some(),
                        "Ajustes (velocidade, qualidade, legendas)",
                    );
                    let hd_badge = telemetry
                        .selected_quality
                        .or(telemetry.current_height)
                        .is_some_and(|h| h >= 720);
                    if hd_badge {
                        let badge_rect = Rect::from_center_size(
                            Pos2::new(
                                settings_resp.rect.right() - 6.0,
                                settings_resp.rect.top() + 7.0,
                            ),
                            Vec2::new(13.0, 9.0),
                        );
                        ui.painter()
                            .rect_filled(badge_rect, CornerRadius::same(2), style.accent);
                        ui.painter().text(
                            badge_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "HD",
                            FontId::proportional(6.5),
                            Color32::WHITE,
                        );
                    }
                    if settings_resp.clicked() {
                        let next_menu = if telemetry.menu.is_some() {
                            None
                        } else {
                            Some(OverlayMenu::Root)
                        };
                        action_settings = Some(OverlayAction::SetMenu(next_menu));
                    }

                    if icon_button(
                        ui,
                        Vec2::new(32.0, 30.0),
                        VectorGlyph::Captions(telemetry.has_active_subtitle),
                        style,
                        true,
                        telemetry.has_active_subtitle,
                        "Legendas (C)",
                    )
                    .clicked()
                    {
                        action_captions = Some(OverlayAction::ToggleCaptions);
                    }

                    if show_autoplay
                        && icon_button(
                            ui,
                            Vec2::new(32.0, 30.0),
                            VectorGlyph::Autoplay(telemetry.autoplay),
                            style,
                            true,
                            telemetry.autoplay,
                            if telemetry.autoplay {
                                "Reprodu\u{00e7}\u{00e3}o autom\u{00e1}tica ativada"
                            } else {
                                "Reprodu\u{00e7}\u{00e3}o autom\u{00e1}tica desativada"
                            },
                        )
                        .clicked()
                    {
                        action_autoplay = Some(OverlayAction::ToggleAutoplay);
                    }

                    if show_audio_only
                        && icon_button(
                            ui,
                            Vec2::new(32.0, 30.0),
                            VectorGlyph::AudioOnly(telemetry.audio_only),
                            style,
                            true,
                            telemetry.audio_only,
                            if telemetry.audio_only {
                                "Voltar ao vídeo (somente áudio ativo)"
                            } else {
                                "Modo somente áudio"
                            },
                        )
                        .clicked()
                    {
                        action_audio_only = Some(OverlayAction::ToggleAudioOnly);
                    }

                    if show_screenshot
                        && icon_button(
                            ui,
                            Vec2::new(32.0, 30.0),
                            VectorGlyph::Screenshot,
                            style,
                            true,
                            false,
                            "Capturar quadro (S)",
                        )
                        .clicked()
                    {
                        action_screenshot = Some(OverlayAction::CaptureScreenshot);
                    }
                });
            });
        });
    }

    let action = sponsor_skip_action
        .or(action_popup)
        .or(action_top)
        .or(action_center)
        .or(action_play)
        .or(action_next)
        .or(action_seek_btn)
        .or(action_progress)
        .or(action_mute)
        .or(action_volume)
        .or(action_live_pill)
        .or(action_chapter_pill)
        .or(action_autoplay)
        .or(action_audio_only)
        .or(action_screenshot)
        .or(action_captions)
        .or(action_settings)
        .or(action_mini)
        .or(action_theater)
        .or(action_fullscreen);

    over_controls |= over_sponsor_pill;

    (action, over_controls)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_style() -> OverlayStyle {
        OverlayStyle {
            panel: Color32::from_rgba_unmultiplied(20, 20, 24, 238),
            icon: Color32::from_rgb(248, 248, 250),
            text: Color32::from_rgb(244, 244, 246),
            accent: Color32::from_rgb(255, 0, 51),
        }
    }

    fn click_at(
        ctx: &egui::Context,
        rect: Rect,
        telemetry: &OverlayTelemetry,
        style: &OverlayStyle,
        pos: Pos2,
    ) -> (Option<OverlayAction>, bool) {
        let mut raw_move = egui::RawInput::default();
        raw_move.events.push(egui::Event::PointerMoved(pos));
        let mut out = ctx.run_ui(raw_move, |ui| {
            let _ = show(ui, rect, telemetry, style, true);
        });
        out.textures_delta.clear();

        let mut raw_click = egui::RawInput::default();
        raw_click.events.push(egui::Event::PointerMoved(pos));
        raw_click.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        raw_click.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });

        let mut click_result = (None, false);
        let mut out = ctx.run_ui(raw_click, |ui| {
            click_result = show(ui, rect, telemetry, style, true);
        });
        out.textures_delta.clear();
        click_result
    }

    #[test]
    fn overlay_visible_truth_table() {
        assert!(overlay_visible(true, false, false, 10.0));
        assert!(overlay_visible(false, true, false, 10.0));
        assert!(overlay_visible(false, false, true, 10.0));
        assert!(overlay_visible(false, false, false, 0.0));
        assert!(overlay_visible(
            false,
            false,
            false,
            OVERLAY_HIDE_DELAY_SECS - 0.01
        ));
        assert!(!overlay_visible(
            false,
            false,
            false,
            OVERLAY_HIDE_DELAY_SECS
        ));
        assert!(!overlay_visible(false, false, false, 15.0));

        assert!(overlay_visible_with_state(
            false, false, false, true, false, 30.0
        ));
        assert!(overlay_visible_with_state(
            false, false, false, false, true, 30.0
        ));
        assert!(!overlay_visible_with_state(
            false, false, false, false, false, 30.0
        ));
    }

    #[test]
    fn format_time_values_and_none_duration() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(59.0), "0:59");
        assert_eq!(format_time(60.0), "1:00");
        assert_eq!(format_time(3599.0), "59:59");
        assert_eq!(format_time(3600.0), "1:00:00");
        assert_eq!(format_time(3661.0), "1:01:01");

        assert_eq!(format_elapsed_total(0.0, None), "0:00 / --:--");
        assert_eq!(format_elapsed_total(65.0, Some(3600.0)), "1:05 / 1:00:00");
    }

    #[test]
    fn headless_smoke_test_and_invisible_short_circuit() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(640.0, 360.0));
        let telemetry = OverlayTelemetry {
            paused: false,
            ended: false,
            muted: false,
            position_secs: 42.0,
            duration_secs: Some(120.0),
            volume_percent: 80.0,
            has_queued: true,
            ..OverlayTelemetry::default()
        };
        let style = sample_style();

        let mut visible_out = None;
        let mut hidden_out = None;

        let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
            visible_out = Some(show(ui, rect, &telemetry, &style, true));
            hidden_out = Some(show(ui, rect, &telemetry, &style, false));
        });
        output.textures_delta.clear();

        assert_eq!(visible_out, Some((None, false)));
        assert_eq!(hidden_out, Some((None, false)));
    }

    #[test]
    fn progress_click_helper_and_synthetic_play_and_settings_clicks() {
        assert_eq!(
            action_for_progress_click(0.25, Some(200.0)),
            Some(OverlayAction::SeekTo(50.0))
        );
        assert_eq!(
            action_for_progress_click(1.5, Some(100.0)),
            Some(OverlayAction::SeekTo(100.0))
        );
        assert_eq!(action_for_progress_click(0.5, None), None);
        assert_eq!(action_for_progress_click(0.5, Some(0.0)), None);

        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(640.0, 360.0));
        let telemetry = OverlayTelemetry::default();
        let style = sample_style();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect válido");

        let play_click_pos = Pos2::new(bar_rect.min.x + 26.0, bar_rect.min.y + 36.0);
        let ctx = egui::Context::default();
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, play_click_pos),
            (Some(OverlayAction::TogglePlay), true)
        );

        let settings_click_pos = Pos2::new(bar_rect.max.x - 120.0, bar_rect.min.y + 36.0);
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, settings_click_pos),
            (Some(OverlayAction::SetMenu(Some(OverlayMenu::Root))), true)
        );
    }

    #[test]
    fn settings_popup_renders_submenus_and_emits_speed_quality_and_subtitle_actions() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let style = sample_style();
        let mut telemetry = OverlayTelemetry {
            duration_secs: Some(300.0),
            position_secs: 45.0,
            qualities: vec![1080, 720, 480],
            subtitles: vec![OverlaySubtitleOption {
                id: "mpv:1".into(),
                label: "Português".into(),
                active: false,
            }],
            chapters: vec![OverlayChapter {
                title: "Introdução".into(),
                time_secs: 0.0,
            }],
            autoplay: true,
            loop_video: true,
            can_bookmark: true,
            menu: Some(OverlayMenu::Root),
            ..OverlayTelemetry::default()
        };

        let ctx = egui::Context::default();
        for page in [
            OverlayMenu::Root,
            OverlayMenu::Speed,
            OverlayMenu::Quality,
            OverlayMenu::Subtitles,
            OverlayMenu::Chapters,
        ] {
            telemetry.menu = Some(page);
            let mut out = ctx.run_ui(Default::default(), |ui| {
                let _ = show(ui, rect, &telemetry, &style, true);
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn autoplay_button_emits_toggle_autoplay() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(640.0, 360.0));
        let telemetry = OverlayTelemetry {
            autoplay: true,
            ..OverlayTelemetry::default()
        };
        let style = sample_style();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect válido");
        let autoplay_click_pos = Pos2::new(bar_rect.max.x - 196.0, bar_rect.min.y + 36.0);
        let ctx = egui::Context::default();
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, autoplay_click_pos),
            (Some(OverlayAction::ToggleAutoplay), true)
        );
    }

    #[test]
    fn live_progress_and_behind_formatting() {
        assert_eq!(format_live_behind(0.0), "-0:00");
        assert_eq!(format_live_behind(-5.0), "-0:00");
        assert_eq!(format_live_behind(75.0), "-1:15");
        assert_eq!(format_live_behind(3665.0), "-1:01:05");

        assert_eq!(
            action_for_live_progress_click(0.5, Some(300.0), false),
            None
        );
        assert_eq!(
            action_for_live_progress_click(0.5, Some(300.0), true),
            Some(OverlayAction::SeekTo(150.0))
        );
        assert_eq!(
            action_for_live_progress_click(0.99, Some(300.0), true),
            Some(OverlayAction::GoLive)
        );
        assert_eq!(
            action_for_live_progress_click(0.985, Some(300.0), true),
            Some(OverlayAction::GoLive)
        );
    }

    #[test]
    fn live_overlay_badge_emits_go_live_and_hides_vod_only_controls() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let style = sample_style();
        let telemetry = OverlayTelemetry {
            paused: false,
            ended: true,
            is_live: true,
            is_live_dvr: true,
            at_live_edge: false,
            live_behind_secs: 45.0,
            position_secs: 555.0,
            duration_secs: Some(600.0),
            autoplay: true,
            loop_video: true,
            menu: Some(OverlayMenu::Root),
            ..OverlayTelemetry::default()
        };

        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let _ = show(ui, rect, &telemetry, &style, true);
        });
        for clipped in &out.shapes {
            match &clipped.shape {
                Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                Shape::Vec(vec) => {
                    for s in vec {
                        if let Shape::Text(t) = s {
                            texts.push(t.galley.text().to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
        out.textures_delta.clear();

        assert!(
            texts.iter().any(|t| t.contains("Ao vivo (-0:45)")),
            "expected live lag label in {:?}",
            texts
        );
        assert!(
            !texts.iter().any(|t| t == "↺10" || t == "10↻"),
            "skip 10s buttons must be hidden for live"
        );
        assert!(
            !texts
                .iter()
                .any(|t| t.contains("Reprodução automática") || t.contains("Repetir vídeo")),
            "autoplay and loop rows must be hidden in live settings menu"
        );

        // Clicking center when ended+live should NOT emit TogglePlay (center replay suppressed)
        let telemetry_no_menu = OverlayTelemetry {
            menu: None,
            ..telemetry
        };
        assert_eq!(
            click_at(&ctx, rect, &telemetry_no_menu, &style, rect.center()),
            (None, false)
        );

        // Clicking the live pill (right after Play(32) + Next(30) + Mute(30) + Slider(64) + spacings)
        let bar_rect = compute_bar_rect(rect).expect("bar_rect válido");
        let live_pill_pos = Pos2::new(bar_rect.min.x + 12.0 + 195.0, bar_rect.min.y + 36.0);
        assert_eq!(
            click_at(&ctx, rect, &telemetry_no_menu, &style, live_pill_pos),
            (Some(OverlayAction::GoLive), true)
        );
    }

    #[test]
    fn sponsorblock_manual_skip_pill_renders_and_emits_skip_action() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let style = sample_style();
        let active_seg = OverlaySponsorSegment {
            start_secs: 15.0,
            end_secs: 45.0,
            category_label: "Patrocínio",
            color: Color32::from_rgb(0, 212, 0),
        };
        let telemetry = OverlayTelemetry {
            position_secs: 20.0,
            duration_secs: Some(300.0),
            sponsor_segments: vec![active_seg.clone()],
            active_sponsor_segment: Some(active_seg),
            ..OverlayTelemetry::default()
        };

        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let _ = show(ui, rect, &telemetry, &style, true);
        });
        for clipped in &out.shapes {
            match &clipped.shape {
                Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                Shape::Vec(vec) => {
                    for s in vec {
                        if let Shape::Text(t) = s {
                            texts.push(t.galley.text().to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
        out.textures_delta.clear();
        assert!(
            texts.iter().any(|t| t.contains("Pular Patrocínio")),
            "expected SponsorBlock skip pill label in {:?}",
            texts
        );

        let bar_rect = compute_bar_rect(rect).expect("bar_rect válido");
        // Pill sits bottom-right right above bar_rect: right = video_rect.max.x - 16, bottom = bar_rect.min.y - 12
        let pill_click_pos = Pos2::new(rect.max.x - 50.0, bar_rect.min.y - 24.0);
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, pill_click_pos),
            (Some(OverlayAction::SkipSponsorSegment(45.0)), true)
        );
    }

    #[test]
    fn custom_seek_step_secs_renders_custom_step_glyphs() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let style = sample_style();
        let telemetry = OverlayTelemetry {
            seek_step_secs: 15,
            duration_secs: Some(300.0),
            ..OverlayTelemetry::default()
        };
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let _ = show(ui, rect, &telemetry, &style, true);
        });
        for clipped in &out.shapes {
            match &clipped.shape {
                Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                Shape::Vec(vec) => {
                    for s in vec {
                        if let Shape::Text(t) = s {
                            texts.push(t.galley.text().to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
        out.textures_delta.clear();
        assert!(
            texts.iter().any(|t| t.contains("15")),
            "expected custom seek step 15 in {:?}",
            texts
        );
    }

    #[test]
    fn buffered_progress_ratio_clamps_invalid_and_overshoot_values() {
        assert_eq!(buffered_progress_ratio(10.0, None, Some(100.0)), 0.0);
        assert_eq!(buffered_progress_ratio(10.0, Some(20.0), None), 0.0);
        assert_eq!(buffered_progress_ratio(10.0, Some(20.0), Some(0.0)), 0.0);
        assert_eq!(buffered_progress_ratio(10.0, Some(-5.0), Some(100.0)), 0.0);
        assert_eq!(
            buffered_progress_ratio(10.0, Some(f64::NAN), Some(100.0)),
            0.0
        );
        assert!((buffered_progress_ratio(-5.0, Some(25.0), Some(100.0)) - 0.25).abs() < 1e-4);
        assert!((buffered_progress_ratio(30.0, Some(20.0), Some(100.0)) - 0.5).abs() < 1e-4);
        assert_eq!(buffered_progress_ratio(90.0, Some(50.0), Some(100.0)), 1.0);
    }

    #[test]
    fn ab_loop_marker_fractions_handles_valid_and_invalid_ranges() {
        assert_eq!(
            ab_loop_marker_fractions(Some(10.0), Some(30.0), None),
            (None, None, None)
        );
        assert_eq!(
            ab_loop_marker_fractions(Some(10.0), None, Some(100.0)),
            (Some(0.1), None, None)
        );
        assert_eq!(
            ab_loop_marker_fractions(None, Some(40.0), Some(100.0)),
            (None, Some(0.4), None)
        );
        assert_eq!(
            ab_loop_marker_fractions(Some(50.0), Some(20.0), Some(100.0)),
            (Some(0.5), Some(0.2), None)
        );
        assert_eq!(
            ab_loop_marker_fractions(Some(25.0), Some(75.0), Some(100.0)),
            (Some(0.25), Some(0.75), Some((0.25, 0.75)))
        );
    }

    #[test]
    fn compute_hover_pill_rect_stays_inside_video_bounds() {
        let video_rect = Rect::from_min_max(Pos2::new(100.0, 50.0), Pos2::new(320.0, 200.0));
        let left_pill = compute_hover_pill_rect(101.0, 140.0, Vec2::new(120.0, 28.0), video_rect);
        assert!(left_pill.left() >= video_rect.left() + 5.9);
        assert!(left_pill.right() <= video_rect.right() - 5.9);

        let right_pill = compute_hover_pill_rect(319.0, 140.0, Vec2::new(120.0, 28.0), video_rect);
        assert!(right_pill.right() <= video_rect.right() - 5.9);
        assert!(right_pill.left() >= video_rect.left() + 5.9);

        let top_pill = compute_hover_pill_rect(210.0, 52.0, Vec2::new(100.0, 32.0), video_rect);
        assert!(top_pill.top() >= video_rect.top() + 5.9);
    }

    #[test]
    fn speed_presets_reach_3x_and_step_cleanly() {
        assert_eq!(SPEED_PRESETS.last(), Some(&3.0));
        assert_eq!(step_speed_preset(1.0, true), 1.25);
        assert_eq!(step_speed_preset(2.0, true), 2.5);
        assert_eq!(step_speed_preset(2.5, true), 3.0);
        assert_eq!(step_speed_preset(3.0, true), 3.0);
        assert_eq!(step_speed_preset(1.0, false), 0.75);
        assert_eq!(step_speed_preset(0.25, false), 0.25);
    }

    #[test]
    fn ab_loop_markers_buffer_and_hover_chapter_pill_render_without_clipping() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let style = sample_style();
        let telemetry = OverlayTelemetry {
            position_secs: 60.0,
            duration_secs: Some(200.0),
            buffered_secs: Some(40.0),
            loop_a_secs: Some(20.0),
            loop_b_secs: Some(100.0),
            chapters: vec![OverlayChapter {
                title: "Capítulo de Testes Muito Longo".into(),
                time_secs: 30.0,
            }],
            ..OverlayTelemetry::default()
        };
        let bar_rect = compute_bar_rect(rect).expect("bar_rect válido");
        let ctx = egui::Context::default();
        let hover_pos = Pos2::new(bar_rect.min.x + 12.0 + 180.0, bar_rect.min.y + 6.0);
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::PointerMoved(hover_pos));

        let mut texts = Vec::new();
        let mut out = ctx.run_ui(raw, |ui| {
            let _ = show(ui, rect, &telemetry, &style, true);
        });
        for clipped in &out.shapes {
            match &clipped.shape {
                Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                Shape::Vec(vec) => {
                    for s in vec {
                        if let Shape::Text(t) = s {
                            texts.push(t.galley.text().to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
        out.textures_delta.clear();
        assert!(
            texts.iter().any(|t| t == "A"),
            "expected A marker in {:?}",
            texts
        );
        assert!(
            texts.iter().any(|t| t == "B"),
            "expected B marker in {:?}",
            texts
        );
        assert!(
            texts.iter().any(|t| t.contains("Capítulo de Testes")),
            "expected chapter title in hover pill: {:?}",
            texts
        );
    }

    #[test]
    fn audio_only_and_screenshot_toolbar_buttons_emit_actions() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let style = sample_style();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect válido");
        let ctx = egui::Context::default();

        // Right toolbar slot order (each slot 32px + 2px spacing = 34px from right_rect.max.x):
        // 0: Fullscreen, 1: Theater, 2: Mini, 3: Settings, 4: Captions, 5: Autoplay, 6: AudioOnly, 7: Screenshot
        let telemetry = OverlayTelemetry {
            duration_secs: Some(300.0),
            ..OverlayTelemetry::default()
        };
        let row_cy = bar_rect.min.y + 36.0;
        let audio_click = Pos2::new(bar_rect.max.x - 12.0 - 6.0 * 34.0 - 16.0, row_cy);
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, audio_click),
            (Some(OverlayAction::ToggleAudioOnly), true)
        );

        let screenshot_click = Pos2::new(bar_rect.max.x - 12.0 - 7.0 * 34.0 - 16.0, row_cy);
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, screenshot_click),
            (Some(OverlayAction::CaptureScreenshot), true)
        );
    }
}
