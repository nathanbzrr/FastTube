mod comments;
mod live_chat_ui;
mod metadata_sidebar;
mod mini_player;
mod player_overlay;
mod player_stage;
mod shorts;
mod transcript_ui;

#[cfg(test)]
mod tests;

use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32};

#[cfg(test)]
pub(crate) use comments::{can_view_watch_comments, is_own_comment};
#[cfg(test)]
pub(crate) use live_chat_ui::{
    LiveChatFilterCacheKey, filter_live_chat_replay_indices, live_chat_visible_range,
    resolve_live_chat_filter_cache,
};
#[cfg(test)]
pub(crate) use metadata_sidebar::{
    format_sidebar_channel_label, is_channel_verified, is_watch_channel_verified,
    sidebar_video_matches_current_channel,
};
#[cfg(test)]
pub(crate) use mini_player::{IN_APP_MINI_PLAYER_ANCHOR_OFFSET, MiniPlayerOverlayLayout};
#[cfg(test)]
pub(crate) use shorts::{
    format_short_compact_count, format_short_stat_date, resolve_short_channel_handle,
    strip_views_word,
};
#[cfg(test)]
pub(crate) use transcript_ui::{TranscriptSearchCacheKey, resolve_transcript_search_cache};

#[cfg(any(windows, test))]
const AUDIO_TRACKS_MENU_POLL_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(any(windows, test))]
const AUDIO_TRACKS_EMPTY_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Decides whether `player.audio_tracks()` should be queried on this frame without per-frame FFI spikes.
#[cfg(any(windows, test))]
pub(crate) fn should_poll_audio_tracks(
    video_changed: bool,
    file_loaded: bool,
    menu: Option<overlay::OverlayMenu>,
    prev_menu: Option<overlay::OverlayMenu>,
    tracks_empty: bool,
    elapsed: Option<Duration>,
) -> bool {
    let audio_menu_open = menu == Some(overlay::OverlayMenu::AudioTracks);
    let root_menu_open = menu == Some(overlay::OverlayMenu::Root);
    let just_opened_audio = audio_menu_open && prev_menu != Some(overlay::OverlayMenu::AudioTracks);
    let just_opened_root = root_menu_open && prev_menu != Some(overlay::OverlayMenu::Root);

    if just_opened_audio || (just_opened_root && tracks_empty) {
        return true;
    }
    if video_changed && file_loaded && (audio_menu_open || root_menu_open) {
        return true;
    }
    if audio_menu_open {
        return elapsed.is_none_or(|d| d >= AUDIO_TRACKS_MENU_POLL_INTERVAL);
    }
    if root_menu_open && tracks_empty {
        return elapsed.is_none_or(|d| d >= AUDIO_TRACKS_EMPTY_POLL_INTERVAL);
    }
    false
}

/// Resolves the "Most Replayed" heatmap points for the Watch overlay, preferring
/// `extras.heatmap` (from InnerTube `next`) and falling back to `live_info.heatmap`
/// converted via `HeatmapMarker::to_overlay_point`, cached per `(live_info_revision, extras.heatmap.len())`
/// so zero per-frame allocation occurs.
pub(crate) fn resolve_watch_heatmap_points<'a>(
    extras: &'a mut watch_extras::WatchExtras,
    live_info: Option<&live_chat::WatchLiveInfo>,
    live_info_revision: u64,
) -> &'a [overlay::HeatmapPoint] {
    let key = (
        live_info_revision,
        extras.heatmap.len(),
        live_info.is_some(),
    );
    if extras.cached_heatmap_key != Some(key) {
        if !extras.heatmap.is_empty() {
            extras.cached_heatmap_points = extras.heatmap.clone();
        } else if let Some(info) = live_info {
            extras.cached_heatmap_points = info
                .heatmap
                .iter()
                .map(live_chat::HeatmapMarker::to_overlay_point)
                .collect();
        } else {
            extras.cached_heatmap_points.clear();
        }
        extras.cached_heatmap_key = Some(key);
    }
    &extras.cached_heatmap_points
}

