use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke,
    Vec2, epaint::Vertex,
};

pub const OVERLAY_HEIGHT: f32 = 64.0;
pub const OVERLAY_HIDE_DELAY_SECS: f64 = 2.5;
pub const SPEED_PRESETS: [f64; 10] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
pub const MIN_CUSTOM_SPEED: f64 = 0.05;
pub const MAX_CUSTOM_SPEED: f64 = 4.0;
pub const CUSTOM_SPEED_STEP: f64 = 0.05;
pub const MAX_VOLUME_PERCENT: f32 = 200.0;
const BAR_SIDE_PADDING: f32 = 12.0;
const PROGRESS_HIT_HEIGHT: f32 = 16.0;
const BUTTON_ROW_HEIGHT: f32 = 38.0;

pub fn clamp_playback_speed(speed: f64) -> f64 {
    if !speed.is_finite() {
        return 1.0;
    }
    let clamped = speed.clamp(MIN_CUSTOM_SPEED, MAX_CUSTOM_SPEED);
    let steps = (clamped / CUSTOM_SPEED_STEP).round();
    (steps * CUSTOM_SPEED_STEP).clamp(MIN_CUSTOM_SPEED, MAX_CUSTOM_SPEED)
}

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
    AudioTracks,
    VideoAudioEq,
    SleepTimer,
    UltrawideZoom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepTimerOption {
    Minutes(u32),
    EndOfVideo,
}

impl SleepTimerOption {
    pub const PRESETS: [Self; 6] = [
        Self::Minutes(10),
        Self::Minutes(15),
        Self::Minutes(30),
        Self::Minutes(45),
        Self::Minutes(60),
        Self::EndOfVideo,
    ];

    pub fn label(self, pt: bool) -> String {
        match self {
            Self::Minutes(mins) => format!("{mins} min"),
            Self::EndOfVideo => if pt { "Fim do vídeo" } else { "End of video" }.to_owned(),
        }
    }