/// Maps available multi-language/dubbing audio tracks from a cached `StreamManifest`
/// into `OverlayAudioTrack` entries, marking the active preferred or default track.
pub(crate) fn manifest_overlay_audio_tracks(
    manifest: &stream_manifest::StreamManifest,
    preferred_audio_lang: Option<&str>,
) -> Vec<overlay::OverlayAudioTrack> {
    let opts = stream_manifest::available_audio_tracks(manifest);
    if opts.is_empty() {
        return Vec::new();
    }
    let pref_norm = preferred_audio_lang
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.replace('_', "-"));
    let pref_base = pref_norm
        .as_deref()
        .map(|s| s.split('-').next().unwrap_or(s));
    let exact_match = pref_norm.as_deref().is_some_and(|pref| {
        opts.iter().any(|opt| {
            opt.language_code
                .replace('_', "-")
                .eq_ignore_ascii_case(pref)
        })
    });
    let base_match = !exact_match
        && pref_base.is_some_and(|base| {
            opts.iter().any(|opt| {
                opt.language_code
                    .split(['-', '_'])
                    .next()
                    .unwrap_or(&opt.language_code)
                    .eq_ignore_ascii_case(base)
            })
        });
    let mut marked_one = false;
    let mut mapped: Vec<overlay::OverlayAudioTrack> = opts
        .iter()
        .map(|opt| {
            let is_sel = if marked_one {
                false
            } else if exact_match {
                let pref = pref_norm.as_deref().unwrap_or_default();
                opt.language_code
                    .replace('_', "-")
                    .eq_ignore_ascii_case(pref)
            } else if base_match {
                let base = pref_base.unwrap_or_default();
                opt.language_code
                    .split(['-', '_'])
                    .next()
                    .unwrap_or(&opt.language_code)
                    .eq_ignore_ascii_case(base)
            } else {
                opt.is_default
            };
            if is_sel {
                marked_one = true;
            }
            opt.to_overlay_audio_track(is_sel)
        })
        .collect();
    if !marked_one && let Some(first) = mapped.first_mut() {
        first.selected = true;
    }
    mapped
}

// ---------------------------------------------------------------------------
// Wave 3 Slice E: Ambient Mode, Player Micro-Interactions, Live Chat Views & Shorts Helpers
// ---------------------------------------------------------------------------