    #[allow(dead_code)]
    pub fn duration_secs(self) -> Option<u64> {
        match self {
            Self::Minutes(mins) => Some(u64::from(mins) * 60),
            Self::EndOfVideo => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpNextOverlayInfo {
    pub title: String,
    pub channel: String,
    pub thumbnail_url: Option<String>,
    pub remaining_secs: f32,
    pub total_secs: f32,
}

impl UpNextOverlayInfo {
    pub fn progress_fraction(&self) -> f32 {
        if !self.total_secs.is_finite()
            || self.total_secs <= 0.0
            || !self.remaining_secs.is_finite()
        {
            return 0.0;
        }
        (self.remaining_secs / self.total_secs).clamp(0.0, 1.0)
    }
}

pub fn format_sleep_timer_summary(
    option: Option<SleepTimerOption>,
    remaining_secs: Option<u64>,
    pt: bool,
) -> String {
    match (option, remaining_secs) {
        (Some(SleepTimerOption::EndOfVideo), _) => {
            if pt {
                "Fim do vídeo".to_owned()
            } else {
                "End of video".to_owned()
            }
        }
        (Some(_), Some(rem)) => format_time(rem as f64),
        (Some(opt), None) => opt.label(pt),
        (None, _) => {
            if pt {
                "Desativado".to_owned()
            } else {
                "Off".to_owned()
            }
        }
    }
}

pub fn format_seek_ripple_label(is_forward: bool, seek_step_secs: u32) -> String {
    let step = seek_step_secs.max(1);
    if is_forward {
        format!("+{step}s")
    } else {
        format!("-{step}s")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayAudioTrack {
    pub id: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioEqPreset {
    #[default]
    Flat,
    Voice,
    BassBoost,
    Night,
}

impl AudioEqPreset {
    pub const ALL: [Self; 4] = [Self::Flat, Self::Voice, Self::BassBoost, Self::Night];

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Flat => "Normal",
            Self::Voice => "Voz / Podcast",
            Self::BassBoost => "Graves reforçados",
            Self::Night => "Noturno (Compressor)",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Voice => "Voice / Podcast",
            Self::BassBoost => "Bass Boost",
            Self::Night => "Night (Compressor)",
        }
    }

    pub fn label(self, pt: bool) -> &'static str {
        if pt { self.label_pt() } else { self.label_en() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayStoryboardFrame {
    pub texture_id: egui::TextureId,
    pub uv: Rect,
    pub aspect_ratio: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayChapter {
    pub title: String,
    pub time_secs: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeatmapPoint {
    pub start_secs: f64,
    pub duration_secs: f64,
    pub intensity: f32,
}

pub const HEATMAP_MOST_REPLAYED_THRESHOLD: f32 = 0.85;
const HEATMAP_MAX_HEIGHT: f32 = 22.0;

fn catmull_rom_clamped(p0: f32, p1: f32, p2: f32, p3: f32, u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    let u2 = u * u;
    let u3 = u2 * u;
    let val = 0.5
        * ((2.0 * p1)
            + (-p0 + p2) * u
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u3);
    val.clamp(0.0, 1.0)
}

fn valid_heatmap_point(point: &HeatmapPoint) -> bool {
    point.start_secs.is_finite()
        && point.start_secs >= 0.0
        && point.duration_secs.is_finite()
        && point.duration_secs > 0.0
        && point.intensity.is_finite()
}

pub fn sample_heatmap_intensity(
    heatmap: &[HeatmapPoint],
    duration_secs: f64,
    time_secs: f64,
) -> f32 {
    if heatmap.is_empty()
        || !duration_secs.is_finite()
        || duration_secs <= 0.0
        || !time_secs.is_finite()
    {
        return 0.0;
    }
    let t = time_secs.clamp(0.0, duration_secs);

    let is_sorted_and_valid = heatmap.iter().all(valid_heatmap_point)
        && heatmap
            .windows(2)
            .all(|w| w[0].start_secs <= w[1].start_secs);

    let owned_sorted: Vec<HeatmapPoint>;
    let points: &[HeatmapPoint] = if is_sorted_and_valid {
        heatmap
    } else {
        let mut buf: Vec<HeatmapPoint> = heatmap
            .iter()
            .copied()
            .filter(valid_heatmap_point)
            .collect();
        buf.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));
        owned_sorted = buf;
        &owned_sorted
    };

    if points.is_empty() {
        return 0.0;
    }

    let first = &points[0];
    if t < first.start_secs {
        return 0.0;
    }
    let last = &points[points.len() - 1];
    let last_end = last.start_secs + last.duration_secs;
    if t > last_end {
        return 0.0;
    }

    let center_of = |p: &HeatmapPoint| p.start_secs + p.duration_secs * 0.5;
    let clamped_intensity = |idx: usize| points[idx].intensity.clamp(0.0, 1.0);

    let first_center = center_of(first);
    if t <= first_center {
        return clamped_intensity(0);
    }

    let last_center = center_of(last);
    if t >= last_center {
        return clamped_intensity(points.len() - 1);
    }

    for i in 0..(points.len() - 1) {
        let curr = &points[i];
        let next = &points[i + 1];
        let c0 = center_of(curr);
        let c1 = center_of(next);
        if t < c0 || t > c1 {
            continue;
        }
        let curr_end = curr.start_secs + curr.duration_secs;
        let is_contiguous = next.start_secs <= curr_end + 1e-3;
        if !is_contiguous {
            if t <= curr_end {
                return clamped_intensity(i);
            } else if t >= next.start_secs {
                return clamped_intensity(i + 1);
            } else {
                return 0.0;
            }
        }
        let span = c1 - c0;
        if span <= 1e-6 {
            return clamped_intensity(i);
        }
        let u = ((t - c0) / span).clamp(0.0, 1.0) as f32;
        let p1 = clamped_intensity(i);
        let p2 = clamped_intensity(i + 1);
        let p0 = if i > 0
            && curr.start_secs <= points[i - 1].start_secs + points[i - 1].duration_secs + 1e-3
        {
            clamped_intensity(i - 1)
        } else {
            p1
        };
        let p3 = if i + 2 < points.len()
            && points[i + 2].start_secs <= next.start_secs + next.duration_secs + 1e-3
        {
            clamped_intensity(i + 2)
        } else {
            p2
        };
        return catmull_rom_clamped(p0, p1, p2, p3, u);
    }

    0.0
}

pub fn is_heatmap_most_replayed(
    heatmap: &[HeatmapPoint],
    duration_secs: f64,
    time_secs: f64,
) -> bool {
    sample_heatmap_intensity(heatmap, duration_secs, time_secs) >= HEATMAP_MOST_REPLAYED_THRESHOLD
}

#[allow(dead_code)]
pub fn heatmap_peak_point(heatmap: &[HeatmapPoint]) -> Option<HeatmapPoint> {
    heatmap
        .iter()
        .copied()
        .filter(valid_heatmap_point)
        .max_by(|a, b| a.intensity.total_cmp(&b.intensity))
        .map(|mut p| {
            p.intensity = p.intensity.clamp(0.0, 1.0);
            p
        })
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
    pub pt: bool,
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
    pub has_previous: bool,
    pub has_queued: bool,
    pub has_active_subtitle: bool,
    pub active_subtitle_text: Option<String>,
    pub subtitles: Vec<OverlaySubtitleOption>,
    pub subtitles_loading: bool,
    pub audio_tracks: Vec<OverlayAudioTrack>,
    pub audio_eq: AudioEqPreset,
    pub brightness: i32,
    pub contrast: i32,
    pub saturation: i32,
    pub storyboard_preview: Option<OverlayStoryboardFrame>,
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
    pub is_live: bool,
    pub is_live_dvr: bool,
    pub at_live_edge: bool,
    pub live_behind_secs: f64,
    pub menu: Option<OverlayMenu>,
    pub stable_volume: bool,
    pub ambient_mode: bool,
    pub show_remaining_time: bool,
    pub caption_style: crate::settings::CaptionStylePrefs,
    pub ultrawide_zoom_mode: crate::settings::UltrawideZoomMode,
    pub ultrawide_custom_zoom: f32,
    pub sleep_timer_option: Option<SleepTimerOption>,
    pub sleep_timer_remaining_secs: Option<u64>,
    pub temp_speed_2x_active: bool,
    pub seek_ripple: Option<(bool, f32)>,
    pub up_next: Option<UpNextOverlayInfo>,
    pub shorts_mode: bool,
}

impl Default for OverlayTelemetry {
    fn default() -> Self {
        Self {
            pt: true,
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
            has_previous: false,
            has_queued: false,
            has_active_subtitle: false,
            active_subtitle_text: None,
            subtitles: Vec::new(),
            subtitles_loading: false,
            audio_tracks: Vec::new(),
            audio_eq: AudioEqPreset::Flat,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            storyboard_preview: None,
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
            is_live: false,
            is_live_dvr: false,
            at_live_edge: false,
            live_behind_secs: 0.0,
            menu: None,
            stable_volume: false,
            ambient_mode: false,
            show_remaining_time: false,
            caption_style: crate::settings::CaptionStylePrefs::default(),
            ultrawide_zoom_mode: crate::settings::UltrawideZoomMode::Fit,
            ultrawide_custom_zoom: 0.0,
            sleep_timer_option: None,
            sleep_timer_remaining_secs: None,
            temp_speed_2x_active: false,
            seek_ripple: None,
            up_next: None,
            shorts_mode: false,
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
    Previous,
    Next,
    ToggleAutoplay,
    ToggleLoop,
    ToggleAudioOnly,
    ToggleCaptions,
    SelectSubtitle(Option<String>),
    SelectAudioTrack(String),
    SetAudioEq(AudioEqPreset),
    SetVideoEq {
        brightness: i32,
        contrast: i32,
        saturation: i32,
    },
    ResetVideoEq,
    SetSpeed(f64),
    SelectQuality(Option<u32>),
    SeekChapter(f64),
    OpenAppSettings,
    SetMenu(Option<OverlayMenu>),
    ToggleMiniPlayer,
    ToggleTheater,
    ToggleFullscreen,
    ToggleRemainingTime,
    ToggleStableVolume,
    ToggleAmbientMode,
    SetSleepTimer(Option<SleepTimerOption>),
    SetCaptionStyle(crate::settings::CaptionStylePrefs),
    SetCaptionAutoTranslate(Option<String>),
    SetUltrawideZoom(crate::settings::UltrawideZoomMode, f32),
    PlayUpNextNow,
    CancelUpNext,
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

pub fn should_keep_overlay_visible_for_pointer(
    fullscreen: bool,
    pointer_pos: Option<Pos2>,
    video_rect: Rect,
) -> bool {
    if fullscreen {
        return false;
    }
    let Some(pos) = pointer_pos else {
        return false;
    };
    compute_bar_rect(video_rect).is_some_and(|bar| bar.contains(pos))
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

#[allow(dead_code)]
fn format_elapsed_total(position_secs: f64, duration_secs: Option<f64>) -> String {
    format_time_display(position_secs, duration_secs, false)
}

pub fn format_time_display(
    position_secs: f64,
    duration_secs: Option<f64>,
    show_remaining: bool,
) -> String {
    let total = match duration_secs {
        Some(d) if d.is_finite() && d > 0.0 => format_time(d),
        _ => "--:--".to_owned(),
    };
    if show_remaining && let Some(d) = duration_secs.filter(|d| d.is_finite() && *d > 0.0) {
        let pos = if position_secs.is_finite() && position_secs > 0.0 {
            position_secs
        } else {
            0.0
        };
        let remaining = (d - pos).max(0.0);
        return format!("-{} / {total}", format_time(remaining));
    }
    let elapsed = format_time(position_secs);
    format!("{elapsed} / {total}")
}

pub(crate) fn compute_bar_rect(video_rect: Rect) -> Option<Rect> {
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

thread_local! {
    static ACTIVE_PROGRESS_DRAG_RECT: std::cell::Cell<Option<Rect>> =
        const { std::cell::Cell::new(None) };
}

fn set_active_progress_drag_rect(rect: Option<Rect>) {
    ACTIVE_PROGRESS_DRAG_RECT.with(|cell| cell.set(rect));
}

fn is_active_progress_drag_for_rect(video_rect: Rect) -> bool {
    ACTIVE_PROGRESS_DRAG_RECT.with(|cell| {
        cell.get().is_some_and(|active| {
            (active.min - video_rect.min).length_sq() < 1.0
                && (active.max - video_rect.max).length_sq() < 1.0
        })
    })
}

pub fn hover_preview_time_secs_with_drag(
    video_rect: Rect,
    duration_secs: Option<f64>,
    pointer_pos: Option<Pos2>,
    dragging_progress: bool,
) -> Option<f64> {
    let duration = duration_secs.filter(|d| d.is_finite() && *d > 0.0)?;
    let pos = pointer_pos?;
    let bar_rect = compute_bar_rect(video_rect)?;
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
    if strip_rect.width() <= 0.0 || (!dragging_progress && !strip_rect.contains(pos)) {
        return None;
    }
    let frac = f64::from((pos.x - strip_rect.min.x) / strip_rect.width()).clamp(0.0, 1.0);
    Some(frac * duration)
}

pub fn hover_preview_time_secs(
    video_rect: Rect,
    duration_secs: Option<f64>,
    pointer_pos: Option<Pos2>,
) -> Option<f64> {
    hover_preview_time_secs_with_drag(
        video_rect,
        duration_secs,
        pointer_pos,
        is_active_progress_drag_for_rect(video_rect),
    )
}

pub(crate) fn paint_vertical_gradient(
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
    Previous,
    Next,
    SkipBack(u32),
    SkipForward(u32),
    Volume(bool, f32),
    Autoplay(bool),
    AudioOnly(bool),
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
            VectorGlyph::Previous => {
                let bar = Rect::from_min_max(
                    Pos2::new(c.x - 6.5, c.y - 6.0),
                    Pos2::new(c.x - 4.0, c.y + 6.0),
                );
                painter.rect_filled(bar, 1.0, fg);
                let pts = vec![
                    Pos2::new(c.x + 6.5, c.y - 6.0),
                    Pos2::new(c.x + 6.5, c.y + 6.0),
                    Pos2::new(c.x - 2.5, c.y),
                ];
                painter.add(Shape::convex_polygon(pts, fg, Stroke::NONE));
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
    pt: bool,
    style: &OverlayStyle,
) -> (Option<f32>, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click_and_drag());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);

    let effective = if muted {
        0.0
    } else {
        volume_percent.clamp(0.0, MAX_VOLUME_PERCENT)
    };

    let mut new_volume = None;
    let interacting = response.dragged() || response.clicked();
    if interacting
        && rect.width() > 8.0
        && let Some(pos) = response.interact_pointer_pos()
    {
        let track_left = rect.left() + 4.0;
        let track_right = rect.right() - 4.0;
        let frac = ((pos.x - track_left) / (track_right - track_left)).clamp(0.0, 1.0);
        new_volume = Some((frac * MAX_VOLUME_PERCENT).round());
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let track_left = rect.left() + 4.0;
        let track_right = rect.right() - 4.0;
        let track_w = (track_right - track_left).max(1.0);
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

        let mid_x = track_left + track_w * (100.0 / MAX_VOLUME_PERCENT);
        let shown_vol = new_volume.unwrap_or(effective);
        let shown_ratio = (shown_vol / MAX_VOLUME_PERCENT).clamp(0.0, 1.0);
        let fill_x = track_left + track_w * shown_ratio;
        let is_boosted = shown_vol > 100.5;
        let boost_accent = Color32::from_rgb(255, 168, 56);

        if fill_x > track_left {
            let base_end = fill_x.min(mid_x);
            let fill_rect = Rect::from_min_max(
                Pos2::new(track_left, cy - 1.5),
                Pos2::new(base_end, cy + 1.5),
            );
            painter.rect_filled(fill_rect, 1.5, style.icon);
            if is_boosted && fill_x > mid_x {
                let boost_rect =
                    Rect::from_min_max(Pos2::new(mid_x, cy - 1.5), Pos2::new(fill_x, cy + 1.5));
                painter.rect_filled(boost_rect, 1.5, boost_accent);
            }
        }
        // Subtle 100% nominal tick marker so the boost region (>100%) is immediately clear.
        painter.line_segment(
            [Pos2::new(mid_x, cy - 3.5), Pos2::new(mid_x, cy + 3.5)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 145)),
        );

        let thumb_radius = if response.hovered() || response.dragged() {
            5.5
        } else {
            4.5
        };
        let thumb_color = if is_boosted {
            boost_accent
        } else {
            Color32::WHITE
        };
        painter.circle_filled(Pos2::new(fill_x, cy), thumb_radius, thumb_color);
    }

    let boost_suffix = if effective > 100.5 {
        if pt { " (Reforço)" } else { " (Boost)" }
    } else {
        ""
    };
    let resp = response.on_hover_text(format!("Volume: {:.0}%{boost_suffix}", effective));
    (new_volume, resp)
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

fn menu_row_sized(
    ui: &mut egui::Ui,
    label: &str,
    value: Option<&str>,
    selected: bool,
    has_chevron: bool,
    row_height: f32,
    style: &OverlayStyle,
) -> egui::Response {
    let width = ui.available_width().max(120.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, row_height), Sense::click());
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

fn menu_row(
    ui: &mut egui::Ui,
    label: &str,
    value: Option<&str>,
    selected: bool,
    has_chevron: bool,
    style: &OverlayStyle,
) -> egui::Response {
    menu_row_sized(ui, label, value, selected, has_chevron, 32.0, style)
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

fn show_settings_popup(
    ui: &mut egui::Ui,
    video_rect: Rect,
    bar_rect: Rect,
    menu: OverlayMenu,
    telemetry: &OverlayTelemetry,
    style: &OverlayStyle,
) -> (Option<OverlayAction>, bool) {
    let popup_w = (video_rect.width() - 16.0).clamp(180.0, 290.0);
    let max_h = (bar_rect.min.y - video_rect.min.y - 10.0).clamp(110.0, 420.0);
    let est_h = match menu {
        OverlayMenu::Root => {
            let base_rows = if telemetry.is_live { 10 } else { 12 };
            let rows = base_rows + usize::from(!telemetry.chapters.is_empty());
            (rows as f32 * 27.5 + 24.0).min(max_h)
        }
        OverlayMenu::Speed => ((SPEED_PRESETS.len() as f32 + 2.5) * 32.0 + 44.0).min(max_h),
        OverlayMenu::Quality => {
            let rows = telemetry.qualities.len().max(1) + 2;
            (rows as f32 * 32.0 + 44.0).min(max_h)
        }
        OverlayMenu::Subtitles => {
            let rows = telemetry.subtitles.len().max(1) + 2;
            (rows as f32 * 32.0 + 220.0).min(max_h)
        }
        OverlayMenu::Chapters => {
            let rows = telemetry.chapters.len().max(1) + 1;
            (rows as f32 * 32.0 + 44.0).min(max_h)
        }
        OverlayMenu::AudioTracks => {
            let rows = telemetry.audio_tracks.len().max(1) + 1;
            (rows as f32 * 32.0 + 44.0).min(max_h)
        }
        OverlayMenu::VideoAudioEq => 316.0_f32.min(max_h),
        OverlayMenu::SleepTimer => {
            let rows = SleepTimerOption::PRESETS.len() + 2;
            (rows as f32 * 32.0 + 56.0).min(max_h)
        }
        OverlayMenu::UltrawideZoom => 220.0_f32.min(max_h),
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
        let pt = telemetry.pt;
        let pick = |pt_text: &'static str, en_text: &'static str| -> &'static str {
            if pt { pt_text } else { en_text }
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
            match menu {
                OverlayMenu::Root => {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                    let root_row_h = 25.5_f32;
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_root_scroll")
                        .max_height(inner.height().max(40.0))
                        .show(ui, |ui| {
                            if !telemetry.is_live {
                                if menu_row_sized(
                                    ui,
                                    pick("Reprodu\u{00e7}\u{00e3}o autom\u{00e1}tica", "Autoplay"),
                                    Some(if telemetry.autoplay {
                                        pick("Ativada", "On")
                                    } else {
                                        pick("Desativada", "Off")
                                    }),
                                    telemetry.autoplay,
                                    false,
                                    root_row_h,
                                    style,
                                )
                                .clicked()
                                {
                                    action = Some(OverlayAction::ToggleAutoplay);
                                }
                                if menu_row_sized(
                                    ui,
                                    pick("Repetir v\u{00ed}deo", "Loop video"),
                                    Some(if telemetry.loop_video {
                                        pick("Ativado", "On")
                                    } else {
                                        pick("Desativado", "Off")
                                    }),
                                    telemetry.loop_video,
                                    false,
                                    root_row_h,
                                    style,
                                )
                                .clicked()
                                {
                                    action = Some(OverlayAction::ToggleLoop);
                                }
                            }
                            if menu_row_sized(
                                ui,
                                pick("Volume est\u{00e1}vel", "Stable volume"),
                                Some(if telemetry.stable_volume {
                                    pick("Ativado", "On")
                                } else {
                                    pick("Desativado", "Off")
                                }),
                                telemetry.stable_volume,
                                false,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::ToggleStableVolume);
                            }
                            if menu_row_sized(
                                ui,
                                pick("Modo ambiente", "Ambient mode"),
                                Some(if telemetry.ambient_mode {
                                    pick("Ativado", "On")
                                } else {
                                    pick("Desativado", "Off")
                                }),
                                telemetry.ambient_mode,
                                false,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::ToggleAmbientMode);
                            }
                            let sleep_summary = format_sleep_timer_summary(
                                telemetry.sleep_timer_option,
                                telemetry.sleep_timer_remaining_secs,
                                pt,
                            );
                            if menu_row_sized(
                                ui,
                                pick("Timer de sono", "Sleep timer"),
                                Some(&sleep_summary),
                                telemetry.sleep_timer_option.is_some(),
                                true,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action =
                                    Some(OverlayAction::SetMenu(Some(OverlayMenu::SleepTimer)));
                            }
                            if menu_row_sized(
                                ui,
                                pick("Velocidade", "Playback speed"),
                                Some(&speed_label(telemetry.speed)),
                                false,
                                true,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Speed)));
                            }
                            if menu_row_sized(
                                ui,
                                pick("Qualidade", "Quality"),
                                Some(&quality_summary(telemetry)),
                                false,
                                true,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Quality)));
                            }
                            if menu_row_sized(
                                ui,
                                pick("Legendas", "Subtitles"),
                                Some(&subtitle_summary(telemetry)),
                                false,
                                true,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Subtitles)));
                            }
                            let audio_summary = telemetry
                                .audio_tracks
                                .iter()
                                .find(|t| t.selected)
                                .map(|t| t.label.as_str())
                                .unwrap_or_else(|| pick("Padr\u{00e3}o", "Default"));
                            if menu_row_sized(
                                ui,
                                pick("Faixa de \u{00e1}udio", "Audio track"),
                                Some(audio_summary),
                                false,
                                true,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action =
                                    Some(OverlayAction::SetMenu(Some(OverlayMenu::AudioTracks)));
                            }
                            let eq_active = telemetry.audio_eq != AudioEqPreset::Flat
                                || telemetry.brightness != 0
                                || telemetry.contrast != 0
                                || telemetry.saturation != 0;
                            if menu_row_sized(
                                ui,
                                pick("Imagem e equalizador", "Video & audio EQ"),
                                Some(telemetry.audio_eq.label(pt)),
                                eq_active,
                                true,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action =
                                    Some(OverlayAction::SetMenu(Some(OverlayMenu::VideoAudioEq)));
                            }
                            let zoom_active = telemetry.ultrawide_zoom_mode
                                != crate::settings::UltrawideZoomMode::Fit;
                            let zoom_summary = if telemetry.ultrawide_zoom_mode
                                == crate::settings::UltrawideZoomMode::Custom
                            {
                                format!("{:.0}%", telemetry.ultrawide_custom_zoom.clamp(0.0, 0.5) * 100.0)
                            } else {
                                telemetry.ultrawide_zoom_mode.label(pt).to_owned()
                            };
                            if menu_row_sized(
                                ui,
                                pick("Zoom ultrawide", "Ultrawide zoom"),
                                Some(&zoom_summary),
                                zoom_active,
                                true,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action =
                                    Some(OverlayAction::SetMenu(Some(OverlayMenu::UltrawideZoom)));
                            }
                            if !telemetry.chapters.is_empty() {
                                let ch_count = format!("{}", telemetry.chapters.len());
                                if menu_row_sized(
                                    ui,
                                    pick("Cap\u{00ed}tulos", "Chapters"),
                                    Some(&ch_count),
                                    false,
                                    true,
                                    root_row_h,
                                    style,
                                )
                                .clicked()
                                {
                                    action =
                                        Some(OverlayAction::SetMenu(Some(OverlayMenu::Chapters)));
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
                            ui.add_space(3.0);
                            if menu_row_sized(
                                ui,
                                pick("Configura\u{00e7}\u{00f5}es do app...", "App settings..."),
                                None,
                                false,
                                false,
                                root_row_h,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::OpenAppSettings);
                            }
                        });
                }
                OverlayMenu::Speed => {
                    if menu_row(
                        ui,
                        pick("‹  Velocidade da reprodu\u{00e7}\u{00e3}o", "‹  Playback speed"),
                        None,
                        false,
                        false,
                        style,
                    )
                    .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_speed_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(pick("Personalizada", "Custom"))
                                        .size(11.5)
                                        .strong()
                                        .color(style.text.gamma_multiply(0.85)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!("{:.2}\u{00d7}", telemetry.speed))
                                                .size(11.5)
                                                .strong()
                                                .color(style.accent),
                                        );
                                    },
                                );
                            });
                            let mut custom_speed = clamp_playback_speed(telemetry.speed);
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut custom_speed,
                                        MIN_CUSTOM_SPEED..=MAX_CUSTOM_SPEED,
                                    )
                                    .step_by(CUSTOM_SPEED_STEP)
                                    .show_value(false),
                                )
                                .changed()
                            {
                                action =
                                    Some(OverlayAction::SetSpeed(clamp_playback_speed(custom_speed)));
                            }
                            ui.add_space(4.0);
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
                    if menu_row(ui, pick("‹  Qualidade", "‹  Quality"), None, false, false, style)
                        .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_quality_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            let auto_label = match telemetry.current_height {
                                Some(h) if telemetry.selected_quality.is_none() => {
                                    format!("Auto ({h}p)")
                                }
                                _ => pick("Autom\u{00e1}tica", "Auto").to_owned(),
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
                                    RichText::new(pick(
                                        "Consultando resolu\u{00e7}\u{00f5}es...",
                                        "Fetching resolutions...",
                                    ))
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
                    if menu_row(ui, pick("‹  Legendas", "‹  Subtitles"), None, false, false, style)
                        .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_subtitles_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            if menu_row(
                                ui,
                                pick("Desativadas", "Off"),
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
                                    RichText::new(pick(
                                        "Buscando faixas de legenda...",
                                        "Loading subtitle tracks...",
                                    ))
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
                            ui.add_space(6.0);
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
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(pick(
                                    "Tradu\u{00e7}\u{00e3}o autom\u{00e1}tica (tlang)",
                                    "Auto-translate (tlang)",
                                ))
                                .size(11.5)
                                .strong()
                                .color(style.text.gamma_multiply(0.85)),
                            );
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                                let cur_tlang = telemetry.caption_style.translate_lang.as_deref();
                                if compact_pill_button(
                                    ui,
                                    pick("Desligado", "Off"),
                                    cur_tlang.is_none(),
                                    true,
                                    style,
                                    pick("Usar idioma original da legenda", "Use original subtitle language"),
                                )
                                .clicked()
                                {
                                    action = Some(OverlayAction::SetCaptionAutoTranslate(None));
                                }
                                for &(code, label) in crate::settings::CAPTION_TRANSLATE_LANGUAGES {
                                    let active = cur_tlang
                                        .is_some_and(|c| c.eq_ignore_ascii_case(code));
                                    if compact_pill_button(ui, label, active, true, style, label)
                                        .clicked()
                                    {
                                        action = Some(OverlayAction::SetCaptionAutoTranslate(
                                            Some(code.to_owned()),
                                        ));
                                    }
                                }
                            });
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(pick("Estilo das legendas", "Caption style"))
                                    .size(11.5)
                                    .strong()
                                    .color(style.text.gamma_multiply(0.85)),
                            );
                            ui.add_space(4.0);
                            let mut next_style = telemetry.caption_style.clone().normalized();
                            let mut font_pct = (next_style.font_scale * 100.0).round() as i32;
                            let font_changed = ui
                                .add(
                                    egui::Slider::new(&mut font_pct, 50..=200)
                                        .suffix("%")
                                        .text(pick("Fonte", "Size")),
                                )
                                .changed();
                            if font_changed {
                                next_style.font_scale = (font_pct as f32 / 100.0).clamp(0.5, 2.0);
                            }
                            let mut bg_pct = (next_style.bg_opacity * 100.0).round() as i32;
                            let bg_changed = ui
                                .add(
                                    egui::Slider::new(&mut bg_pct, 0..=100)
                                        .suffix("%")
                                        .text(pick("Fundo", "Bg")),
                                )
                                .changed();
                            if bg_changed {
                                next_style.bg_opacity = (bg_pct as f32 / 100.0).clamp(0.0, 1.0);
                            }
                            let mut color_changed = false;
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                                for &(hex, label_pt, label_en) in
                                    crate::settings::CAPTION_COLOR_PRESETS
                                {
                                    let selected =
                                        next_style.text_color_hex.eq_ignore_ascii_case(hex);
                                    let lbl = if pt { label_pt } else { label_en };
                                    if compact_pill_button(ui, lbl, selected, true, style, hex)
                                        .clicked()
                                    {
                                        next_style.text_color_hex = hex.to_owned();
                                        color_changed = true;
                                    }
                                }
                            });
                            if font_changed || bg_changed || color_changed {
                                action = Some(OverlayAction::SetCaptionStyle(next_style));
                            }
                        });
                }
                OverlayMenu::Chapters => {
                    if menu_row(
                        ui,
                        pick("‹  Cap\u{00ed}tulos", "‹  Chapters"),
                        None,
                        false,
                        false,
                        style,
                    )
                    .clicked()
                    {
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
                OverlayMenu::AudioTracks => {
                    if menu_row(
                        ui,
                        pick("‹  Faixas de \u{00e1}udio", "‹  Audio tracks"),
                        None,
                        false,
                        false,
                        style,
                    )
                    .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_audio_tracks_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            if telemetry.audio_tracks.is_empty() {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(pick(
                                        "Apenas uma faixa de \u{00e1}udio dispon\u{00ed}vel.",
                                        "Only one audio track available.",
                                    ))
                                    .size(11.5)
                                    .color(style.text.gamma_multiply(0.7)),
                                );
                            }
                            for track in &telemetry.audio_tracks {
                                if menu_row(ui, &track.label, None, track.selected, false, style)
                                    .clicked()
                                {
                                    action =
                                        Some(OverlayAction::SelectAudioTrack(track.id.clone()));
                                }
                            }
                        });
                }
                OverlayMenu::VideoAudioEq => {
                    if menu_row(
                        ui,
                        pick("‹  Imagem e equalizador", "‹  Video & audio EQ"),
                        None,
                        false,
                        false,
                        style,
                    )
                    .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_eq_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(pick("Equalizador de \u{00e1}udio", "Audio EQ preset"))
                                    .size(11.5)
                                    .strong()
                                    .color(style.text.gamma_multiply(0.85)),
                            );
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                                for preset in AudioEqPreset::ALL {
                                    if compact_pill_button(
                                        ui,
                                        preset.label(pt),
                                        telemetry.audio_eq == preset,
                                        true,
                                        style,
                                        preset.label(pt),
                                    )
                                    .clicked()
                                    {
                                        action = Some(OverlayAction::SetAudioEq(preset));
                                    }
                                }
                            });
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(pick("Ajustes de v\u{00ed}deo", "Video picture"))
                                    .size(11.5)
                                    .strong()
                                    .color(style.text.gamma_multiply(0.85)),
                            );
                            ui.add_space(4.0);
                            let mut b = telemetry.brightness;
                            let mut c = telemetry.contrast;
                            let mut s = telemetry.saturation;
                            let b_changed = ui
                                .add(
                                    egui::Slider::new(&mut b, -50..=50)
                                        .text(pick("Brilho", "Brightness")),
                                )
                                .changed();
                            let c_changed = ui
                                .add(
                                    egui::Slider::new(&mut c, -50..=50)
                                        .text(pick("Contraste", "Contrast")),
                                )
                                .changed();
                            let s_changed = ui
                                .add(
                                    egui::Slider::new(&mut s, -50..=50)
                                        .text(pick("Satura\u{00e7}\u{00e3}o", "Saturation")),
                                )
                                .changed();
                            if b_changed || c_changed || s_changed {
                                action = Some(OverlayAction::SetVideoEq {
                                    brightness: b,
                                    contrast: c,
                                    saturation: s,
                                });
                            }
                            ui.add_space(6.0);
                            if compact_pill_button(
                                ui,
                                pick("Redefinir imagem e EQ", "Reset picture & EQ"),
                                false,
                                telemetry.brightness != 0
                                    || telemetry.contrast != 0
                                    || telemetry.saturation != 0
                                    || telemetry.audio_eq != AudioEqPreset::Flat,
                                style,
                                pick(
                                    "Voltar brilho, contraste, satura\u{00e7}\u{00e3}o e EQ ao padr\u{00e3}o",
                                    "Reset picture and audio EQ to defaults",
                                ),
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::ResetVideoEq);
                            }
                        });
                }
                OverlayMenu::SleepTimer => {
                    if menu_row(
                        ui,
                        pick("‹  Timer de sono", "‹  Sleep timer"),
                        None,
                        false,
                        false,
                        style,
                    )
                    .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_sleep_timer_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            if let Some(rem) = telemetry.sleep_timer_remaining_secs {
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(if pt {
                                        format!("Pausando em {}", format_time(rem as f64))
                                    } else {
                                        format!("Pausing in {}", format_time(rem as f64))
                                    })
                                    .size(11.5)
                                    .strong()
                                    .color(style.accent),
                                );
                                ui.add_space(4.0);
                            }
                            if menu_row(
                                ui,
                                pick("Desativado", "Off"),
                                None,
                                telemetry.sleep_timer_option.is_none(),
                                false,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetSleepTimer(None));
                            }
                            for preset in SleepTimerOption::PRESETS {
                                let label = preset.label(pt);
                                let selected = telemetry.sleep_timer_option == Some(preset);
                                if menu_row(ui, &label, None, selected, false, style).clicked() {
                                    action = Some(OverlayAction::SetSleepTimer(Some(preset)));
                                }
                            }
                        });
                }
                OverlayMenu::UltrawideZoom => {
                    if menu_row(
                        ui,
                        pick("‹  Zoom ultrawide", "‹  Ultrawide zoom"),
                        None,
                        false,
                        false,
                        style,
                    )
                    .clicked()
                    {
                        action = Some(OverlayAction::SetMenu(Some(OverlayMenu::Root)));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_ultrawide_zoom_scroll")
                        .max_height((inner.height() - 36.0).max(40.0))
                        .show(ui, |ui| {
                            use crate::settings::UltrawideZoomMode;
                            if menu_row(
                                ui,
                                UltrawideZoomMode::Fit.label(pt),
                                None,
                                telemetry.ultrawide_zoom_mode == UltrawideZoomMode::Fit,
                                false,
                                style,
                            )
                            .clicked()
                            {
                                action =
                                    Some(OverlayAction::SetUltrawideZoom(UltrawideZoomMode::Fit, 0.0));
                            }
                            if menu_row(
                                ui,
                                UltrawideZoomMode::Crop21x9.label(pt),
                                None,
                                telemetry.ultrawide_zoom_mode == UltrawideZoomMode::Crop21x9,
                                false,
                                style,
                            )
                            .clicked()
                            {
                                action = Some(OverlayAction::SetUltrawideZoom(
                                    UltrawideZoomMode::Crop21x9,
                                    0.0,
                                ));
                            }
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(pick(
                                    "Zoom personalizado (video-zoom)",
                                    "Custom zoom (video-zoom)",
                                ))
                                .size(11.5)
                                .strong()
                                .color(style.text.gamma_multiply(0.85)),
                            );
                            ui.add_space(4.0);
                            let mut zoom_pct =
                                (telemetry.ultrawide_custom_zoom.clamp(0.0, 0.5) * 100.0).round()
                                    as i32;
                            if ui
                                .add(
                                    egui::Slider::new(&mut zoom_pct, 0..=50)
                                        .suffix("%")
                                        .text(pick("Zoom", "Zoom")),
                                )
                                .changed()
                            {
                                let next_zoom = (zoom_pct as f32 / 100.0).clamp(0.0, 0.5);
                                let next_mode = if next_zoom <= 0.001 {
                                    UltrawideZoomMode::Fit
                                } else {
                                    UltrawideZoomMode::Custom
                                };
                                action = Some(OverlayAction::SetUltrawideZoom(next_mode, next_zoom));
                            }
                        });
                }
            }
        });
    });

    (action, pointer_inside)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayHitState {
    pub over_controls: bool,
    pub over_volume: bool,
}

#[allow(dead_code)]
pub fn show(
    ui: &mut egui::Ui,
    video_rect: Rect,
    telemetry: &OverlayTelemetry,
    style: &OverlayStyle,
    visible: bool,
) -> (Option<OverlayAction>, bool) {
    show_with_heatmap(ui, video_rect, telemetry, &[], style, visible)
}

#[allow(dead_code)]
pub fn show_with_heatmap(
    ui: &mut egui::Ui,
    video_rect: Rect,
    telemetry: &OverlayTelemetry,
    heatmap: &[HeatmapPoint],
    style: &OverlayStyle,
    visible: bool,
) -> (Option<OverlayAction>, bool) {
    let (action, hit) =
        show_with_heatmap_hit_state(ui, video_rect, telemetry, heatmap, style, visible);
    (action, hit.over_controls)
}

pub fn show_with_heatmap_hit_state(
    ui: &mut egui::Ui,
    video_rect: Rect,
    telemetry: &OverlayTelemetry,
    heatmap: &[HeatmapPoint],
    style: &OverlayStyle,
    visible: bool,
) -> (Option<OverlayAction>, OverlayHitState) {
    let prev_clip = ui.clip_rect();
    let clipped_video = video_rect.intersect(prev_clip);
    if clipped_video.width() <= 0.0 || clipped_video.height() <= 0.0 {
        set_active_progress_drag_rect(None);
        return (None, OverlayHitState::default());
    }

    let bar_rect = compute_bar_rect(video_rect);

    if let Some(sub_text) = telemetry
        .active_subtitle_text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        let cap_style = telemetry.caption_style.clone().normalized();
        let (cr, cg, cb) = cap_style.parse_rgb();
        let cap_text_color = Color32::from_rgb(cr, cg, cb);
        let cap_bg_alpha = (cap_style.bg_opacity * 255.0).round().clamp(0.0, 255.0) as u8;
        let font_px = ((video_rect.height() * 0.042).clamp(14.0, 26.0) * cap_style.font_scale)
            .clamp(10.0, 48.0);
        let max_sub_width = (video_rect.width() * 0.84).max(120.0);
        let painter = ui.painter().with_clip_rect(clipped_video);
        let galley = painter.layout(
            sub_text.to_owned(),
            FontId::proportional(font_px),
            cap_text_color,
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
        if cap_bg_alpha > 0 {
            painter.rect_filled(
                box_rect,
                CornerRadius::same(6),
                Color32::from_rgba_unmultiplied(8, 8, 10, cap_bg_alpha),
            );
        }
        let text_pos = Pos2::new(
            box_rect.center().x - text_size.x * 0.5,
            box_rect.min.y + pad_y,
        );
        painter.galley(text_pos, galley, cap_text_color);
    }

    // Hold-Space / Hold-Mouse temporary 2x speed pill (top-center).
    if telemetry.temp_speed_2x_active && video_rect.height() >= 90.0 {
        let label = if telemetry.pt {
            "Velocidade 2\u{00d7} >>"
        } else {
            "2\u{00d7} >>"
        };
        let painter = ui.painter().with_clip_rect(clipped_video);
        let galley =
            painter.layout_no_wrap(label.to_owned(), FontId::proportional(12.5), Color32::WHITE);
        let pad = Vec2::new(14.0, 6.0);
        let pill_size = galley.size() + pad * 2.0;
        let pill_rect = Rect::from_center_size(
            Pos2::new(
                video_rect.center().x,
                video_rect.top() + 14.0 + pill_size.y * 0.5,
            ),
            pill_size,
        );
        painter.rect_filled(
            pill_rect,
            CornerRadius::same(14),
            Color32::from_rgba_unmultiplied(16, 16, 20, 224),
        );
        painter.rect_stroke(
            pill_rect,
            CornerRadius::same(14),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 44)),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            Pos2::new(pill_rect.left() + pad.x, pill_rect.top() + pad.y),
            galley,
            Color32::WHITE,
        );
    }

    // Double-click left/right +/-10s circular ripple feedback.
    if let Some((is_forward, raw_progress)) = telemetry.seek_ripple
        && raw_progress.is_finite()
        && (0.0..1.0).contains(&raw_progress)
        && video_rect.width() >= 140.0
        && video_rect.height() >= 90.0
    {
        let t = raw_progress.clamp(0.0, 1.0);
        let fade = (1.0 - t).powf(1.2);
        let min_dim = video_rect.width().min(video_rect.height());
        let ripple_r = min_dim * (0.14 + 0.22 * t);
        let center_x = if is_forward {
            video_rect.right() - video_rect.width() * 0.18
        } else {
            video_rect.left() + video_rect.width() * 0.18
        };
        let center = Pos2::new(center_x, video_rect.center().y);
        let painter = ui.painter().with_clip_rect(clipped_video);
        let circle_alpha = (fade * 72.0).round().clamp(0.0, 255.0) as u8;
        let text_alpha = (fade * 255.0).round().clamp(0.0, 255.0) as u8;
        painter.circle_filled(
            center,
            ripple_r,
            Color32::from_rgba_unmultiplied(255, 255, 255, circle_alpha),
        );
        let chevron_str = if is_forward {
            "\u{203a}\u{203a}\u{203a}"
        } else {
            "\u{2039}\u{2039}\u{2039}"
        };
        painter.text(
            Pos2::new(center.x, center.y - 10.0),
            egui::Align2::CENTER_CENTER,
            chevron_str,
            FontId::proportional(20.0),
            Color32::from_rgba_unmultiplied(255, 255, 255, text_alpha),
        );
        let ripple_label = format_seek_ripple_label(is_forward, telemetry.seek_step_secs);
        painter.text(
            Pos2::new(center.x, center.y + 12.0),
            egui::Align2::CENTER_CENTER,
            ripple_label,
            FontId::proportional(13.5),
            Color32::from_rgba_unmultiplied(255, 255, 255, text_alpha),
        );
    }

    // Autoplay "Up Next" countdown card.
    let mut action_up_next: Option<OverlayAction> = None;
    let mut over_up_next = false;
    if let Some(up_next) = &telemetry.up_next
        && telemetry.menu.is_none()
        && video_rect.width() >= 240.0
        && video_rect.height() >= 150.0
    {
        let card_w = (video_rect.width() - 32.0).clamp(220.0, 320.0);
        let card_h = (video_rect.height() - 32.0).clamp(138.0, 196.0);
        let card_rect = Rect::from_center_size(video_rect.center(), Vec2::new(card_w, card_h));
        if ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| card_rect.contains(p))
        {
            over_up_next = true;
        }
        let pt = telemetry.pt;
        ui.scope_builder(egui::UiBuilder::new().max_rect(card_rect), |ui| {
            ui.set_clip_rect(card_rect.intersect(clipped_video));
            let _ = ui.interact(
                card_rect,
                ui.make_persistent_id("fasttube_up_next_backdrop"),
                Sense::click(),
            );
            let painter = ui.painter();
            painter.rect_filled(
                card_rect,
                CornerRadius::same(12),
                Color32::from_rgba_unmultiplied(18, 18, 22, 238),
            );
            painter.rect_stroke(
                card_rect,
                CornerRadius::same(12),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36)),
                egui::StrokeKind::Inside,
            );
            let inner = card_rect.shrink2(Vec2::new(14.0, 12.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                ui.vertical_centered(|ui| {
                    let secs_left = up_next.remaining_secs.ceil().max(0.0) as u32;
                    let header = if pt {
                        format!("A seguir em {secs_left}s")
                    } else {
                        format!("Up next in {secs_left}s")
                    };
                    ui.label(
                        RichText::new(header)
                            .size(11.5)
                            .color(style.text.gamma_multiply(0.75)),
                    );
                    ui.add_space(2.0);
                    let display_title = if up_next.title.chars().count() > 46 {
                        let prefix: String = up_next.title.chars().take(45).collect();
                        format!("{}\u{2026}", prefix.trim_end())
                    } else {
                        up_next.title.clone()
                    };
                    ui.label(
                        RichText::new(display_title)
                            .size(13.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    if !up_next.channel.trim().is_empty() {
                        ui.label(
                            RichText::new(up_next.channel.trim())
                                .size(11.0)
                                .color(style.text.gamma_multiply(0.7)),
                        );
                    }
                    ui.add_space(8.0);
                    // Circular countdown ring with centered play triangle.
                    let (ring_rect, ring_resp) =
                        ui.allocate_exact_size(Vec2::new(50.0, 50.0), Sense::click());
                    let ring_resp = ring_resp
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(if pt { "Reproduzir agora" } else { "Play now" });
                    let rc = ring_rect.center();
                    let radius = 21.0_f32;
                    let p = ui.painter();
                    p.circle_filled(
                        rc,
                        radius,
                        if ring_resp.hovered() {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 42)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 22)
                        },
                    );
                    p.circle_stroke(
                        rc,
                        radius,
                        Stroke::new(2.5, Color32::from_rgba_unmultiplied(255, 255, 255, 48)),
                    );
                    let frac = up_next.progress_fraction();
                    if frac > 0.005 {
                        let segs = ((frac * 32.0).ceil() as usize).max(2);
                        let start = -std::f32::consts::FRAC_PI_2;
                        let sweep = frac * std::f32::consts::TAU;
                        let arc_pts: Vec<Pos2> = (0..=segs)
                            .map(|i| {
                                let a = start + sweep * (i as f32 / segs as f32);
                                Pos2::new(rc.x + a.cos() * radius, rc.y + a.sin() * radius)
                            })
                            .collect();
                        p.add(Shape::line(arc_pts, Stroke::new(2.8, Color32::WHITE)));
                    }
                    let tri = vec![
                        Pos2::new(rc.x - 4.5, rc.y - 7.0),
                        Pos2::new(rc.x - 4.5, rc.y + 7.0),
                        Pos2::new(rc.x + 7.5, rc.y),
                    ];
                    p.add(Shape::convex_polygon(tri, Color32::WHITE, Stroke::NONE));
                    if ring_resp.clicked() {
                        action_up_next = Some(OverlayAction::PlayUpNextNow);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let btn_w = ((inner.width() - 8.0) * 0.5).max(80.0);
                        let cancel_btn = egui::Button::new(
                            RichText::new(if pt { "Cancelar" } else { "Cancel" })
                                .size(12.0)
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 26))
                        .corner_radius(6)
                        .min_size(Vec2::new(btn_w, 28.0));
                        if ui
                            .add(cancel_btn)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                        {
                            action_up_next = Some(OverlayAction::CancelUpNext);
                        }
                        let play_btn = egui::Button::new(
                            RichText::new(if pt { "Reproduzir agora" } else { "Play now" })
                                .size(12.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(style.accent)
                        .corner_radius(6)
                        .min_size(Vec2::new(btn_w, 28.0));
                        if ui
                            .add(play_btn)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked()
                            && action_up_next.is_none()
                        {
                            action_up_next = Some(OverlayAction::PlayUpNextNow);
                        }
                    });
                });
            });
        });
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

    if telemetry.shorts_mode
        && !telemetry.fullscreen
        && !telemetry.theater
        && !telemetry.mini_player
    {
        let (shorts_action, shorts_hit) =
            show_shorts_overlay(ui, video_rect, clipped_video, telemetry, style);
        return (
            action_up_next.or(sponsor_skip_action).or(shorts_action),
            OverlayHitState {
                over_controls: over_up_next || over_sponsor_pill || shorts_hit.over_controls,
                over_volume: shorts_hit.over_volume && !over_up_next && !over_sponsor_pill,
            },
        );
    }

    if !visible {
        set_active_progress_drag_rect(None);
        return (
            action_up_next.or(sponsor_skip_action),
            OverlayHitState {
                over_controls: over_up_next || over_sponsor_pill,
                over_volume: false,
            },
        );
    }

    let Some(bar_rect) = bar_rect else {
        set_active_progress_drag_rect(None);
        return (
            action_up_next.or(sponsor_skip_action),
            OverlayHitState {
                over_controls: over_up_next || over_sponsor_pill,
                over_volume: false,
            },
        );
    };

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let mut over_controls = pointer_pos.is_some_and(|pos| bar_rect.contains(pos));
    let mut over_volume = false;
    let mut over_popup = false;

    let mut action_top: Option<OverlayAction> = None;
    let mut action_center: Option<OverlayAction> = None;
    let mut action_popup: Option<OverlayAction> = None;
    let mut action_play: Option<OverlayAction> = None;
    let mut action_prev: Option<OverlayAction> = None;
    let mut action_next: Option<OverlayAction> = None;
    let mut action_seek_btn: Option<OverlayAction> = None;
    let mut action_progress: Option<OverlayAction> = None;
    let mut action_mute: Option<OverlayAction> = None;
    let mut action_volume: Option<OverlayAction> = None;
    let mut action_live_pill: Option<OverlayAction> = None;
    let mut action_time_toggle: Option<OverlayAction> = None;
    let mut action_chapter_pill: Option<OverlayAction> = None;
    let mut action_autoplay: Option<OverlayAction> = None;
    let mut action_audio_only: Option<OverlayAction> = None;
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
        && telemetry.up_next.is_none()
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
        let (popup_act, popup_hovered) =
            show_settings_popup(ui, video_rect, bar_rect, menu, telemetry, style);
        action_popup = popup_act;
        over_popup = popup_hovered;
        if popup_hovered {
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

            let is_dragging_progress = can_scrub && progress_resp.dragged();
            let retain_scrub_preview =
                is_dragging_progress || (can_scrub && progress_resp.is_pointer_button_down_on());
            set_active_progress_drag_rect(retain_scrub_preview.then_some(video_rect));
            let is_progress_hot = can_scrub
                && (progress_resp.hovered()
                    || progress_resp.dragged()
                    || progress_resp.is_pointer_button_down_on());
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

            let scrub_pointer_pos = if is_dragging_progress {
                progress_resp
                    .interact_pointer_pos()
                    .or_else(|| ui.input(|i| i.pointer.interact_pos()))
                    .or(pointer_pos)
            } else {
                pointer_pos
            };
            let hover_fraction = if is_progress_hot && strip_rect.width() > 0.0 {
                if is_dragging_progress {
                    drag_fraction.map(|f| f as f32).or_else(|| {
                        scrub_pointer_pos.map(|p| {
                            ((p.x - strip_rect.min.x) / strip_rect.width()).clamp(0.0, 1.0)
                        })
                    })
                } else {
                    scrub_pointer_pos
                        .map(|p| ((p.x - strip_rect.min.x) / strip_rect.width()).clamp(0.0, 1.0))
                }
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

                let mut heatmap_top_y = strip_rect.min.y;
                if is_progress_hot
                    && !telemetry.is_live
                    && !heatmap.is_empty()
                    && line_rect.width() >= 24.0
                {
                    let baseline_y = line_rect.min.y - 1.5;
                    let max_h =
                        HEATMAP_MAX_HEIGHT.min((baseline_y - video_rect.min.y - 8.0).max(0.0));
                    if max_h >= 6.0 {
                        let steps = ((line_rect.width() / 3.5).round() as usize).clamp(32, 220);
                        let mut mesh = egui::Mesh::default();
                        let mut contour_pts = Vec::with_capacity(steps + 1);
                        let mut highlight_pts = Vec::new();
                        let hover_f = hover_fraction.unwrap_or(-1.0);
                        let mut any_positive = false;

                        for idx in 0..=steps {
                            let frac = idx as f32 / steps as f32;
                            let x = line_rect.min.x + line_rect.width() * frac;
                            let sample_t = f64::from(frac) * duration;
                            let intensity = sample_heatmap_intensity(heatmap, duration, sample_t);
                            if intensity > 0.005 {
                                any_positive = true;
                            }
                            let h = if intensity <= 0.005 {
                                0.0
                            } else {
                                (intensity * max_h).max(1.5)
                            };
                            let top_y = baseline_y - h;
                            let top_pos = Pos2::new(x, top_y);
                            let bot_pos = Pos2::new(x, baseline_y);
                            contour_pts.push(top_pos);

                            let dist_to_hover = (frac - hover_f).abs();
                            let near_hover = hover_f >= 0.0 && dist_to_hover <= 0.055;
                            let is_peak = intensity >= HEATMAP_MOST_REPLAYED_THRESHOLD;
                            if is_peak && near_hover {
                                highlight_pts.push(top_pos);
                            }

                            let (top_color, bot_color) = if is_peak && near_hover {
                                (
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 210),
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 92),
                                )
                            } else if near_hover || is_peak {
                                (
                                    Color32::from_rgba_unmultiplied(245, 247, 252, 148),
                                    Color32::from_rgba_unmultiplied(230, 234, 242, 56),
                                )
                            } else {
                                (
                                    Color32::from_rgba_unmultiplied(228, 232, 240, 102),
                                    Color32::from_rgba_unmultiplied(200, 205, 215, 34),
                                )
                            };

                            let base_v = mesh.vertices.len() as u32;
                            mesh.vertices.push(Vertex {
                                pos: top_pos,
                                uv: egui::epaint::WHITE_UV,
                                color: top_color,
                            });
                            mesh.vertices.push(Vertex {
                                pos: bot_pos,
                                uv: egui::epaint::WHITE_UV,
                                color: bot_color,
                            });
                            if idx > 0 {
                                let prev_top = base_v - 2;
                                let prev_bot = base_v - 1;
                                mesh.add_triangle(prev_top, prev_bot, base_v);
                                mesh.add_triangle(prev_bot, base_v + 1, base_v);
                            }
                        }

                        if any_positive {
                            heatmap_top_y = (baseline_y - max_h).min(strip_rect.min.y);
                            let painter = ui.painter().with_clip_rect(clipped_video);
                            painter.add(Shape::mesh(mesh));
                            painter.add(Shape::line(
                                contour_pts,
                                Stroke::new(
                                    1.25,
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 176),
                                ),
                            ));
                            if highlight_pts.len() >= 2 {
                                painter.add(Shape::line(
                                    highlight_pts,
                                    Stroke::new(2.0, Color32::WHITE),
                                ));
                            }
                        }
                    }
                }

                if let Some(hf) = hover_fraction {
                    let preview_anchor_x = scrub_pointer_pos
                        .map(|p| p.x)
                        .unwrap_or_else(|| strip_rect.min.x + strip_rect.width() * hf);
                    let preview_secs = f64::from(hf) * duration;
                    let chapter_title = current_chapter_title(&telemetry.chapters, preview_secs)
                        .map(str::trim)
                        .filter(|t| !t.is_empty());
                    let most_replayed_badge = (!telemetry.is_live
                        && is_heatmap_most_replayed(heatmap, duration, preview_secs))
                    .then_some(if telemetry.pt {
                        "Mais assistido"
                    } else {
                        "Most replayed"
                    });
                    let mut time_line = if telemetry.is_live {
                        if f64::from(hf) >= LIVE_EDGE_SNAP_FRACTION {
                            if telemetry.pt { "Ao vivo" } else { "Live" }.to_owned()
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
                    let most_replayed_galley = most_replayed_badge.map(|badge| {
                        ui.painter().layout_no_wrap(
                            badge.to_owned(),
                            FontId::proportional(10.5),
                            Color32::WHITE,
                        )
                    });
                    let time_galley = ui.painter().layout(
                        time_line,
                        FontId::proportional(11.5),
                        Color32::WHITE,
                        max_text_w,
                    );

                    let storyboard_size = telemetry.storyboard_preview.and_then(|frame| {
                        if telemetry.is_live || video_rect.height() < 180.0 {
                            return None;
                        }
                        let ratio = if frame.aspect_ratio.is_finite() && frame.aspect_ratio > 0.2 {
                            frame.aspect_ratio.clamp(0.5, 3.0)
                        } else {
                            16.0 / 9.0
                        };
                        let target_w = (video_rect.width() * 0.24).clamp(112.0, 176.0);
                        let max_h = (video_rect.height() * 0.34).clamp(60.0, 110.0);
                        let mut w = target_w;
                        let mut h = w / ratio;
                        if h > max_h {
                            h = max_h;
                            w = h * ratio;
                        }
                        Some(Vec2::new(w.round(), h.round()))
                    });

                    let pad = Vec2::new(9.0, 5.0);
                    let line_gap = if chapter_galley.is_some() { 2.0 } else { 0.0 };
                    let badge_pad = Vec2::new(6.0, 2.0);
                    let badge_box_size = most_replayed_galley
                        .as_ref()
                        .map(|g| g.size() + badge_pad * 2.0);
                    let badge_gap = if badge_box_size.is_some() { 3.0 } else { 0.0 };
                    let thumb_gap = if storyboard_size.is_some() { 6.0 } else { 0.0 };
                    let content_w = chapter_galley
                        .as_ref()
                        .map_or(0.0, |g| g.size().x)
                        .max(badge_box_size.map_or(0.0, |s| s.x))
                        .max(time_galley.size().x)
                        .max(storyboard_size.map_or(0.0, |s| s.x));
                    let content_h = storyboard_size.map_or(0.0, |s| s.y + thumb_gap)
                        + chapter_galley
                            .as_ref()
                            .map_or(0.0, |g| g.size().y + line_gap)
                        + badge_box_size.map_or(0.0, |s| s.y + badge_gap)
                        + time_galley.size().y;
                    let pill_size = Vec2::new(content_w + pad.x * 2.0, content_h + pad.y * 2.0);
                    let pill_rect = compute_hover_pill_rect(
                        preview_anchor_x,
                        heatmap_top_y,
                        pill_size,
                        video_rect,
                    );
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
                    if let (Some(frame), Some(thumb_sz)) =
                        (telemetry.storyboard_preview, storyboard_size)
                    {
                        let thumb_x =
                            (pill_rect.center().x - thumb_sz.x * 0.5).max(pill_rect.left() + pad.x);
                        let thumb_rect = Rect::from_min_size(Pos2::new(thumb_x, text_y), thumb_sz);
                        let thumb_painter = pill_painter.with_clip_rect(pill_rect);
                        thumb_painter.image(frame.texture_id, thumb_rect, frame.uv, Color32::WHITE);
                        thumb_painter.rect_stroke(
                            thumb_rect,
                            CornerRadius::same(3),
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36)),
                            egui::StrokeKind::Inside,
                        );
                        text_y += thumb_sz.y + thumb_gap;
                    }
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
                    if let (Some(badge_galley), Some(box_sz)) =
                        (most_replayed_galley, badge_box_size)
                    {
                        let box_x =
                            (pill_rect.center().x - box_sz.x * 0.5).max(pill_rect.left() + pad.x);
                        let badge_rect = Rect::from_min_size(Pos2::new(box_x, text_y), box_sz);
                        let badge_painter = pill_painter.with_clip_rect(pill_rect);
                        badge_painter.rect_filled(
                            badge_rect,
                            CornerRadius::same(4),
                            Color32::from_rgba_unmultiplied(255, 255, 255, 32),
                        );
                        badge_painter.rect_stroke(
                            badge_rect,
                            CornerRadius::same(4),
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 58)),
                            egui::StrokeKind::Inside,
                        );
                        badge_painter.galley(
                            Pos2::new(
                                badge_rect.left() + badge_pad.x,
                                badge_rect.top() + badge_pad.y,
                            ),
                            badge_galley,
                            Color32::WHITE,
                        );
                        text_y += box_sz.y + badge_gap;
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

                    if telemetry.has_previous
                        && icon_button(
                            ui,
                            Vec2::new(30.0, 30.0),
                            VectorGlyph::Previous,
                            style,
                            true,
                            false,
                            "V\u{00ed}deo anterior (Shift+P)",
                        )
                        .clicked()
                    {
                        action_prev = Some(OverlayAction::Previous);
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

                    let mute_resp = icon_button(
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
                    );
                    if mute_resp.clicked() {
                        action_mute = Some(OverlayAction::ToggleMute);
                    }

                    let mut volume_rect = mute_resp.rect;
                    let mut volume_widget_hot = mute_resp.hovered();

                    if slider_w > 0.0 {
                        let (vol_changed, vol_resp) = volume_slider(
                            ui,
                            slider_w,
                            telemetry.volume_percent,
                            telemetry.muted,
                            telemetry.pt,
                            style,
                        );
                        let vol_hot = vol_resp.hovered() || vol_resp.dragged();
                        if vol_hot {
                            over_controls = true;
                            volume_widget_hot = true;
                        }
                        if let Some(v) = vol_changed {
                            action_volume = Some(OverlayAction::SetVolume(v));
                        }
                        volume_rect = volume_rect.union(vol_resp.rect);
                    }

                    let visible_volume_rect =
                        volume_rect.intersect(left_rect.intersect(clipped_video));
                    if visible_volume_rect.is_positive()
                        && (volume_widget_hot
                            || pointer_pos.is_some_and(|pos| visible_volume_rect.contains(pos)))
                    {
                        over_volume = true;
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
                        let time_label = format_time_display(
                            shown_pos,
                            telemetry.duration_secs,
                            telemetry.show_remaining_time,
                        );
                        ui.add_space(4.0);
                        let time_resp = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(time_label).color(style.text).size(12.0),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .on_hover_text(if telemetry.pt {
                                "Alternar tempo decorrido / restante"
                            } else {
                                "Toggle elapsed / remaining time"
                            });
                        if time_resp.hovered() {
                            over_controls = true;
                        }
                        if time_resp.clicked() {
                            action_time_toggle = Some(OverlayAction::ToggleRemainingTime);
                        }
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
                });
            });
        });
    } else {
        set_active_progress_drag_rect(None);
    }

    let action = action_up_next
        .or(sponsor_skip_action)
        .or(action_popup)
        .or(action_top)
        .or(action_center)
        .or(action_play)
        .or(action_prev)
        .or(action_next)
        .or(action_seek_btn)
        .or(action_progress)
        .or(action_mute)
        .or(action_volume)
        .or(action_live_pill)
        .or(action_time_toggle)
        .or(action_chapter_pill)
        .or(action_autoplay)
        .or(action_audio_only)
        .or(action_captions)
        .or(action_settings)
        .or(action_mini)
        .or(action_theater)
        .or(action_fullscreen);

    over_controls |= over_up_next || over_sponsor_pill;
    if over_up_next || over_sponsor_pill || over_popup {
        over_volume = false;
    }

    (
        action,
        OverlayHitState {
            over_controls,
            over_volume,
        },
    )
}