pub(crate) const DOUBLE_CLICK_DEBOUNCE_WINDOW: Duration = Duration::from_millis(230);
pub(crate) const SEEK_RIPPLE_DURATION_MS: f32 = 520.0;
pub(crate) const HOLD_SPEED_THRESHOLD: Duration = Duration::from_millis(320);
pub(crate) const HOLD_SPEED_MULTIPLIER: f64 = 2.0;
pub(crate) const UP_NEXT_COUNTDOWN_SECS: f32 = 5.0;
pub(crate) const SHORTS_WHEEL_SNAP_COOLDOWN: Duration = Duration::from_millis(320);
pub(crate) const SHORTS_WHEEL_SNAP_THRESHOLD: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlayerClickZone {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlayerPointerGesture {
    None,
    SeekBack10,
    SeekForward10,
    ToggleFullscreen,
    TogglePlay,
}

/// Classifies a horizontal click coordinate on `video_rect` into Left (<30%), Center, or Right (>70%).
pub(crate) fn classify_player_click_zone(click_x: f32, rect: egui::Rect) -> PlayerClickZone {
    let w = rect.width().max(1.0);
    let rel = ((click_x - rect.left()) / w).clamp(0.0, 1.0);
    if rel < 0.30 {
        PlayerClickZone::Left
    } else if rel > 0.70 {
        PlayerClickZone::Right
    } else {
        PlayerClickZone::Center
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PlayerClickDebounceState {
    pub(crate) pending_single_click_at: Option<Instant>,
}

impl PlayerClickDebounceState {
    pub(crate) fn on_single_click(&mut self, now: Instant) {
        self.pending_single_click_at = Some(now);
    }

    pub(crate) fn on_double_click(&mut self, zone: PlayerClickZone) -> PlayerPointerGesture {
        self.pending_single_click_at = None;
        match zone {
            PlayerClickZone::Left => PlayerPointerGesture::SeekBack10,
            PlayerClickZone::Right => PlayerPointerGesture::SeekForward10,
            PlayerClickZone::Center => PlayerPointerGesture::ToggleFullscreen,
        }
    }

    pub(crate) fn poll_ready_single_click(&mut self, now: Instant) -> PlayerPointerGesture {
        if let Some(at) = self.pending_single_click_at
            && now.saturating_duration_since(at) >= DOUBLE_CLICK_DEBOUNCE_WINDOW
        {
            self.pending_single_click_at = None;
            return PlayerPointerGesture::TogglePlay;
        }
        PlayerPointerGesture::None
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.pending_single_click_at.is_some()
    }

    pub(crate) fn cancel(&mut self) {
        self.pending_single_click_at = None;
    }
}

#[derive(Clone, Copy)]
struct SeekRippleFeedback {
    zone: PlayerClickZone,
    started_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HoldSpeedTransition {
    None,
    Activate2x,
    Restore(f64),
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HoldSpeedBoostState {
    pub(crate) pressed_at: Option<Instant>,
    pub(crate) active: bool,
    pub(crate) restore_speed: Option<f64>,
    pub(crate) suppress_next_click: bool,
}

impl HoldSpeedBoostState {
    pub(crate) fn update(
        &mut self,
        held: bool,
        current_speed: f64,
        now: Instant,
    ) -> HoldSpeedTransition {
        if held {
            let started = *self.pressed_at.get_or_insert(now);
            if !self.active && now.saturating_duration_since(started) >= HOLD_SPEED_THRESHOLD {
                self.active = true;
                self.restore_speed = Some(if current_speed.is_finite() && current_speed > 0.0 {
                    current_speed
                } else {
                    1.0
                });
                self.suppress_next_click = true;
                return HoldSpeedTransition::Activate2x;
            }
            HoldSpeedTransition::None
        } else {
            self.pressed_at = None;
            if self.active {
                self.active = false;
                let prev = self.restore_speed.take().unwrap_or(1.0);
                return HoldSpeedTransition::Restore(prev);
            }
            self.restore_speed = None;
            HoldSpeedTransition::None
        }
    }

    pub(crate) fn take_suppressed_click(&mut self) -> bool {
        std::mem::take(&mut self.suppress_next_click)
    }
}

/// Computes the next volume percentage (`0.0..=100.0`) from a mouse-wheel Y delta.
pub(crate) fn compute_wheel_volume_target(
    current_volume: f32,
    muted: bool,
    wheel_delta_y: f32,
    step: f32,
) -> Option<f32> {
    if !wheel_delta_y.is_finite() || wheel_delta_y.abs() < 1.0 {
        return None;
    }
    let base = if muted {
        0.0
    } else {
        current_volume.clamp(0.0, 100.0)
    };
    let delta = if wheel_delta_y > 0.0 {
        step.abs()
    } else {
        -step.abs()
    };
    Some((base + delta).clamp(0.0, 100.0).round())
}

pub(crate) const VOLUME_WHEEL_SCROLL_GUARD_WINDOW: Duration = Duration::from_millis(350);

/// Suppresses wheel-to-volume when an active page scroll moves the volume controls under a stationary pointer.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VolumeWheelScrollGuard {
    last_page_scroll_at: Option<Instant>,
    last_page_scroll_pos: Option<egui::Pos2>,
}

impl VolumeWheelScrollGuard {
    pub(crate) fn allow_volume_wheel(
        &mut self,
        over_volume: bool,
        pointer_pos: Option<egui::Pos2>,
        has_vertical_wheel: bool,
        now: Instant,
    ) -> bool {
        let pointer_moved = match (self.last_page_scroll_pos, pointer_pos) {
            (Some(prev), Some(curr)) => prev.distance_sq(curr) > 4.0,
            _ => false,
        };
        let expired = self.last_page_scroll_at.is_some_and(|at| {
            now.saturating_duration_since(at) >= VOLUME_WHEEL_SCROLL_GUARD_WINDOW
        });
        if pointer_moved || expired || pointer_pos.is_none() {
            self.last_page_scroll_at = None;
            self.last_page_scroll_pos = None;
        }

        if !over_volume {
            if has_vertical_wheel {
                self.last_page_scroll_at = Some(now);
                self.last_page_scroll_pos = pointer_pos;
            }
            return false;
        }

        if self.last_page_scroll_at.is_some() {
            if has_vertical_wheel {
                self.last_page_scroll_at = Some(now);
                self.last_page_scroll_pos = pointer_pos;
            }
            return false;
        }

        true
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SleepTimerTarget {
    #[default]
    Off,
    Until(Instant),
    EndOfVideo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SleepTimerTickOutcome {
    Idle,
    Active { remaining_secs: Option<u64> },
    ExpiredPause,
}

pub(crate) fn evaluate_sleep_timer_tick(
    target: SleepTimerTarget,
    ended: bool,
    now: Instant,
) -> SleepTimerTickOutcome {
    match target {
        SleepTimerTarget::Off => SleepTimerTickOutcome::Idle,
        SleepTimerTarget::EndOfVideo => {
            if ended {
                SleepTimerTickOutcome::ExpiredPause
            } else {
                SleepTimerTickOutcome::Active {
                    remaining_secs: None,
                }
            }
        }
        SleepTimerTarget::Until(deadline) => {
            if now >= deadline {
                SleepTimerTickOutcome::ExpiredPause
            } else {
                let remaining = deadline.saturating_duration_since(now).as_secs().max(1);
                SleepTimerTickOutcome::Active {
                    remaining_secs: Some(remaining),
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
struct UpNextCountdownState {
    source_video_url: String,
    next_video: Video,
    started_at: Instant,
    cancelled: bool,
    triggered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum UpNextCountdownTick {
    Inactive,
    Counting { remaining_secs: f32, progress: f32 },
    TriggerNext,
}

pub(crate) fn evaluate_up_next_countdown(
    started_at: Instant,
    cancelled: bool,
    now: Instant,
) -> UpNextCountdownTick {
    if cancelled {
        return UpNextCountdownTick::Inactive;
    }
    let elapsed = now.saturating_duration_since(started_at).as_secs_f32();
    if elapsed >= UP_NEXT_COUNTDOWN_SECS {
        return UpNextCountdownTick::TriggerNext;
    }
    let remaining_secs = (UP_NEXT_COUNTDOWN_SECS - elapsed).max(0.0);
    let progress = (elapsed / UP_NEXT_COUNTDOWN_SECS).clamp(0.0, 1.0);
    UpNextCountdownTick::Counting {
        remaining_secs,
        progress,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShortsWheelSnapDirection {
    None,
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ShortsWheelSnapState {
    pub(crate) accum_delta_y: f32,
    pub(crate) last_snap_at: Option<Instant>,
}

impl ShortsWheelSnapState {
    pub(crate) fn feed_wheel_delta(
        &mut self,
        delta_y: f32,
        now: Instant,
    ) -> ShortsWheelSnapDirection {
        if !delta_y.is_finite() || delta_y.abs() < 0.5 {
            return ShortsWheelSnapDirection::None;
        }
        if self
            .last_snap_at
            .is_some_and(|at| now.saturating_duration_since(at) < SHORTS_WHEEL_SNAP_COOLDOWN)
        {
            self.accum_delta_y = 0.0;
            return ShortsWheelSnapDirection::None;
        }
        if self.accum_delta_y != 0.0 && self.accum_delta_y.signum() != delta_y.signum() {
            self.accum_delta_y = 0.0;
        }
        self.accum_delta_y += delta_y;
        if self.accum_delta_y <= -SHORTS_WHEEL_SNAP_THRESHOLD {
            self.accum_delta_y = 0.0;
            self.last_snap_at = Some(now);
            ShortsWheelSnapDirection::Next
        } else if self.accum_delta_y >= SHORTS_WHEEL_SNAP_THRESHOLD {
            self.accum_delta_y = 0.0;
            self.last_snap_at = Some(now);
            ShortsWheelSnapDirection::Previous
        } else {
            ShortsWheelSnapDirection::None
        }
    }
}

/// Computes a vibrant dominant accent color from encoded thumbnail bytes (sampled on a sparse grid).
pub(crate) fn compute_dominant_accent_from_image_bytes(bytes: &[u8]) -> Option<Color32> {
    let img = image::load_from_memory(bytes).ok()?.to_rgb8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let step_x = (w / 16).max(1);
    let step_y = (h / 9).max(1);
    let mut sum_r = 0u64;
    let mut sum_g = 0u64;
    let mut sum_b = 0u64;
    let mut total_w = 0u64;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let p = img.get_pixel(x, y);
            let r = p[0];
            let g = p[1];
            let b = p[2];
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            if max_c >= 24 && min_c <= 238 {
                let sat = (max_c - min_c) as u64;
                let weight = sat.saturating_mul(2) + 24;
                sum_r += (r as u64) * weight;
                sum_g += (g as u64) * weight;
                sum_b += (b as u64) * weight;
                total_w += weight;
            }
            x += step_x;
        }
        y += step_y;
    }
    if total_w == 0 {
        return None;
    }
    let r = ((sum_r / total_w) as u8).clamp(36, 224);
    let g = ((sum_g / total_w) as u8).clamp(36, 224);
    let b = ((sum_b / total_w) as u8).clamp(36, 224);
    Some(Color32::from_rgb(r, g, b))
}

pub(crate) fn fallback_ambient_color_for_video(video: &Video) -> Color32 {
    let seed = if !video.channel.trim().is_empty() {
        video.channel.as_str()
    } else if !video.title.trim().is_empty() {
        video.title.as_str()
    } else {
        video.url.as_str()
    };
    let [r, g, b] = recommendations::channel_avatar_rgb(seed);
    Color32::from_rgb(r, g, b)
}

impl FastTube {
    pub(crate) fn paint_hairline_divider(ui: &mut egui::Ui, palette: &ThemePalette) {
        let width = ui.available_width().max(1.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
        ui.painter().line_segment(
            [
                egui::pos2(rect.left(), rect.center().y),
                egui::pos2(rect.right(), rect.center().y),
            ],
            egui::Stroke::new(1.0, palette.card_alt_stroke),
        );
    }

    pub(crate) fn paint_comment_avatar_ring(ui: &egui::Ui, rect: egui::Rect) {
        ui.painter().circle_stroke(
            rect.center(),
            (rect.width().min(rect.height()) * 0.5 - 0.5).max(1.0),
            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(128, 128, 128, 45)),
        );
    }

    pub(crate) fn is_watch_short_mode(&self) -> bool {
        let Some(video) = self.selected.as_ref() else {
            return false;
        };
        if video.url.contains("/shorts/")
            || self
                .metadata_for(&video.url)
                .is_some_and(|meta| meta.kind == search::SearchResultKind::Short)
        {
            return true;
        }
        if matches!(self.feed.filter, recommendations::FeedFilter::Shorts)
            || self.ui.browse_section == BrowseSection::Shorts
        {
            recommendations::is_short_video(video, self.metadata_for(&video.url))
        } else {
            false
        }
    }

    pub(crate) fn watch_page_side_by_side_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        let side_margin = watch_page_side_margin(ui.available_width(), columns);
        egui::ScrollArea::vertical()
            .id_salt("watch_primary_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(side_margin);
                    ui.vertical(|ui| {
                        ui.set_width(columns.primary_width);
                        ui.set_max_width(columns.primary_width);
                        self.watch_primary_ui(
                            ui,
                            ctx,
                            columns.primary_width,
                            available_height,
                            WatchLayoutMode::SideBySide,
                        );
                        if is_actionable_inline_message(&self.message) {
                            ui.add_space(6.0);
                            let palette = self.palette();
                            if inline_error_banner_ui(ui, &self.message, &palette) {
                                self.message.clear();
                            }
                        }
                    });
                    ui.add_space(columns.gap);
                    ui.vertical(|ui| {
                        ui.set_width(columns.secondary_width);
                        ui.set_max_width(columns.secondary_width);
                        if self.watch_live_chat_panel_ui(
                            ui,
                            ctx,
                            columns.secondary_width,
                            (available_height - 24.0).max(320.0),
                        ) {
                            ui.add_space(10.0);
                        }
                        chosen =
                            self.watch_sidebar_results_ui(ui, ctx, "watch_sidebar_results_scroll");
                    });
                });
            });
        chosen
    }

    pub(crate) fn watch_page_short_column_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("watch_short_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.watch_primary_ui(
                    ui,
                    ctx,
                    columns.primary_width,
                    available_height,
                    WatchLayoutMode::Short,
                );
                if is_actionable_inline_message(&self.message) {
                    ui.add_space(6.0);
                    let palette = self.palette();
                    if inline_error_banner_ui(ui, &self.message, &palette) {
                        self.message.clear();
                    }
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                chosen = self.watch_sidebar_results_ui(ui, ctx, "watch_short_results_scroll");
            });
        chosen
    }

    pub(crate) fn watch_page_stacked_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        if self.playback.theater {
            egui::ScrollArea::vertical()
                .id_salt("watch_theater_main_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let theater_width = ui.available_width().max(columns.primary_width);
                    egui::Frame::NONE.fill(Color32::BLACK).show(ui, |ui| {
                        ui.set_width(theater_width);
                        ui.vertical_centered(|ui| {
                            self.watch_primary_with_metadata_ui(
                                ui,
                                ctx,
                                theater_width,
                                available_height,
                                WatchLayoutMode::Stacked,
                                false,
                            );
                        });
                    });
                    ui.add_space(12.0);
                    let margin = if theater_width >= 680.0 {
                        watch_page_margin()
                    } else {
                        12.0
                    };
                    let inner_width = (theater_width - margin * 2.0).max(260.0);
                    let below_columns = compute_watch_columns(inner_width);
                    let side_margin = watch_page_side_margin(theater_width, &below_columns);
                    let selected_video = self.selected.clone();
                    #[cfg(windows)]
                    let resolution_badge = selected_video
                        .as_ref()
                        .and_then(|video| {
                            (self.playback.last_video_size_url.as_deref()
                                == Some(video.url.as_str()))
                            .then_some(self.playback.last_video_size)
                            .flatten()
                        })
                        .map(|size| format!("Reproduzindo em {size}"));
                    #[cfg(not(windows))]
                    let resolution_badge: Option<String> = None;
                    if below_columns.mode == WatchLayoutMode::SideBySide {
                        ui.horizontal_top(|ui| {
                            ui.add_space(side_margin);
                            ui.vertical(|ui| {
                                ui.set_width(below_columns.primary_width);
                                ui.set_max_width(below_columns.primary_width);
                                if let Some(video) = &selected_video {
                                    self.watch_metadata_card_ui(
                                        ui,
                                        video,
                                        below_columns.primary_width,
                                        resolution_badge.as_deref(),
                                    );
                                }
                                if is_actionable_inline_message(&self.message) {
                                    ui.add_space(6.0);
                                    let palette = self.palette();
                                    if inline_error_banner_ui(ui, &self.message, &palette) {
                                        self.message.clear();
                                    }
                                }
                            });
                            ui.add_space(below_columns.gap);
                            ui.vertical(|ui| {
                                ui.set_width(below_columns.secondary_width);
                                ui.set_max_width(below_columns.secondary_width);
                                if self.watch_live_chat_panel_ui(
                                    ui,
                                    ctx,
                                    below_columns.secondary_width,
                                    (available_height - 24.0).max(320.0),
                                ) {
                                    ui.add_space(10.0);
                                }
                                chosen = self.watch_sidebar_results_ui(
                                    ui,
                                    ctx,
                                    "watch_theater_results_scroll",
                                );
                            });
                        });
                    } else {
                        ui.horizontal_top(|ui| {
                            ui.add_space(side_margin);
                            ui.vertical(|ui| {
                                ui.set_width(below_columns.primary_width);
                                ui.set_max_width(below_columns.primary_width);
                                if let Some(video) = &selected_video {
                                    self.watch_metadata_card_ui(
                                        ui,
                                        video,
                                        below_columns.primary_width,
                                        resolution_badge.as_deref(),
                                    );
                                }
                                if is_actionable_inline_message(&self.message) {
                                    ui.add_space(6.0);
                                    let palette = self.palette();
                                    if inline_error_banner_ui(ui, &self.message, &palette) {
                                        self.message.clear();
                                    }
                                }
                                if self.watch_live_chat_panel_ui(
                                    ui,
                                    ctx,
                                    below_columns.primary_width,
                                    420.0,
                                ) {
                                    ui.add_space(8.0);
                                }
                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(6.0);
                                chosen = self.watch_sidebar_results_ui(
                                    ui,
                                    ctx,
                                    "watch_theater_results_scroll",
                                );
                            });
                        });
                    }
                });
            return chosen;
        }
        let side_margin = watch_page_side_margin(ui.available_width(), columns);
        egui::ScrollArea::vertical()
            .id_salt("watch_stacked_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(side_margin);
                    ui.vertical(|ui| {
                        ui.set_width(columns.primary_width);
                        ui.set_max_width(columns.primary_width);
                        self.watch_primary_ui(
                            ui,
                            ctx,
                            columns.primary_width,
                            available_height,
                            WatchLayoutMode::Stacked,
                        );
                        if is_actionable_inline_message(&self.message) {
                            ui.add_space(6.0);
                            let palette = self.palette();
                            if inline_error_banner_ui(ui, &self.message, &palette) {
                                self.message.clear();
                            }
                        }
                        if self.watch_live_chat_panel_ui(ui, ctx, columns.primary_width, 420.0) {
                            ui.add_space(8.0);
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(6.0);
                        chosen =
                            self.watch_sidebar_results_ui(ui, ctx, "watch_stacked_results_scroll");
                    });
                });
            });
        chosen
    }

    pub(crate) fn watch_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if self.is_watch_short_mode() && !self.playback.theater && !self.playback.fullscreen {
            return self.shorts_watch_page_ui(ui, ctx);
        }
        let columns = compute_watch_page_columns(
            ui.available_width(),
            ui.available_height(),
            self.playback.theater,
        );
        let available_height = ui.available_height();
        let chosen = match columns.mode {
            WatchLayoutMode::SideBySide => {
                self.watch_page_side_by_side_ui(ui, ctx, &columns, available_height)
            }
            WatchLayoutMode::Short => {
                self.watch_page_short_column_ui(ui, ctx, &columns, available_height)
            }
            WatchLayoutMode::Stacked => {
                self.watch_page_stacked_ui(ui, ctx, &columns, available_height)
            }
        };
        self.watch_report_modal_ui(ctx);
        chosen
    }
}