fn show_shorts_overlay(
    ui: &mut egui::Ui,
    video_rect: Rect,
    clipped_video: Rect,
    telemetry: &OverlayTelemetry,
    style: &OverlayStyle,
) -> (Option<OverlayAction>, OverlayHitState) {
    let painter = ui.painter().with_clip_rect(clipped_video);
    let scrim_h = 68.0_f32.min(video_rect.height() * 0.25);
    if scrim_h > 4.0 {
        let bands = 6_usize;
        let band_h = scrim_h / bands as f32;
        for i in 0..bands {
            let t = i as f32 / bands as f32;
            let alpha = ((1.0 - t).powf(1.4) * 110.0).round() as u8;
            let band_rect = Rect::from_min_max(
                Pos2::new(video_rect.left(), video_rect.top() + i as f32 * band_h),
                Pos2::new(
                    video_rect.right(),
                    video_rect.top() + (i + 1) as f32 * band_h,
                ),
            );
            painter.rect_filled(
                band_rect,
                0.0,
                Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
            );
        }
    }

    let pointer_pos = ui.input(|i| i.pointer.hover_pos());
    let mut over_controls = false;
    let mut over_volume = false;
    let mut action_play: Option<OverlayAction> = None;
    let mut action_mute: Option<OverlayAction> = None;
    let mut action_volume: Option<OverlayAction> = None;
    let mut action_progress: Option<OverlayAction> = None;

    // Top-left circular Play/Pause button.
    let play_rect = Rect::from_min_size(
        Pos2::new(video_rect.left() + 12.0, video_rect.top() + 12.0),
        Vec2::splat(40.0),
    );
    let play_resp = ui
        .interact(
            play_rect,
            ui.make_persistent_id("fasttube_shorts_play_btn"),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if telemetry.paused || telemetry.ended {
            if telemetry.pt { "Reproduzir" } else { "Play" }
        } else if telemetry.pt {
            "Pausar"
        } else {
            "Pause"
        });
    if play_resp.hovered() || pointer_pos.is_some_and(|p| play_rect.contains(p)) {
        over_controls = true;
    }
    let play_bg = if play_resp.hovered() {
        Color32::from_rgba_unmultiplied(15, 15, 15, 210)
    } else {
        Color32::from_rgba_unmultiplied(15, 15, 15, 150)
    };
    painter.circle_filled(play_rect.center(), 20.0, play_bg);
    let c = play_rect.center();
    if telemetry.paused || telemetry.ended {
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(c.x - 4.5, c.y - 6.5),
                Pos2::new(c.x - 4.5, c.y + 6.5),
                Pos2::new(c.x + 6.5, c.y),
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));
    } else {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(c.x - 5.0, c.y - 6.5),
                Pos2::new(c.x - 1.5, c.y + 6.5),
            ),
            1.0,
            Color32::WHITE,
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(c.x + 1.5, c.y - 6.5),
                Pos2::new(c.x + 5.0, c.y + 6.5),
            ),
            1.0,
            Color32::WHITE,
        );
    }
    if play_resp.clicked() {
        action_play = Some(OverlayAction::TogglePlay);
    }

    // Top-left circular Volume/Mute button with hover-expandable volume slider pill.
    let vol_state_id = ui.make_persistent_id("fasttube_shorts_vol_hover");
    let prev_expanded: bool = ui.data(|d| d.get_temp(vol_state_id)).unwrap_or(false);
    let mute_rect = Rect::from_min_size(
        Pos2::new(video_rect.left() + 60.0, video_rect.top() + 12.0),
        Vec2::splat(40.0),
    );
    let max_pill_w = (video_rect.width() - 124.0).clamp(40.0, 134.0);
    let pill_hover_rect = Rect::from_min_size(
        mute_rect.min,
        Vec2::new(if prev_expanded { max_pill_w } else { 40.0 }, 40.0),
    );
    let mut show_slider = max_pill_w >= 112.0
        && (prev_expanded && ui.input(|i| i.pointer.any_down())
            || pointer_pos.is_some_and(|p| pill_hover_rect.contains(p)));
    if show_slider {
        let full_pill = Rect::from_min_size(mute_rect.min, Vec2::new(max_pill_w, 40.0));
        painter.rect_filled(
            full_pill,
            CornerRadius::same(20),
            Color32::from_rgba_unmultiplied(15, 15, 15, 175),
        );
        if pointer_pos.is_some_and(|p| full_pill.contains(p)) {
            over_controls = true;
        }
    }

    let mute_resp = ui
        .interact(
            mute_rect,
            ui.make_persistent_id("fasttube_shorts_mute_btn"),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if telemetry.muted {
            if telemetry.pt { "Ativar som" } else { "Unmute" }
        } else if telemetry.pt {
            "Silenciar"
        } else {
            "Mute"
        });
    if mute_resp.hovered() || pointer_pos.is_some_and(|p| mute_rect.contains(p)) {
        over_controls = true;
        over_volume = true;
        show_slider |= max_pill_w >= 112.0;
    }
    if !show_slider {
        let mute_bg = if mute_resp.hovered() {
            Color32::from_rgba_unmultiplied(15, 15, 15, 210)
        } else {
            Color32::from_rgba_unmultiplied(15, 15, 15, 150)
        };
        painter.circle_filled(mute_rect.center(), 20.0, mute_bg);
    }
    let vc = mute_rect.center();
    let speaker = vec![
        Pos2::new(vc.x - 7.5, vc.y - 2.5),
        Pos2::new(vc.x - 4.0, vc.y - 2.5),
        Pos2::new(vc.x + 0.5, vc.y - 6.5),
        Pos2::new(vc.x + 0.5, vc.y + 6.5),
        Pos2::new(vc.x - 4.0, vc.y + 2.5),
        Pos2::new(vc.x - 7.5, vc.y + 2.5),
    ];
    painter.add(Shape::convex_polygon(speaker, Color32::WHITE, Stroke::NONE));
    if telemetry.muted || telemetry.volume_percent <= 0.5 {
        painter.line_segment(
            [
                Pos2::new(vc.x + 3.0, vc.y - 3.5),
                Pos2::new(vc.x + 8.0, vc.y + 3.5),
            ],
            Stroke::new(1.8, Color32::WHITE),
        );
        painter.line_segment(
            [
                Pos2::new(vc.x + 8.0, vc.y - 3.5),
                Pos2::new(vc.x + 3.0, vc.y + 3.5),
            ],
            Stroke::new(1.8, Color32::WHITE),
        );
    } else {
        let arc1 = [
            Pos2::new(vc.x + 3.0, vc.y - 3.0),
            Pos2::new(vc.x + 4.5, vc.y),
            Pos2::new(vc.x + 3.0, vc.y + 3.0),
        ];
        painter.add(Shape::line(arc1.to_vec(), Stroke::new(1.6, Color32::WHITE)));
        if telemetry.volume_percent > 45.0 {
            let arc2 = [
                Pos2::new(vc.x + 5.5, vc.y - 5.5),
                Pos2::new(vc.x + 8.0, vc.y),
                Pos2::new(vc.x + 5.5, vc.y + 5.5),
            ];
            painter.add(Shape::line(arc2.to_vec(), Stroke::new(1.6, Color32::WHITE)));
        }
    }
    if mute_resp.clicked() {
        action_mute = Some(OverlayAction::ToggleMute);
    }

    if show_slider {
        let slider_rect = Rect::from_min_max(
            Pos2::new(mute_rect.right() + 2.0, mute_rect.top() + 6.0),
            Pos2::new(
                mute_rect.left() + max_pill_w - 8.0,
                mute_rect.bottom() - 6.0,
            ),
        );
        let slider_w = slider_rect.width().max(24.0);
        let (vol_changed, vol_resp) = ui
            .scope_builder(egui::UiBuilder::new().max_rect(slider_rect), |ui| {
                ui.set_clip_rect(slider_rect.intersect(clipped_video));
                volume_slider(
                    ui,
                    slider_w,
                    telemetry.volume_percent,
                    telemetry.muted,
                    telemetry.pt,
                    style,
                )
            })
            .inner;
        let vol_dragging = vol_resp.hovered() || vol_resp.dragged();
        if let Some(v) = vol_changed {
            action_volume = Some(OverlayAction::SetVolume(v));
        }
        over_controls |= vol_dragging;
        let in_pill = pointer_pos.is_some_and(|p| {
            Rect::from_min_size(mute_rect.min, Vec2::new(max_pill_w, 40.0)).contains(p)
        });
        over_volume |= vol_dragging || in_pill;
        show_slider = vol_dragging || in_pill;
    }
    ui.data_mut(|d| d.insert_temp(vol_state_id, show_slider));

    // Thin YouTube-red bottom progress bar along the bottom edge of the 9:16 card.
    let strip_rect = Rect::from_min_max(
        Pos2::new(
            video_rect.left() + 4.0,
            (video_rect.bottom() - 14.0).max(video_rect.top() + 60.0),
        ),
        Pos2::new(video_rect.right() - 4.0, video_rect.bottom()),
    );
    let can_scrub = telemetry
        .duration_secs
        .is_some_and(|d| d.is_finite() && d > 0.0);
    let progress_id = ui.make_persistent_id("fasttube_shorts_progress_strip");
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
    if progress_resp.hovered() || pointer_pos.is_some_and(|p| strip_rect.contains(p)) {
        over_controls = true;
    }

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
        let frac = f64::from((pos.x - strip_rect.min.x) / strip_rect.width()).clamp(0.0, 1.0);
        drag_fraction = Some(frac);
        ui.data_mut(|d| d.insert_temp(progress_id, frac));
    }
    if can_scrub && progress_resp.drag_stopped() {
        if let Some(frac) = drag_fraction.take() {
            action_progress = action_for_progress_click(frac, telemetry.duration_secs);
        }
        ui.data_mut(|d| d.remove::<f64>(progress_id));
    } else if can_scrub
        && progress_resp.clicked()
        && strip_rect.width() > 0.0
        && let Some(pos) = progress_resp
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.interact_pos()))
    {
        let frac = f64::from((pos.x - strip_rect.min.x) / strip_rect.width()).clamp(0.0, 1.0);
        action_progress = action_for_progress_click(frac, telemetry.duration_secs);
    }

    let is_dragging = can_scrub && progress_resp.dragged();
    let retain_preview = is_dragging || (can_scrub && progress_resp.is_pointer_button_down_on());
    set_active_progress_drag_rect(retain_preview.then_some(video_rect));
    over_controls |= retain_preview;

    let bar_h = if progress_resp.hovered() || retain_preview {
        5.0
    } else {
        3.0
    };
    let bar_bottom = video_rect.bottom() - 1.5;
    let bar_rect_visual = Rect::from_min_max(
        Pos2::new(strip_rect.left(), bar_bottom - bar_h),
        Pos2::new(strip_rect.right(), bar_bottom),
    );
    painter.rect_filled(
        bar_rect_visual,
        1.5,
        Color32::from_rgba_unmultiplied(255, 255, 255, 72),
    );

    let played_ratio = drag_fraction
        .unwrap_or_else(|| {
            telemetry
                .duration_secs
                .filter(|d| d.is_finite() && *d > 0.0)
                .map_or(0.0, |d| (telemetry.position_secs / d).clamp(0.0, 1.0))
        })
        .clamp(0.0, 1.0) as f32;
    let buffered_ratio = telemetry
        .buffered_secs
        .zip(telemetry.duration_secs)
        .filter(|(b, d)| b.is_finite() && d.is_finite() && *d > 0.0)
        .map_or(0.0, |(b, d)| (b / d).clamp(0.0, 1.0)) as f32;

    if buffered_ratio > played_ratio {
        let buf_end =
            bar_rect_visual.left() + bar_rect_visual.width() * buffered_ratio.clamp(0.0, 1.0);
        painter.rect_filled(
            Rect::from_min_max(
                bar_rect_visual.min,
                Pos2::new(buf_end, bar_rect_visual.max.y),
            ),
            1.5,
            Color32::from_rgba_unmultiplied(255, 255, 255, 125),
        );
    }
    let yt_red = Color32::from_rgb(255, 0, 51);
    if played_ratio > 0.0 {
        let fill_end = bar_rect_visual.left() + bar_rect_visual.width() * played_ratio;
        painter.rect_filled(
            Rect::from_min_max(
                bar_rect_visual.min,
                Pos2::new(fill_end, bar_rect_visual.max.y),
            ),
            1.5,
            yt_red,
        );
        if progress_resp.hovered() || retain_preview {
            painter.circle_filled(Pos2::new(fill_end, bar_rect_visual.center().y), 5.5, yt_red);
        }
    }

    let mut action_popup: Option<OverlayAction> = None;
    if let Some(menu) = telemetry.menu {
        let popup_anchor = Rect::from_min_max(
            Pos2::new(
                video_rect.left(),
                (video_rect.bottom() - 16.0).max(video_rect.top()),
            ),
            video_rect.max,
        );
        let (act, over_popup) =
            show_settings_popup(ui, video_rect, popup_anchor, menu, telemetry, style);
        action_popup = act;
        over_controls |= over_popup;
        if over_popup {
            over_volume = false;
        }
    }

    let action = action_popup
        .or(action_play)
        .or(action_mute)
        .or(action_volume)
        .or(action_progress);
    (
        action,
        OverlayHitState {
            over_controls,
            over_volume,
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerContextAction {
    ToggleLoop,
    ToggleMiniPlayer,
    CopyVideoUrl,
    CopyVideoUrlAtCurrentTime,
    CopyEmbedCode,
    CopyDebugInfo,
    TroubleshootPlayback,
    ToggleStatsForNerds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerContextMenuState {
    pub pt: bool,
    pub is_live: bool,
    pub loop_video: bool,
    pub mini_player: bool,
    pub can_copy_embed: bool,
    pub stats_for_nerds_open: bool,
}

#[derive(Clone, Copy)]
enum ContextMenuGlyph {
    Loop,
    MiniPlayer,
    Link,
    Embed,
    Bug,
    Help,
    Info,
}

fn paint_context_menu_icon(
    painter: &egui::Painter,
    center: Pos2,
    glyph: ContextMenuGlyph,
    color: Color32,
) {
    let stroke = Stroke::new(1.55, color);
    match glyph {
        ContextMenuGlyph::Loop => {
            painter.line_segment(
                [
                    Pos2::new(center.x - 6.0, center.y - 3.5),
                    Pos2::new(center.x + 5.0, center.y - 3.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 2.5, center.y - 6.0),
                    Pos2::new(center.x + 5.5, center.y - 3.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 2.5, center.y - 1.0),
                    Pos2::new(center.x + 5.5, center.y - 3.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 5.0, center.y + 3.5),
                    Pos2::new(center.x + 6.0, center.y + 3.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.5, center.y + 1.0),
                    Pos2::new(center.x - 5.5, center.y + 3.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.5, center.y + 6.0),
                    Pos2::new(center.x - 5.5, center.y + 3.5),
                ],
                stroke,
            );
        }
        ContextMenuGlyph::MiniPlayer => {
            let outer = Rect::from_center_size(center, Vec2::new(15.0, 11.0));
            painter.rect_stroke(
                outer,
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            let pip = Rect::from_min_max(
                Pos2::new(outer.right() - 7.0, outer.bottom() - 5.2),
                Pos2::new(outer.right() - 1.6, outer.bottom() - 1.6),
            );
            painter.rect_filled(pip, 1.0, color);
        }
        ContextMenuGlyph::Link => {
            let left_loop =
                Rect::from_center_size(Pos2::new(center.x - 3.2, center.y), Vec2::new(7.0, 5.2));
            let right_loop =
                Rect::from_center_size(Pos2::new(center.x + 3.2, center.y), Vec2::new(7.0, 5.2));
            painter.rect_stroke(
                left_loop,
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                right_loop,
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.2, center.y),
                    Pos2::new(center.x + 2.2, center.y),
                ],
                stroke,
            );
        }
        ContextMenuGlyph::Embed => {
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.5, center.y - 4.0),
                    Pos2::new(center.x - 6.5, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 6.5, center.y),
                    Pos2::new(center.x - 2.5, center.y + 4.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 2.5, center.y - 4.0),
                    Pos2::new(center.x + 6.5, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 6.5, center.y),
                    Pos2::new(center.x + 2.5, center.y + 4.0),
                ],
                stroke,
            );
        }
        ContextMenuGlyph::Bug => {
            let body =
                Rect::from_center_size(Pos2::new(center.x, center.y + 0.8), Vec2::new(7.0, 8.6));
            painter.rect_stroke(
                body,
                CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.5, body.top() - 1.5),
                    Pos2::new(center.x - 0.8, body.top()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 2.5, body.top() - 1.5),
                    Pos2::new(center.x + 0.8, body.top()),
                ],
                stroke,
            );
            for dy in [-1.5_f32, 1.5, 4.0] {
                painter.line_segment(
                    [
                        Pos2::new(body.left() - 2.5, center.y + dy),
                        Pos2::new(body.left(), center.y + dy),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        Pos2::new(body.right(), center.y + dy),
                        Pos2::new(body.right() + 2.5, center.y + dy),
                    ],
                    stroke,
                );
            }
        }
        ContextMenuGlyph::Help => {
            painter.circle_stroke(center, 7.0, stroke);
            painter.text(
                Pos2::new(center.x, center.y + 0.3),
                egui::Align2::CENTER_CENTER,
                "?",
                FontId::proportional(10.5),
                color,
            );
        }
        ContextMenuGlyph::Info => {
            painter.circle_stroke(center, 7.0, stroke);
            painter.text(
                Pos2::new(center.x, center.y + 0.3),
                egui::Align2::CENTER_CENTER,
                "i",
                FontId::proportional(10.5),
                color,
            );
        }
    }
}

pub fn show_context_menu(
    ctx: &egui::Context,
    anchor_pos: Pos2,
    state: &PlayerContextMenuState,
    style: &OverlayStyle,
) -> (Option<PlayerContextAction>, Rect) {
    let content = ctx.input(|i| i.content_rect());
    let bounds = content.shrink(8.0);
    let menu_w = (bounds.width().max(180.0))
        .clamp(220.0, 316.0)
        .min(bounds.width().max(140.0));
    let row_h = 36.0_f32;
    let pad_y = 6.0_f32;
    let rows_count = 8_usize;
    let menu_h = (rows_count as f32 * row_h + pad_y * 2.0).min(bounds.height().max(row_h));
    let max_x = (bounds.right() - menu_w).max(bounds.left());
    let max_y = (bounds.bottom() - menu_h).max(bounds.top());
    let x = anchor_pos.x.clamp(bounds.left(), max_x);
    let y = anchor_pos.y.clamp(bounds.top(), max_y);
    let menu_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(menu_w, menu_h));

    let pick = |pt_text: &'static str, en_text: &'static str| -> &'static str {
        if state.pt { pt_text } else { en_text }
    };
    let items: [(PlayerContextAction, ContextMenuGlyph, &str, bool, bool); 8] = [
        (
            PlayerContextAction::ToggleLoop,
            ContextMenuGlyph::Loop,
            "Loop",
            !state.is_live,
            state.loop_video,
        ),
        (
            PlayerContextAction::ToggleMiniPlayer,
            ContextMenuGlyph::MiniPlayer,
            "Miniplayer",
            true,
            state.mini_player,
        ),
        (
            PlayerContextAction::CopyVideoUrl,
            ContextMenuGlyph::Link,
            pick("Copiar URL do v\u{00ed}deo", "Copy video URL"),
            true,
            false,
        ),
        (
            PlayerContextAction::CopyVideoUrlAtCurrentTime,
            ContextMenuGlyph::Link,
            pick(
                "Copiar URL do v\u{00ed}deo no momento atual",
                "Copy video URL at current time",
            ),
            true,
            false,
        ),
        (
            PlayerContextAction::CopyEmbedCode,
            ContextMenuGlyph::Embed,
            pick("Copiar c\u{00f3}digo de incorporação", "Copy embed code"),
            state.can_copy_embed,
            false,
        ),
        (
            PlayerContextAction::CopyDebugInfo,
            ContextMenuGlyph::Bug,
            pick(
                "Copiar informa\u{00e7}\u{00f5}es de depura\u{00e7}\u{00e3}o",
                "Copy debug info",
            ),
            true,
            false,
        ),
        (
            PlayerContextAction::TroubleshootPlayback,
            ContextMenuGlyph::Help,
            pick(
                "Resolver problemas de reprodu\u{00e7}\u{00e3}o",
                "Troubleshoot playback issue",
            ),
            true,
            false,
        ),
        (
            PlayerContextAction::ToggleStatsForNerds,
            ContextMenuGlyph::Info,
            pick("Estat\u{00ed}sticas para nerds", "Stats for nerds"),
            true,
            state.stats_for_nerds_open,
        ),
    ];

    let mut chosen: Option<PlayerContextAction> = None;
    let mut rendered_rect = menu_rect;
    egui::Area::new(egui::Id::new("fasttube_player_context_menu"))
        .order(egui::Order::Foreground)
        .movable(false)
        .fade_in(false)
        .default_size(menu_rect.size())
        .fixed_pos(menu_rect.min)
        .show(ctx, |ui| {
            let frame_resp = egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(24, 24, 26, 242))
                .stroke(Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                ))
                .corner_radius(CornerRadius::same(12))
                .shadow(egui::Shadow {
                    offset: [0, 6],
                    blur: 18,
                    spread: 0,
                    color: Color32::from_rgba_unmultiplied(0, 0, 0, 145),
                })
                .inner_margin(egui::Margin::symmetric(6, pad_y as i8))
                .show(ui, |ui| {
                    let inner_w = (menu_w - 12.0).max(120.0);
                    ui.set_min_width(inner_w);
                    ui.set_max_width(inner_w);
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    egui::ScrollArea::vertical()
                        .id_salt("fasttube_player_context_menu_scroll")
                        .max_height((bounds.height() - pad_y * 2.0).max(row_h))
                        .show(ui, |ui| {
                            for (act, glyph, label, enabled, checked) in items {
                                let (row_rect, row_resp) = ui.allocate_exact_size(
                                    Vec2::new(inner_w, row_h),
                                    if enabled {
                                        Sense::click()
                                    } else {
                                        Sense::hover()
                                    },
                                );
                                let row_resp = if enabled {
                                    row_resp.on_hover_cursor(CursorIcon::PointingHand)
                                } else {
                                    row_resp
                                };
                                if ui.is_rect_visible(row_rect) {
                                    let painter = ui.painter();
                                    if enabled && row_resp.hovered() {
                                        painter.rect_filled(
                                            row_rect,
                                            CornerRadius::same(6),
                                            Color32::from_rgba_unmultiplied(255, 255, 255, 24),
                                        );
                                    }
                                    let fg = if enabled {
                                        style.text
                                    } else {
                                        style.text.gamma_multiply(0.4)
                                    };
                                    let icon_center =
                                        Pos2::new(row_rect.left() + 18.0, row_rect.center().y);
                                    paint_context_menu_icon(painter, icon_center, glyph, fg);
                                    let text_right_limit = if checked {
                                        row_rect.right() - 26.0
                                    } else {
                                        row_rect.right() - 8.0
                                    };
                                    let text_rect = Rect::from_min_max(
                                        Pos2::new(row_rect.left() + 36.0, row_rect.top()),
                                        Pos2::new(
                                            text_right_limit.max(row_rect.left() + 40.0),
                                            row_rect.bottom(),
                                        ),
                                    );
                                    painter.with_clip_rect(text_rect).text(
                                        Pos2::new(text_rect.left(), row_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        label,
                                        FontId::proportional(13.0),
                                        fg,
                                    );
                                    if checked {
                                        painter.text(
                                            Pos2::new(row_rect.right() - 10.0, row_rect.center().y),
                                            egui::Align2::RIGHT_CENTER,
                                            "\u{2713}",
                                            FontId::proportional(13.5),
                                            style.accent,
                                        );
                                    }
                                }
                                if enabled && row_resp.clicked() {
                                    chosen = Some(act);
                                }
                            }
                        });
                });
            rendered_rect = frame_resp.response.rect;
        });

    (chosen, rendered_rect)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsForNerdsData {
    pub pt: bool,
    pub video_id: String,
    pub viewport_and_frames: String,
    pub current_res: String,
    pub volume_and_speed: String,
    pub codecs: String,
    pub decoding: String,
    pub buffer_health: String,
    pub bitrate: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsForNerdsAction {
    CopyDebugInfo,
    Close,
}

pub fn show_stats_for_nerds(
    ui: &mut egui::Ui,
    video_rect: Rect,
    data: &StatsForNerdsData,
    style: &OverlayStyle,
) -> (Option<StatsForNerdsAction>, bool) {
    if video_rect.width() < 160.0 || video_rect.height() < 110.0 {
        return (None, false);
    }
    let pt = data.pt;
    let pick = |pt_text: &'static str, en_text: &'static str| -> &'static str {
        if pt { pt_text } else { en_text }
    };
    let card_w = (video_rect.width() - 24.0).clamp(150.0, 372.0);
    let max_h = (video_rect.height() - 24.0).max(90.0);
    let mut rows: Vec<(&'static str, &str)> = vec![
        ("Video ID", &data.video_id),
        (
            pick("Viewport / Quadros", "Viewport / Frames"),
            &data.viewport_and_frames,
        ),
        (
            pick("Resolu\u{00e7}\u{00e3}o / FPS", "Current / Optimal Res"),
            &data.current_res,
        ),
        (
            pick("Volume / Velocidade", "Volume / Speed"),
            &data.volume_and_speed,
        ),
        ("Codecs", &data.codecs),
        (
            pick("Decodifica\u{00e7}\u{00e3}o", "Decoding"),
            &data.decoding,
        ),
        ("Buffer Health", &data.buffer_health),
    ];
    if let Some(bitrate) = data.bitrate.as_deref() {
        rows.push((pick("Taxa de bits", "Bitrate"), bitrate));
    }

    let header_h = 26.0_f32;
    let row_h = 18.0_f32;
    let pad = 10.0_f32;
    let card_h = (pad * 2.0 + header_h + 6.0 + rows.len() as f32 * row_h).min(max_h);
    let card_rect = Rect::from_min_size(
        Pos2::new(video_rect.left() + 12.0, video_rect.top() + 12.0),
        Vec2::new(card_w, card_h),
    );
    let pointer_inside = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| card_rect.contains(p));

    let mut action: Option<StatsForNerdsAction> = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(card_rect), |ui| {
        ui.set_clip_rect(card_rect.intersect(video_rect));
        let bg_id = ui.make_persistent_id("fasttube_stats_for_nerds_bg");
        let _ = ui.interact(card_rect, bg_id, Sense::click());

        let painter = ui.painter();
        painter.rect_filled(
            card_rect,
            CornerRadius::same(8),
            Color32::from_rgba_unmultiplied(15, 15, 18, 224),
        );
        painter.rect_stroke(
            card_rect,
            CornerRadius::same(8),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
            egui::StrokeKind::Inside,
        );

        let inner = card_rect.shrink(pad);
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(pick("Estat\u{00ed}sticas para nerds", "Stats for nerds"))
                        .size(12.5)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close_btn = egui::Button::new(
                        RichText::new("\u{2715}").size(11.5).color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 22))
                    .corner_radius(5)
                    .min_size(Vec2::new(22.0, 20.0));
                    if ui
                        .add(close_btn)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        action = Some(StatsForNerdsAction::Close);
                    }
                    let copy_btn = egui::Button::new(
                        RichText::new(pick("Copiar", "Copy"))
                            .size(10.5)
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 22))
                    .corner_radius(5)
                    .min_size(Vec2::new(48.0, 20.0));
                    if ui
                        .add(copy_btn)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                        && action.is_none()
                    {
                        action = Some(StatsForNerdsAction::CopyDebugInfo);
                    }
                });
            });
            ui.add_space(4.0);
            let label_w = (inner.width() * 0.40).clamp(84.0, 140.0);
            for (label, value) in rows {
                let (r, _) = ui.allocate_exact_size(Vec2::new(inner.width(), 16.0), Sense::hover());
                let p = ui.painter();
                let label_rect = Rect::from_min_max(
                    r.min,
                    Pos2::new((r.min.x + label_w - 6.0).max(r.min.x + 20.0), r.max.y),
                );
                let value_rect = Rect::from_min_max(Pos2::new(r.min.x + label_w, r.min.y), r.max);
                p.with_clip_rect(label_rect).text(
                    Pos2::new(label_rect.left(), r.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(11.0),
                    style.text.gamma_multiply(0.72),
                );
                p.with_clip_rect(value_rect).text(
                    Pos2::new(value_rect.left(), r.center().y),
                    egui::Align2::LEFT_CENTER,
                    value,
                    FontId::proportional(11.0),
                    Color32::WHITE,
                );
            }
        });
    });

    (action, pointer_inside)
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

            if page == OverlayMenu::Root {
                for expected in [
                    "Reprodução automática",
                    "Repetir vídeo",
                    "Velocidade",
                    "Qualidade",
                    "Legendas",
                    "Faixa de áudio",
                    "Imagem e equalizador",
                    "Capítulos",
                    "Configurações do app...",
                ] {
                    assert!(
                        texts.iter().any(|t| t == expected),
                        "expected root menu item {expected:?} in {texts:?}"
                    );
                }
                for removed in [
                    "Loop A-B",
                    "Definir A",
                    "Definir B",
                    "Modo somente áudio",
                    "Capturar quadro",
                    "Proporção e zoom",
                    "Adicionar marcador",
                ] {
                    assert!(
                        !texts.iter().any(|t| t == removed),
                        "removed root menu item {removed:?} should not appear in {texts:?}"
                    );
                }
            }
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
        // 0: Fullscreen, 1: Theater, 2: Mini, 3: Settings, 4: Captions, 5: Autoplay, 6: AudioOnly
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
            (None, true)
        );
    }

    #[test]
    fn hover_preview_time_secs_tracks_timeline_fraction() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let bar_rect = compute_bar_rect(rect).expect("bar_rect");
        let track_left = bar_rect.min.x + BAR_SIDE_PADDING;
        let track_right = bar_rect.max.x - BAR_SIDE_PADDING;
        let mid_pos = Pos2::new((track_left + track_right) * 0.5, bar_rect.min.y + 5.0);

        let t = hover_preview_time_secs(rect, Some(200.0), Some(mid_pos)).expect("hover secs");
        assert!((t - 100.0).abs() < 1.5, "got {t}");

        assert_eq!(
            hover_preview_time_secs(rect, Some(200.0), Some(Pos2::new(10.0, 10.0))),
            None
        );
        assert_eq!(hover_preview_time_secs(rect, None, Some(mid_pos)), None);
    }

    #[test]
    fn storyboard_hover_pill_renders_image_and_new_menus_paint_cleanly() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let style = sample_style();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect");
        let ctx = egui::Context::default();
        let hover_pos = Pos2::new(bar_rect.center().x, bar_rect.min.y + 6.0);

        let telemetry = OverlayTelemetry {
            duration_secs: Some(300.0),
            storyboard_preview: Some(OverlayStoryboardFrame {
                texture_id: egui::TextureId::Managed(1),
                uv: Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(0.2, 0.2)),
                aspect_ratio: 16.0 / 9.0,
            }),
            audio_tracks: vec![
                OverlayAudioTrack {
                    id: "1".into(),
                    label: "Português".into(),
                    selected: true,
                },
                OverlayAudioTrack {
                    id: "2".into(),
                    label: "English".into(),
                    selected: false,
                },
            ],
            menu: Some(OverlayMenu::AudioTracks),
            ..OverlayTelemetry::default()
        };

        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::PointerMoved(hover_pos));
        let mut has_mesh = false;
        let mut out = ctx.run_ui(raw, |ui| {
            let _ = show(ui, rect, &telemetry, &style, true);
        });
        for clipped in &out.shapes {
            if matches!(clipped.shape, Shape::Mesh(_)) {
                has_mesh = true;
            }
        }
        out.textures_delta.clear();
        assert!(
            has_mesh,
            "expected storyboard preview mesh/image to be painted"
        );

        for menu in [OverlayMenu::AudioTracks, OverlayMenu::VideoAudioEq] {
            let t_menu = OverlayTelemetry {
                menu: Some(menu),
                pt: false,
                ..telemetry.clone()
            };
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let _ = show(ui, rect, &t_menu, &style, true);
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn should_keep_overlay_visible_for_pointer_differs_between_fullscreen_and_windowed() {
        let video_rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let bar_rect = compute_bar_rect(video_rect).expect("bar_rect");
        let on_bar = Pos2::new(bar_rect.center().x, bar_rect.center().y);
        let on_canvas = Pos2::new(video_rect.center().x, video_rect.center().y);

        assert!(!should_keep_overlay_visible_for_pointer(
            true,
            Some(on_bar),
            video_rect
        ));
        assert!(!should_keep_overlay_visible_for_pointer(
            true,
            Some(on_canvas),
            video_rect
        ));
        assert!(should_keep_overlay_visible_for_pointer(
            false,
            Some(on_bar),
            video_rect
        ));
        assert!(!should_keep_overlay_visible_for_pointer(
            false,
            Some(on_canvas),
            video_rect
        ));
        assert!(!should_keep_overlay_visible_for_pointer(
            false, None, video_rect
        ));
    }

    #[test]
    fn context_menu_and_stats_for_nerds_render_and_emit_expected_actions() {
        let ctx = egui::Context::default();
        let style = sample_style();
        let screen = Rect::from_min_max(Pos2::ZERO, Pos2::new(960.0, 540.0));
        let anchor = Pos2::new(220.0, 120.0);

        for (pt, expected_items) in [
            (
                true,
                [
                    "Loop",
                    "Miniplayer",
                    "Copiar URL do vídeo",
                    "Copiar URL do vídeo no momento atual",
                    "Copiar código de incorporação",
                    "Copiar informações de depuração",
                    "Resolver problemas de reprodução",
                    "Estatísticas para nerds",
                ],
            ),
            (
                false,
                [
                    "Loop",
                    "Miniplayer",
                    "Copy video URL",
                    "Copy video URL at current time",
                    "Copy embed code",
                    "Copy debug info",
                    "Troubleshoot playback issue",
                    "Stats for nerds",
                ],
            ),
        ] {
            let menu_state = PlayerContextMenuState {
                pt,
                is_live: false,
                loop_video: true,
                mini_player: false,
                can_copy_embed: true,
                stats_for_nerds_open: false,
            };
            let mut menu_rect = Rect::NOTHING;
            let mut texts = Vec::new();
            let mut warm_out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    let _ = show_context_menu(ui.ctx(), anchor, &menu_state, &style);
                },
            );
            warm_out.textures_delta.clear();
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    let (_, rect) = show_context_menu(ui.ctx(), anchor, &menu_state, &style);
                    menu_rect = rect;
                },
            );
            for clipped in &out.shapes {
                if let Shape::Text(text) = &clipped.shape {
                    texts.push(text.galley.text().to_owned());
                } else if let Shape::Vec(items) = &clipped.shape {
                    for s in items {
                        if let Shape::Text(text) = s {
                            texts.push(text.galley.text().to_owned());
                        }
                    }
                }
            }
            out.textures_delta.clear();
            for item in expected_items {
                assert!(
                    texts.iter().any(|t| t == item),
                    "pt={pt}: missing menu item {item:?} in {texts:?}"
                );
            }
            assert!(menu_rect.height() > 200.0);
        }

        // Click on last row ("Stats for nerds" / row index 7).
        let pt_state = PlayerContextMenuState {
            pt: true,
            is_live: false,
            loop_video: false,
            mini_player: false,
            can_copy_embed: true,
            stats_for_nerds_open: false,
        };
        let mut last_row_pos = Pos2::new(anchor.x + 80.0, anchor.y + 6.0 + 7.5 * 36.0);
        let mut warm = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(last_row_pos)],
                ..Default::default()
            },
            |ui| {
                let (_, rect) = show_context_menu(ui.ctx(), anchor, &pt_state, &style);
                last_row_pos = Pos2::new(rect.center().x, rect.bottom() - 24.0);
            },
        );
        warm.textures_delta.clear();

        let mut picked_action = None;
        for pressed in [true, false] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        egui::Event::PointerMoved(last_row_pos),
                        egui::Event::PointerButton {
                            pos: last_row_pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    let (act, _) = show_context_menu(ui.ctx(), anchor, &pt_state, &style);
                    if act.is_some() {
                        picked_action = act;
                    }
                },
            );
            out.textures_delta.clear();
        }
        assert_eq!(
            picked_action,
            Some(PlayerContextAction::ToggleStatsForNerds)
        );

        let stats_data = StatsForNerdsData {
            pt: true,
            video_id: "dQw4w9WgXcQ".to_owned(),
            viewport_and_frames: "1280x720 / 0 descartados".to_owned(),
            current_res: "1920x1080 @ 60 fps".to_owned(),
            volume_and_speed: "100% / 1.00x".to_owned(),
            codecs: "av1 / opus".to_owned(),
            decoding: "hardware ativo (d3d11va)".to_owned(),
            buffer_health: "12.5s (Padrão)".to_owned(),
            bitrate: Some("2.4 Mbps".to_owned()),
        };
        let mut stats_texts = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let _ = show_stats_for_nerds(ui, screen, &stats_data, &style);
            },
        );
        for clipped in &out.shapes {
            if let Shape::Text(text) = &clipped.shape {
                stats_texts.push(text.galley.text().to_owned());
            } else if let Shape::Vec(items) = &clipped.shape {
                for s in items {
                    if let Shape::Text(text) = s {
                        stats_texts.push(text.galley.text().to_owned());
                    }
                }
            }
        }
        out.textures_delta.clear();
        assert!(stats_texts.iter().any(|t| t == "Estatísticas para nerds"));
        assert!(stats_texts.iter().any(|t| t == "dQw4w9WgXcQ"));
        assert!(stats_texts.iter().any(|t| t == "av1 / opus"));
    }

    #[test]
    fn overlay_renders_previous_button_when_has_previous_is_true() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let style = sample_style();
        let telemetry = OverlayTelemetry {
            paused: true,
            has_previous: true,
            has_queued: true,
            duration_secs: Some(120.0),
            ..OverlayTelemetry::default()
        };
        let ctx = egui::Context::default();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect");
        // Left row order: Play(32) + spacing(4) + Previous(30) + spacing(4) + Next(30)
        let prev_pos = Pos2::new(
            bar_rect.min.x + 12.0 + 32.0 + 4.0 + 15.0,
            bar_rect.min.y + 36.0,
        );
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, prev_pos),
            (Some(OverlayAction::Previous), true)
        );
        let next_pos = Pos2::new(prev_pos.x + 34.0, prev_pos.y);
        assert_eq!(
            click_at(&ctx, rect, &telemetry, &style, next_pos),
            (Some(OverlayAction::Next), true)
        );
    }

    #[test]
    fn hover_preview_time_secs_retains_scrub_time_when_dragging_outside_strip() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(720.0, 405.0));
        let bar_rect = compute_bar_rect(rect).expect("bar_rect");
        let track_left = bar_rect.min.x + BAR_SIDE_PADDING;
        let track_right = bar_rect.max.x - BAR_SIDE_PADDING;
        let track_w = track_right - track_left;

        let above_strip = Pos2::new(track_left + track_w * 0.75, bar_rect.min.y - 80.0);
        let below_strip = Pos2::new(track_left + track_w * 0.25, bar_rect.max.y + 40.0);
        let past_right = Pos2::new(track_right + 50.0, bar_rect.min.y - 60.0);
        let past_left = Pos2::new(track_left - 50.0, bar_rect.min.y - 60.0);

        // Hovering outside the 16px strip without dragging yields None.
        assert_eq!(
            hover_preview_time_secs_with_drag(rect, Some(200.0), Some(above_strip), false),
            None
        );
        assert_eq!(
            hover_preview_time_secs_with_drag(rect, Some(200.0), Some(below_strip), false),
            None
        );

        // While dragging, vertical drift and horizontal overshoot stay active and clamped.
        let t_above = hover_preview_time_secs_with_drag(rect, Some(200.0), Some(above_strip), true)
            .expect("dragged scrub time above strip");
        assert!((t_above - 150.0).abs() < 1.0, "got {t_above}");

        let t_below = hover_preview_time_secs_with_drag(rect, Some(200.0), Some(below_strip), true)
            .expect("dragged scrub time below strip");
        assert!((t_below - 50.0).abs() < 1.0, "got {t_below}");

        let t_right = hover_preview_time_secs_with_drag(rect, Some(200.0), Some(past_right), true)
            .expect("dragged scrub time past right");
        assert!((t_right - 200.0).abs() < 1e-6, "got {t_right}");

        let t_left = hover_preview_time_secs_with_drag(rect, Some(200.0), Some(past_left), true)
            .expect("dragged scrub time past left");
        assert!(t_left.abs() < 1e-6, "got {t_left}");
    }

    #[test]
    fn progress_bar_drag_retains_storyboard_preview_when_pointer_drifts_vertically() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let style = sample_style();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect");
        let side_pad = BAR_SIDE_PADDING.min((bar_rect.width() * 0.04).max(4.0));
        let track_left = bar_rect.min.x + side_pad;
        let track_right = bar_rect.max.x - side_pad;
        let track_w = track_right - track_left;

        let start_pos = Pos2::new(track_left + track_w * 0.25, bar_rect.min.y + 6.0);
        let drifted_pos = Pos2::new(track_left + track_w * 0.75, bar_rect.min.y - 120.0);

        let telemetry = OverlayTelemetry {
            duration_secs: Some(200.0),
            storyboard_preview: Some(OverlayStoryboardFrame {
                texture_id: egui::TextureId::Managed(1),
                uv: Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(0.2, 0.2)),
                aspect_ratio: 16.0 / 9.0,
            }),
            ..OverlayTelemetry::default()
        };

        let ctx = egui::Context::default();
        set_active_progress_drag_rect(None);

        // Frame 0: warm up layout and hover over the progress strip.
        let mut out0 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![egui::Event::PointerMoved(start_pos)],
                ..Default::default()
            },
            |ui| {
                let _ = show(ui, rect, &telemetry, &style, true);
            },
        );
        out0.textures_delta.clear();

        // Frame 1: begin pressing on the progress strip.
        let mut out1 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![
                    egui::Event::PointerMoved(start_pos),
                    egui::Event::PointerButton {
                        pos: start_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| {
                let _ = show(ui, rect, &telemetry, &style, true);
            },
        );
        out1.textures_delta.clear();

        // Frame 2: pointer drifts 120px vertically above the 16px strip while dragging.
        let preview_secs = hover_preview_time_secs(rect, Some(200.0), Some(drifted_pos)).expect(
            "hover_preview_time_secs must remain active while dragging vertically off strip",
        );
        assert!((preview_secs - 150.0).abs() < 1.5, "got {preview_secs}");

        let mut texts = Vec::new();
        let mut has_mesh = false;
        let mut out2 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![egui::Event::PointerMoved(drifted_pos)],
                ..Default::default()
            },
            |ui| {
                let _ = show(ui, rect, &telemetry, &style, true);
            },
        );
        for clipped in &out2.shapes {
            match &clipped.shape {
                Shape::Mesh(_) => has_mesh = true,
                Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                Shape::Vec(vec) => {
                    for s in vec {
                        match s {
                            Shape::Mesh(_) => has_mesh = true,
                            Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        out2.textures_delta.clear();
        assert!(
            has_mesh,
            "storyboard preview mesh must stay rendered while dragging vertically off strip"
        );
        assert!(
            texts.iter().any(|t| t == "2:30"),
            "expected dragged preview timestamp 2:30 in {texts:?}"
        );

        // Frame 3: release pointer and confirm SeekTo is emitted and drag state clears.
        let mut release_action = None;
        let mut out3 = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![
                    egui::Event::PointerMoved(drifted_pos),
                    egui::Event::PointerButton {
                        pos: drifted_pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| {
                release_action = show(ui, rect, &telemetry, &style, true).0;
            },
        );
        out3.textures_delta.clear();
        match release_action {
            Some(OverlayAction::SeekTo(secs)) => {
                assert!((secs - 150.0).abs() < 1.5, "unexpected seek target {secs}");
            }
            other => panic!("expected SeekTo(~150.0) on drag release, got {other:?}"),
        }
        assert_eq!(
            hover_preview_time_secs(rect, Some(200.0), Some(drifted_pos)),
            None
        );
    }

    #[test]
    fn sample_heatmap_intensity_interpolates_across_boundaries_and_guards_empty_or_zero_duration() {
        let points = [
            HeatmapPoint {
                start_secs: 0.0,
                duration_secs: 10.0,
                intensity: 0.2,
            },
            HeatmapPoint {
                start_secs: 10.0,
                duration_secs: 10.0,
                intensity: 0.8,
            },
            HeatmapPoint {
                start_secs: 20.0,
                duration_secs: 10.0,
                intensity: 0.4,
            },
        ];

        assert_eq!(sample_heatmap_intensity(&[], 30.0, 10.0), 0.0);
        assert_eq!(sample_heatmap_intensity(&points, 0.0, 10.0), 0.0);
        assert_eq!(sample_heatmap_intensity(&points, -5.0, 10.0), 0.0);
        assert_eq!(sample_heatmap_intensity(&points, f64::NAN, 10.0), 0.0);
        assert_eq!(sample_heatmap_intensity(&points, 30.0, f64::NAN), 0.0);

        // Segment centers match exact normalized intensities.
        assert!((sample_heatmap_intensity(&points, 30.0, 0.0) - 0.2).abs() < 1e-5);
        assert!((sample_heatmap_intensity(&points, 30.0, 5.0) - 0.2).abs() < 1e-5);
        assert!((sample_heatmap_intensity(&points, 30.0, 15.0) - 0.8).abs() < 1e-5);
        assert!((sample_heatmap_intensity(&points, 30.0, 25.0) - 0.4).abs() < 1e-5);
        assert!((sample_heatmap_intensity(&points, 30.0, 30.0) - 0.4).abs() < 1e-5);

        // Boundary at t=10.0s sits smoothly between 0.2 and 0.8.
        let boundary_01 = sample_heatmap_intensity(&points, 30.0, 10.0);
        assert!(
            boundary_01 > 0.45 && boundary_01 < 0.65,
            "expected smooth boundary transition around ~0.5, got {boundary_01}"
        );

        // Disjoint markers return 0.0 in uncovered gaps and outside marker bounds.
        let disjoint = [
            HeatmapPoint {
                start_secs: 20.0,
                duration_secs: 10.0,
                intensity: 0.9,
            },
            HeatmapPoint {
                start_secs: 50.0,
                duration_secs: 10.0,
                intensity: 0.6,
            },
        ];
        assert_eq!(sample_heatmap_intensity(&disjoint, 100.0, 5.0), 0.0);
        assert!((sample_heatmap_intensity(&disjoint, 100.0, 25.0) - 0.9).abs() < 1e-5);
        assert_eq!(sample_heatmap_intensity(&disjoint, 100.0, 35.0), 0.0);
        assert!((sample_heatmap_intensity(&disjoint, 100.0, 55.0) - 0.6).abs() < 1e-5);
        assert_eq!(sample_heatmap_intensity(&disjoint, 100.0, 80.0), 0.0);
    }

    #[test]
    fn heatmap_peak_detection_and_most_replayed_hover_badge_renders_bilingually() {
        let heatmap = [
            HeatmapPoint {
                start_secs: 0.0,
                duration_secs: 50.0,
                intensity: 0.25,
            },
            HeatmapPoint {
                start_secs: 50.0,
                duration_secs: 50.0,
                intensity: 0.96,
            },
            HeatmapPoint {
                start_secs: 100.0,
                duration_secs: 50.0,
                intensity: 0.30,
            },
            HeatmapPoint {
                start_secs: 150.0,
                duration_secs: 50.0,
                intensity: 0.15,
            },
        ];

        assert!(!is_heatmap_most_replayed(&heatmap, 200.0, 25.0));
        assert!(is_heatmap_most_replayed(&heatmap, 200.0, 75.0));
        assert!(!is_heatmap_most_replayed(&heatmap, 200.0, 175.0));
        assert_eq!(
            heatmap_peak_point(&heatmap),
            Some(HeatmapPoint {
                start_secs: 50.0,
                duration_secs: 50.0,
                intensity: 0.96,
            })
        );

        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let style = sample_style();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect");
        let side_pad = BAR_SIDE_PADDING.min((bar_rect.width() * 0.04).max(4.0));
        let track_left = bar_rect.min.x + side_pad;
        let track_right = bar_rect.max.x - side_pad;
        let track_w = track_right - track_left;

        // Hover at 75.0s / 200.0s (fraction 0.375, center of the 0.96 peak marker).
        let peak_hover_pos = Pos2::new(track_left + track_w * 0.375, bar_rect.min.y + 6.0);
        // Hover at 175.0s / 200.0s (fraction 0.875, low-intensity 0.15 marker).
        let low_hover_pos = Pos2::new(track_left + track_w * 0.875, bar_rect.min.y + 6.0);

        for (pt, expected_badge) in [(true, "Mais assistido"), (false, "Most replayed")] {
            let ctx = egui::Context::default();
            let telemetry = OverlayTelemetry {
                pt,
                duration_secs: Some(200.0),
                position_secs: 10.0,
                ..OverlayTelemetry::default()
            };

            // Warm up one pass so egui registers the progress strip rect for hover hit-testing.
            let mut warmup = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events: vec![egui::Event::PointerMoved(peak_hover_pos)],
                    ..Default::default()
                },
                |ui| {
                    let _ = show_with_heatmap(ui, rect, &telemetry, &heatmap, &style, true);
                },
            );
            warmup.textures_delta.clear();

            let mut texts = Vec::new();
            let mut has_mesh = false;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events: vec![egui::Event::PointerMoved(peak_hover_pos)],
                    ..Default::default()
                },
                |ui| {
                    let _ = show_with_heatmap(ui, rect, &telemetry, &heatmap, &style, true);
                },
            );
            for clipped in &out.shapes {
                match &clipped.shape {
                    Shape::Mesh(_) => has_mesh = true,
                    Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                    Shape::Vec(vec) => {
                        for s in vec {
                            match s {
                                Shape::Mesh(_) => has_mesh = true,
                                Shape::Text(t) => texts.push(t.galley.text().to_owned()),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            out.textures_delta.clear();
            assert!(has_mesh, "expected heatmap area mesh to be painted");
            assert!(
                texts.iter().any(|t| t == expected_badge),
                "pt={pt}: expected {expected_badge:?} badge in {texts:?}"
            );

            // Hovering over a low-intensity segment must not display the Most replayed badge.
            let mut low_texts = Vec::new();
            let mut out_low = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events: vec![egui::Event::PointerMoved(low_hover_pos)],
                    ..Default::default()
                },
                |ui| {
                    let _ = show_with_heatmap(ui, rect, &telemetry, &heatmap, &style, true);
                },
            );
            for clipped in &out_low.shapes {
                match &clipped.shape {
                    Shape::Text(t) => low_texts.push(t.galley.text().to_owned()),
                    Shape::Vec(vec) => {
                        for s in vec {
                            if let Shape::Text(t) = s {
                                low_texts.push(t.galley.text().to_owned());
                            }
                        }
                    }
                    _ => {}
                }
            }
            out_low.textures_delta.clear();
            assert!(
                !low_texts.iter().any(|t| t == expected_badge),
                "pt={pt}: unexpected {expected_badge:?} badge at low intensity in {low_texts:?}"
            );
        }
    }

    #[test]
    fn clickable_remaining_time_format_and_toggle_action() {
        assert_eq!(
            format_time_display(83.0, Some(600.0), false),
            "1:23 / 10:00"
        );
        assert_eq!(
            format_time_display(83.0, Some(600.0), true),
            "-8:37 / 10:00"
        );
        assert_eq!(
            format_time_display(620.0, Some(600.0), true),
            "-0:00 / 10:00"
        );
        assert_eq!(format_time_display(45.0, None, true), "0:45 / --:--");

        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let style = sample_style();
        let telemetry = OverlayTelemetry {
            position_secs: 83.0,
            duration_secs: Some(600.0),
            show_remaining_time: true,
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
            texts.iter().any(|t| t == "-8:37 / 10:00"),
            "expected remaining time readout in {texts:?}"
        );
    }

    #[test]
    fn continuous_speed_clamp_bounds_and_step_snapping() {
        assert_eq!(clamp_playback_speed(f64::NAN), 1.0);
        assert_eq!(clamp_playback_speed(0.0), 0.05);
        assert_eq!(clamp_playback_speed(0.05), 0.05);
        assert!((clamp_playback_speed(1.37) - 1.35).abs() < 1e-6);
        assert!((clamp_playback_speed(1.38) - 1.40).abs() < 1e-6);
        assert_eq!(clamp_playback_speed(4.0), 4.0);
        assert_eq!(clamp_playback_speed(9.5), 4.0);
    }

    #[test]
    fn up_next_overlay_and_sleep_timer_helpers_and_rendering() {
        let up_next = UpNextOverlayInfo {
            title: "Next Great Video".to_owned(),
            channel: "FastTube Channel".to_owned(),
            thumbnail_url: None,
            remaining_secs: 4.0,
            total_secs: 8.0,
        };
        assert!((up_next.progress_fraction() - 0.5).abs() < 1e-5);
        let zero_total = UpNextOverlayInfo {
            total_secs: 0.0,
            ..up_next.clone()
        };
        assert_eq!(zero_total.progress_fraction(), 0.0);

        assert_eq!(format_sleep_timer_summary(None, None, true), "Desativado");
        assert_eq!(format_sleep_timer_summary(None, None, false), "Off");
        assert_eq!(
            format_sleep_timer_summary(Some(SleepTimerOption::Minutes(15)), Some(890), false),
            "14:50"
        );
        assert_eq!(
            format_sleep_timer_summary(Some(SleepTimerOption::EndOfVideo), None, true),
            "Fim do vídeo"
        );
        assert_eq!(SleepTimerOption::Minutes(30).duration_secs(), Some(1800));
        assert_eq!(SleepTimerOption::EndOfVideo.duration_secs(), None);

        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let style = sample_style();
        let telemetry = OverlayTelemetry {
            pt: false,
            ended: true,
            up_next: Some(up_next),
            ..OverlayTelemetry::default()
        };
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let _ = show(ui, rect, &telemetry, &style, false);
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
            texts.iter().any(|t| t == "Play now"),
            "missing Play now in {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Cancel"),
            "missing Cancel in {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Next Great Video"),
            "missing Up Next title in {texts:?}"
        );
    }

    #[test]
    fn temp_2x_pill_seek_ripple_and_new_submenus_render_cleanly() {
        assert_eq!(format_seek_ripple_label(true, 10), "+10s");
        assert_eq!(format_seek_ripple_label(false, 10), "-10s");
        assert_eq!(format_seek_ripple_label(true, 15), "+15s");

        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let style = sample_style();
        let ctx = egui::Context::default();
        for menu in [
            None,
            Some(OverlayMenu::SleepTimer),
            Some(OverlayMenu::UltrawideZoom),
            Some(OverlayMenu::Subtitles),
            Some(OverlayMenu::Speed),
        ] {
            let telemetry = OverlayTelemetry {
                pt: false,
                volume_percent: 165.0,
                temp_speed_2x_active: true,
                seek_ripple: Some((true, 0.35)),
                sleep_timer_option: Some(SleepTimerOption::Minutes(15)),
                sleep_timer_remaining_secs: Some(420),
                menu,
                ..OverlayTelemetry::default()
            };
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
            if menu.is_none() {
                assert!(
                    texts.iter().any(|t| t == "2\u{00d7} >>"),
                    "missing 2x pill in {texts:?}"
                );
                assert!(
                    texts.iter().any(|t| t == "+10s"),
                    "missing +10s ripple in {texts:?}"
                );
            }
        }
    }

    #[test]
    fn volume_wheel_hit_state_targets_only_volume_area() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 450.0));
        let bar_rect = compute_bar_rect(rect).expect("bar_rect");
        let style = sample_style();
        let telemetry = OverlayTelemetry {
            duration_secs: Some(180.0),
            position_secs: 42.0,
            volume_percent: 75.0,
            ..OverlayTelemetry::default()
        };
        let row_cy = bar_rect.min.y + PROGRESS_HIT_HEIGHT + 2.0 + BUTTON_ROW_HEIGHT * 0.5;
        let ctx = egui::Context::default();

        let probe_hit = |telemetry: &OverlayTelemetry, visible: bool, pos: Pos2| {
            // Two frames so egui widget hover state and hit-rects settle on `pos`.
            let mut hit = OverlayHitState::default();
            for _ in 0..2 {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(rect),
                        events: vec![egui::Event::PointerMoved(pos)],
                        ..Default::default()
                    },
                    |ui| {
                        hit =
                            show_with_heatmap_hit_state(ui, rect, telemetry, &[], &style, visible)
                                .1;
                    },
                );
                out.textures_delta.clear();
            }
            hit
        };

        // Warm up once to locate the rendered timestamp rect so coordinates derive from layout.
        let mut warm_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                let _ = show_with_heatmap_hit_state(ui, rect, &telemetry, &[], &style, true);
            },
        );
        let mut time_rect = None;
        for clipped in &warm_out.shapes {
            match &clipped.shape {
                Shape::Text(t) if t.galley.text() == "0:42 / 3:00" => {
                    time_rect = Some(clipped.shape.visual_bounding_rect());
                }
                Shape::Vec(vec) => {
                    for s in vec {
                        if let Shape::Text(t) = s
                            && t.galley.text() == "0:42 / 3:00"
                        {
                            time_rect = Some(s.visual_bounding_rect());
                        }
                    }
                }
                _ => {}
            }
        }
        warm_out.textures_delta.clear();
        let time_rect = time_rect.expect("rendered time readout rect");

        let slider_pos = Pos2::new(time_rect.left() - 36.0, row_cy);
        let gap_pos = Pos2::new(time_rect.left() - 74.0, row_cy);
        let mute_pos = Pos2::new(time_rect.left() - 88.0, row_cy);

        for pos in [mute_pos, gap_pos, slider_pos] {
            let hit = probe_hit(&telemetry, true, pos);
            assert_eq!(
                hit,
                OverlayHitState {
                    over_controls: true,
                    over_volume: true,
                },
                "expected volume hit at {pos:?}"
            );
        }

        let non_volume_controls = [
            ("play", Pos2::new(bar_rect.left() + 26.0, row_cy)),
            (
                "progress",
                Pos2::new(bar_rect.center().x, bar_rect.min.y + 8.0),
            ),
            ("timestamp", time_rect.center()),
            ("empty_bar", Pos2::new(bar_rect.center().x, row_cy)),
            ("fullscreen", Pos2::new(bar_rect.right() - 28.0, row_cy)),
        ];
        for (label, pos) in non_volume_controls {
            let hit = probe_hit(&telemetry, true, pos);
            assert_eq!(
                hit,
                OverlayHitState {
                    over_controls: true,
                    over_volume: false,
                },
                "expected over_controls=true, over_volume=false for {label} at {pos:?}"
            );
        }

        // Video stage outside bar.
        assert_eq!(
            probe_hit(&telemetry, true, Pos2::new(400.0, 200.0)),
            OverlayHitState {
                over_controls: false,
                over_volume: false,
            }
        );

        // Hidden overlay never reports over_volume even at the slider coordinates.
        assert_eq!(
            probe_hit(&telemetry, false, slider_pos),
            OverlayHitState {
                over_controls: false,
                over_volume: false,
            }
        );

        // Open settings popup reports over_controls=true and over_volume=false when hovered.
        let menu_telemetry = OverlayTelemetry {
            menu: Some(OverlayMenu::Root),
            ..telemetry.clone()
        };
        let mut menu_out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                let _ = show_with_heatmap_hit_state(ui, rect, &menu_telemetry, &[], &style, true);
            },
        );
        let mut speed_rect = None;
        for clipped in &menu_out.shapes {
            match &clipped.shape {
                Shape::Text(t) if t.galley.text() == "Velocidade" => {
                    speed_rect = Some(clipped.shape.visual_bounding_rect());
                }
                Shape::Vec(vec) => {
                    for s in vec {
                        if let Shape::Text(t) = s
                            && t.galley.text() == "Velocidade"
                        {
                            speed_rect = Some(s.visual_bounding_rect());
                        }
                    }
                }
                _ => {}
            }
        }
        menu_out.textures_delta.clear();
        let popup_pos = speed_rect.expect("Velocidade menu item rect").center();
        assert_eq!(
            probe_hit(&menu_telemetry, true, popup_pos),
            OverlayHitState {
                over_controls: true,
                over_volume: false,
            }
        );
    }
}
