#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod backup;
mod browse_ui;
mod channel_ui;
mod command_palette;
mod comment_actions;
mod community_apis;
mod desktop;
mod diagnostics;
mod downloads;
mod feed_prefs;
mod followed_channels;
mod history;
mod i18n;
mod insights;
mod layout;
mod library;
mod library_ui;
mod live_chat;
mod oauth;
mod overlay;
#[cfg(windows)]
mod player;
mod process;
mod queue;
mod recommendations;
mod search;
mod session;
mod settings;
mod settings_ui;
mod skeleton;
mod sponsorblock;
mod thumbnails;
mod toast;
mod transcript;
mod ui_icons;
mod updater;
mod watch_extras;
mod watch_ui;

use diagnostics::{DecodingDiagnostics, VideoSize, collect_from_property_reader};
use eframe::egui::{self, Color32, RichText};
use history::{Entry as HistoryEntry, History};
use layout::*;
use queue::{PlaybackEvent, Queue, QueueLoadResult, QueuePlaybackCorrelation};
use recommendations::{FeedFilter, RecommendationEngine, ScoredVideo, UserProfile};
use search::{ChannelPage, ChannelTarget, SearchResult, Video, VideoMetadata};
use settings::{BufferProfile, BufferSettings, PlaybackPreferences, Theme};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thumbnails::ThumbnailCache;
use ui_icons::*;

pub(crate) fn extract_playlist_id_from_url(raw: &str) -> Option<String> {
    match command_palette::parse_direct_youtube_url(raw) {
        Some(command_palette::DirectYouTubeTarget::Playlist { list_id, .. }) => Some(list_id),
        _ => None,
    }
}

fastframe_icons::icons! {
    enum Icon {
        prefix: "fasttube-icon-",
        directory: "../assets/icons/",
        Play => lucide "play",
        Pause => lucide "pause",
        Search => lucide "search",
        Volume => lucide "volume-2",
        Muted => lucide "volume-x",
        Maximize => lucide "maximize-2",
        Minimize => lucide "minimize-2",
        Close => lucide "x",
        ThumbsUp => "thumbs-up",
        ThumbsDown => "thumbs-down",
        Smile => "smile",
    }
}

pub(crate) const BROWSE_MAX_CONTENT_WIDTH: f32 = 1600.0;
pub(crate) const HISTORY_SAVE_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const HISTORY_MIN_POSITION_DELTA_MS: u64 = 1_000;
pub(crate) const HISTORY_MAX_TEXT_BYTES: usize = 4096;
pub(crate) const DIAGNOSTICS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const DEFAULT_CONFIGURED_HWDEC: &str = "auto-safe";
pub(crate) const SUBSCRIPTIONS_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

pub(crate) const DEFAULT_QUEUE_LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiagnosticsDisplay {
    status_line: String,
    configured_line: String,
    effective_line: String,
    metadata_line: Option<String>,
    summary_line: String,
    note_line: &'static str,
}

pub(crate) fn format_diagnostics_display(
    diag: &DecodingDiagnostics,
    has_selected_video: bool,
) -> DiagnosticsDisplay {
    let configured_label = diag.configured_hwdec_label();
    let effective_short = diag.effective_decoding.short_label();
    let configured_line = format!(
        "Política solicitada (hwdec): {configured_label} (apenas configuração; não confirma aceleração ativa por si só)"
    );
    let note_line = "Nota: hwdec configurado define apenas a política solicitada ao libmpv, sem alegar economia de memória nem aceleração de GPU sem confirmação em hwdec-current. Amostragem sob demanda (~1 s) apenas com este painel visível.";

    if !has_selected_video {
        let inactive_diag = DecodingDiagnostics::unknown(diag.configured_hwdec.as_deref());
        return DiagnosticsDisplay {
            status_line: "Estado do vídeo: inativo (nenhum vídeo em reprodução)".into(),
            configured_line,
            effective_line:
                "Decodificação efetiva (hwdec-current): desconhecido / inativo (sem vídeo ativo)"
                    .into(),
            metadata_line: None,
            summary_line: inactive_diag.summary_line(),
            note_line,
        };
    }

    let status_line = if diag.effective_decoding.is_hardware_active() {
        let backend = diag
            .effective_decoding
            .active_backend()
            .unwrap_or("hardware");
        format!("Estado do vídeo: decodificação por hardware ativa confirmada ({backend})")
    } else if diag.effective_decoding.is_software() {
        "Estado do vídeo: decodificação por software confirmada (hwdec-current = no)".into()
    } else {
        debug_assert!(diag.effective_decoding.is_unknown());
        "Estado do vídeo: decodificação efetiva desconhecida / não confirmada".into()
    };

    let effective_line = format!("Decodificação efetiva (hwdec-current): {effective_short}");

    let mut meta_parts = Vec::new();
    if let Some(codec) = &diag.codec {
        meta_parts.push(format!("Codec: {codec}"));
    }
    if let Some(size) = &diag.size {
        meta_parts.push(format!("Tamanho: {size}"));
    }
    let metadata_line = if meta_parts.is_empty() {
        None
    } else {
        Some(meta_parts.join(" • "))
    };

    DiagnosticsDisplay {
        status_line,
        configured_line,
        effective_line,
        metadata_line,
        summary_line: diag.summary_line(),
        note_line,
    }
}

pub(crate) fn update_decoding_diagnostics<F>(
    diagnostics: &mut DecodingDiagnostics,
    last_poll_at: &mut Option<Instant>,
    settings_open: bool,
    has_active_video: bool,
    now: Instant,
    read_property: F,
) -> bool
where
    F: FnMut(&str) -> Option<String>,
{
    if !has_active_video {
        *diagnostics = DecodingDiagnostics::unknown(Some(DEFAULT_CONFIGURED_HWDEC));
        *last_poll_at = None;
        return false;
    }
    if !settings_open {
        return false;
    }
    if last_poll_at
        .is_some_and(|last| now.saturating_duration_since(last) < DIAGNOSTICS_REFRESH_INTERVAL)
    {
        return false;
    }
    let mut diag = collect_from_property_reader(read_property);
    if diag.configured_hwdec.is_none() {
        diag.configured_hwdec = Some(DEFAULT_CONFIGURED_HWDEC.to_owned());
    }
    *diagnostics = diag;
    *last_poll_at = Some(now);
    true
}

pub(crate) fn buffer_profile_name(profile: BufferProfile) -> &'static str {
    match profile {
        BufferProfile::Small => "Pequeno",
        BufferProfile::Default => "Padrão",
        BufferProfile::Large => "Grande",
        BufferProfile::Custom(_) => "Personalizado",
    }
}

pub(crate) fn buffer_profile_summary(profile: BufferProfile) -> String {
    let values: BufferSettings = profile.values();
    format!(
        "{} ({} / {} MiB, {}s)",
        buffer_profile_name(profile),
        values.max_mib,
        values.back_mib,
        values.cache_seconds
    )
}

pub(crate) fn theme_name(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "Escuro",
        Theme::Light => "Claro",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ThemePalette {
    pub(crate) icon_fg: Color32,
    pub(crate) header_button_bg: Color32,
    pub(crate) header_button_fg: Color32,
    pub(crate) nav_button_bg: Color32,
    pub(crate) nav_button_fg: Color32,
    pub(crate) active_button_bg: Color32,
    pub(crate) active_button_fg: Color32,
    pub(crate) theater_inactive_bg: Color32,
    pub(crate) theater_inactive_fg: Color32,
    pub(crate) playing_nav_bg: Color32,
    pub(crate) playing_nav_fg: Color32,
    pub(crate) player_bar_bg: Color32,
    pub(crate) metadata_card_bg: Color32,
    pub(crate) channel_fg: Color32,
    pub(crate) description_fg: Color32,
    pub(crate) card_bg: Color32,
    pub(crate) card_stroke: Color32,
    pub(crate) card_alt_bg: Color32,
    pub(crate) card_alt_stroke: Color32,
    pub(crate) card_playing_bg: Color32,
    pub(crate) card_playing_stroke: Color32,
    pub(crate) playing_badge_fg: Color32,
    pub(crate) queue_next_bg: Color32,
    pub(crate) snippet_fg: Color32,
    pub(crate) resume_status_fg: Color32,
    pub(crate) watched_status_fg: Color32,
    pub(crate) neutral_status_fg: Color32,
    pub(crate) info_box_bg: Color32,
    pub(crate) info_box_stroke: Color32,
    pub(crate) info_accent_fg: Color32,
    pub(crate) profile_selected_bg: Color32,
    pub(crate) profile_selected_stroke: Color32,
    pub(crate) profile_unselected_bg: Color32,
    pub(crate) profile_unselected_stroke: Color32,
    pub(crate) warning_fg: Color32,
    pub(crate) error_fg: Color32,
    pub(crate) notice_bg: Color32,
    pub(crate) notice_stroke: Color32,
    pub(crate) notice_fg: Color32,
    pub(crate) error_bg: Color32,
    pub(crate) error_stroke: Color32,
    pub(crate) error_banner_fg: Color32,
    pub(crate) message_box_bg: Color32,
    pub(crate) diag_inactive_fg: Color32,
    pub(crate) diag_hw_fg: Color32,
    pub(crate) diag_sw_fg: Color32,
    pub(crate) diag_unknown_fg: Color32,
}

impl ThemePalette {
    pub(crate) fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self {
                icon_fg: Color32::from_rgb(241, 241, 241),
                header_button_bg: Color32::from_rgb(39, 39, 39),
                header_button_fg: Color32::from_rgb(241, 241, 241),
                nav_button_bg: Color32::TRANSPARENT,
                nav_button_fg: Color32::from_rgb(241, 241, 241),
                active_button_bg: Color32::from_rgb(241, 241, 241),
                active_button_fg: Color32::from_rgb(15, 15, 15),
                theater_inactive_bg: Color32::from_rgb(39, 39, 39),
                theater_inactive_fg: Color32::from_rgb(241, 241, 241),
                playing_nav_bg: Color32::from_rgb(50, 22, 26),
                playing_nav_fg: Color32::from_rgb(255, 120, 120),
                player_bar_bg: Color32::from_rgb(33, 33, 33),
                metadata_card_bg: Color32::from_rgb(15, 15, 15),
                channel_fg: Color32::from_rgb(241, 241, 241),
                description_fg: Color32::from_rgb(241, 241, 241),
                card_bg: Color32::from_rgb(15, 15, 15),
                card_stroke: Color32::TRANSPARENT,
                card_alt_bg: Color32::from_rgb(33, 33, 33),
                card_alt_stroke: Color32::from_rgb(48, 48, 48),
                card_playing_bg: Color32::from_rgb(30, 30, 30),
                card_playing_stroke: Color32::from_rgb(255, 0, 51),
                playing_badge_fg: Color32::from_rgb(255, 138, 138),
                queue_next_bg: Color32::from_rgb(28, 38, 48),
                snippet_fg: Color32::from_rgb(170, 170, 170),
                resume_status_fg: Color32::from_rgb(62, 166, 255),
                watched_status_fg: Color32::from_rgb(155, 210, 165),
                neutral_status_fg: Color32::from_rgb(170, 170, 170),
                info_box_bg: Color32::from_rgb(24, 24, 24),
                info_box_stroke: Color32::from_rgb(48, 48, 48),
                info_accent_fg: Color32::from_rgb(62, 166, 255),
                profile_selected_bg: Color32::from_rgb(31, 31, 31),
                profile_selected_stroke: Color32::from_rgb(255, 0, 51),
                profile_unselected_bg: Color32::from_rgb(33, 33, 33),
                profile_unselected_stroke: Color32::from_rgb(48, 48, 48),
                warning_fg: Color32::from_rgb(255, 183, 77),
                error_fg: Color32::from_rgb(255, 138, 138),
                notice_bg: Color32::from_rgb(21, 42, 29),
                notice_stroke: Color32::from_rgb(64, 130, 84),
                notice_fg: Color32::from_rgb(185, 235, 195),
                error_bg: Color32::from_rgb(44, 24, 26),
                error_stroke: Color32::from_rgb(176, 66, 70),
                error_banner_fg: Color32::from_rgb(255, 175, 175),
                message_box_bg: Color32::from_rgb(33, 33, 33),
                diag_inactive_fg: Color32::from_rgb(170, 170, 170),
                diag_hw_fg: Color32::from_rgb(145, 225, 165),
                diag_sw_fg: Color32::from_rgb(245, 200, 125),
                diag_unknown_fg: Color32::from_rgb(190, 205, 225),
            },
            Theme::Light => Self {
                icon_fg: Color32::from_rgb(15, 15, 15),
                header_button_bg: Color32::from_rgb(242, 242, 242),
                header_button_fg: Color32::from_rgb(15, 15, 15),
                nav_button_bg: Color32::from_rgb(242, 242, 242),
                nav_button_fg: Color32::from_rgb(15, 15, 15),
                active_button_bg: Color32::from_rgb(15, 15, 15),
                active_button_fg: Color32::WHITE,
                theater_inactive_bg: Color32::from_rgb(242, 242, 242),
                theater_inactive_fg: Color32::from_rgb(15, 15, 15),
                playing_nav_bg: Color32::from_rgb(252, 226, 229),
                playing_nav_fg: Color32::from_rgb(172, 24, 34),
                player_bar_bg: Color32::from_rgb(242, 242, 242),
                metadata_card_bg: Color32::from_rgb(249, 249, 249),
                channel_fg: Color32::from_rgb(15, 15, 15),
                description_fg: Color32::from_rgb(15, 15, 15),
                card_bg: Color32::WHITE,
                card_stroke: Color32::TRANSPARENT,
                card_alt_bg: Color32::from_rgb(242, 242, 242),
                card_alt_stroke: Color32::from_rgb(229, 229, 229),
                card_playing_bg: Color32::from_rgb(242, 242, 242),
                card_playing_stroke: Color32::from_rgb(196, 52, 62),
                playing_badge_fg: Color32::from_rgb(176, 24, 34),
                queue_next_bg: Color32::from_rgb(238, 246, 255),
                snippet_fg: Color32::from_rgb(96, 96, 96),
                resume_status_fg: Color32::from_rgb(6, 95, 212),
                watched_status_fg: Color32::from_rgb(43, 166, 64),
                neutral_status_fg: Color32::from_rgb(96, 96, 96),
                info_box_bg: Color32::from_rgb(249, 249, 249),
                info_box_stroke: Color32::from_rgb(229, 229, 229),
                info_accent_fg: Color32::from_rgb(6, 95, 212),
                profile_selected_bg: Color32::from_rgb(249, 249, 249),
                profile_selected_stroke: Color32::from_rgb(196, 52, 62),
                profile_unselected_bg: Color32::from_rgb(242, 242, 242),
                profile_unselected_stroke: Color32::from_rgb(229, 229, 229),
                warning_fg: Color32::from_rgb(168, 86, 0),
                error_fg: Color32::from_rgb(180, 28, 34),
                notice_bg: Color32::from_rgb(230, 245, 234),
                notice_stroke: Color32::from_rgb(82, 160, 102),
                notice_fg: Color32::from_rgb(20, 98, 40),
                error_bg: Color32::from_rgb(253, 233, 235),
                error_stroke: Color32::from_rgb(202, 86, 92),
                error_banner_fg: Color32::from_rgb(160, 24, 30),
                message_box_bg: Color32::from_rgb(242, 242, 242),
                diag_inactive_fg: Color32::from_rgb(96, 96, 96),
                diag_hw_fg: Color32::from_rgb(20, 114, 46),
                diag_sw_fg: Color32::from_rgb(164, 88, 0),
                diag_unknown_fg: Color32::from_rgb(34, 78, 136),
            },
        }
    }
}

pub(crate) fn visuals_for_theme(theme: Theme) -> egui::Visuals {
    let mut visuals = match theme {
        Theme::Dark => {
            let mut v = egui::Visuals::dark();
            v.panel_fill = Color32::from_rgb(15, 15, 15);
            v.window_fill = Color32::from_rgb(28, 28, 30);
            v.window_stroke = egui::Stroke::new(1.0, Color32::from_rgb(52, 52, 56));
            v.extreme_bg_color = Color32::from_rgb(20, 20, 22);
            v.faint_bg_color = Color32::from_rgb(33, 33, 36);
            v.selection.bg_fill = Color32::from_rgb(35, 75, 118);
            v.selection.stroke = egui::Stroke::new(1.2, Color32::from_rgb(62, 166, 255));
            v.hyperlink_color = Color32::from_rgb(62, 166, 255);
            v.widgets.noninteractive.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_rgb(46, 46, 50));
            v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(241, 241, 241);
            v.widgets.inactive.bg_fill = Color32::from_rgb(39, 39, 42);
            v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(54, 54, 58));
            v.widgets.inactive.fg_stroke.color = Color32::from_rgb(241, 241, 241);
            v.widgets.hovered.bg_fill = Color32::from_rgb(56, 56, 60);
            v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(88, 88, 96));
            v.widgets.hovered.fg_stroke.color = Color32::WHITE;
            v.widgets.active.bg_fill = Color32::from_rgb(70, 70, 76);
            v.widgets.active.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(120, 120, 130));
            v.widgets.active.fg_stroke.color = Color32::WHITE;
            v.widgets.open.bg_fill = Color32::from_rgb(52, 52, 56);
            v.widgets.open.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(88, 88, 96));
            v.weak_text_color = Some(Color32::from_rgb(170, 170, 170));
            v
        }
        Theme::Light => {
            let mut v = egui::Visuals::light();
            v.panel_fill = Color32::WHITE;
            v.window_fill = Color32::WHITE;
            v.window_stroke = egui::Stroke::new(1.0, Color32::from_rgb(220, 220, 224));
            v.extreme_bg_color = Color32::from_rgb(246, 246, 248);
            v.faint_bg_color = Color32::from_rgb(242, 242, 245);
            v.selection.bg_fill = Color32::from_rgb(204, 226, 255);
            v.selection.stroke = egui::Stroke::new(1.2, Color32::from_rgb(6, 95, 212));
            v.hyperlink_color = Color32::from_rgb(6, 95, 212);
            v.widgets.noninteractive.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_rgb(226, 226, 230));
            v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.widgets.inactive.bg_fill = Color32::from_rgb(240, 240, 243);
            v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(216, 216, 222));
            v.widgets.inactive.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.widgets.hovered.bg_fill = Color32::from_rgb(228, 228, 232);
            v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(184, 184, 192));
            v.widgets.hovered.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.widgets.active.bg_fill = Color32::from_rgb(214, 214, 220);
            v.widgets.active.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(150, 150, 160));
            v.widgets.active.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.widgets.open.bg_fill = Color32::from_rgb(230, 230, 235);
            v.widgets.open.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(184, 184, 192));
            v.weak_text_color = Some(Color32::from_rgb(96, 96, 96));
            v
        }
    };
    let widget_radius = egui::CornerRadius::same(8);
    visuals.widgets.noninteractive.corner_radius = widget_radius;
    visuals.widgets.inactive.corner_radius = widget_radius;
    visuals.widgets.hovered.corner_radius = widget_radius;
    visuals.widgets.active.corner_radius = widget_radius;
    visuals.widgets.open.corner_radius = widget_radius;
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.menu_corner_radius = egui::CornerRadius::same(10);
    fastframe_text::detect().apply_to_visuals(&mut visuals);
    visuals
}

pub(crate) fn apply_theme_visuals(ctx: &egui::Context, theme: Theme) {
    let visuals = visuals_for_theme(theme);
    let egui_theme = match theme {
        Theme::Dark => egui::Theme::Dark,
        Theme::Light => egui::Theme::Light,
    };
    ctx.set_theme(egui_theme);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.animation_time = 0.14;
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.slider_width = 110.0;
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 7.0,
            handle_min_length: 28.0,
            bar_inner_margin: 3.0,
            bar_outer_margin: 2.0,
            ..egui::style::ScrollStyle::floating()
        };
    });
}

pub(crate) fn format_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push('.');
        }
        formatted.push(ch);
    }
    formatted
}

pub(crate) fn format_view_count(count: u64) -> String {
    if count == 1 {
        "1 visualização".into()
    } else {
        format!("{} visualizações", format_thousands(count))
    }
}

#[cfg(test)]
pub(crate) fn format_upload_date(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() == 8 && trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        let year = &trimmed[0..4];
        let month: u32 = trimmed[4..6].parse().ok()?;
        let day: u32 = trimmed[6..8].parse().ok()?;
        if (1..=12).contains(&month) && (1..=31).contains(&day) {
            return Some(format!("{day:02}/{month:02}/{year}"));
        }
    }
    Some(trimmed.to_owned())
}

pub(crate) fn clean_description(raw: Option<&str>) -> Option<String> {
    let text = raw?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

pub(crate) fn description_snippet(raw: Option<&str>, max_chars: usize) -> Option<String> {
    let cleaned = clean_description(raw)?;
    let collapsed: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let char_count = collapsed.chars().count();
    if char_count <= max_chars {
        Some(collapsed)
    } else {
        let prefix: String = collapsed
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect();
        Some(format!("{}…", prefix.trim_end()))
    }
}

#[cfg(test)]
pub(crate) fn format_card_metadata_line(
    duration: &str,
    metadata: Option<&VideoMetadata>,
) -> Option<String> {
    let mut parts = Vec::new();
    let duration = duration.trim();
    if !duration.is_empty() {
        parts.push(duration.to_owned());
    }
    if let Some(views) = metadata.and_then(|meta| meta.view_count) {
        parts.push(format_view_count(views));
    }
    if let Some(date) = metadata
        .and_then(|meta| meta.upload_date.as_deref())
        .and_then(format_upload_date)
    {
        parts.push(date);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" • "))
    }
}

#[cfg(test)]
pub(crate) fn format_subscriber_count(count: u64) -> String {
    if count == 1 {
        "1 inscrito".into()
    } else {
        format!("{} inscritos", format_thousands(count))
    }
}

pub(crate) fn channel_target_display_label(target: &ChannelTarget) -> &str {
    match target {
        ChannelTarget::ChannelId(id) => id.as_str(),
        ChannelTarget::Handle(handle) => handle.as_str(),
    }
}

pub(crate) fn channel_page_display_title(page: &ChannelPage) -> &str {
    page.title
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            page.handle
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .or_else(|| {
            page.channel_id
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_else(|| channel_target_display_label(&page.target))
}

#[cfg(test)]
pub(crate) fn format_channel_metadata_line(page: &ChannelPage) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(handle) = page
        .handle
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(handle.to_owned());
    }
    if let Some(subs) = page.subscriber_count {
        parts.push(format_subscriber_count(subs));
    }
    if let Some(channel_id) = page
        .channel_id
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("ID: {channel_id}"));
    }
    let video_count = page.videos.len();
    parts.push(if video_count == 1 {
        "1 vídeo".into()
    } else {
        format!("{video_count} vídeos")
    });
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" • "))
    }
}

#[cfg(test)]
pub(crate) fn format_channel_media_line(page: &ChannelPage) -> Option<String> {
    let mut parts = Vec::new();
    if page
        .avatar_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|url| !url.is_empty())
    {
        parts.push("Avatar informado".to_owned());
    }
    if page
        .banner_url
        .as_deref()
        .map(str::trim)
        .is_some_and(|url| !url.is_empty())
    {
        parts.push("Banner informado".to_owned());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" • "))
    }
}

pub(crate) fn parse_duration_ms(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    let total_seconds = match parts.as_slice() {
        [minutes, seconds] => {
            let mins: u64 = minutes.parse().ok()?;
            let secs: u64 = seconds.parse().ok()?;
            if secs >= 60 {
                return None;
            }
            mins.checked_mul(60)?.checked_add(secs)?
        }
        [hours, minutes, seconds] => {
            let hrs: u64 = hours.parse().ok()?;
            let mins: u64 = minutes.parse().ok()?;
            let secs: u64 = seconds.parse().ok()?;
            if mins >= 60 || secs >= 60 {
                return None;
            }
            hrs.checked_mul(3600)?
                .checked_add(mins.checked_mul(60)?)?
                .checked_add(secs)?
        }
        _ => return None,
    };
    total_seconds.checked_mul(1000).filter(|&ms| ms > 0)
}

pub(crate) fn is_live_duration_label(raw: &str) -> bool {
    let normalized = raw.trim().to_ascii_lowercase();
    normalized == "ao vivo" || normalized == "live"
}

pub(crate) fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub(crate) fn sanitize_history_title(title: &str, video_id: &str) -> String {
    let trimmed = title.trim();
    let fallback;
    let chosen = if trimmed.is_empty() {
        fallback = format!("Vídeo {video_id}");
        fallback.as_str()
    } else {
        trimmed
    };
    let truncated = truncate_utf8_bytes(chosen, HISTORY_MAX_TEXT_BYTES);
    let final_trimmed = truncated.trim();
    if final_trimmed.is_empty() {
        format!("Vídeo {video_id}")
    } else {
        final_trimmed.to_owned()
    }
}

pub(crate) fn sanitize_history_channel(channel: &str) -> String {
    truncate_utf8_bytes(channel.trim(), HISTORY_MAX_TEXT_BYTES)
}

pub(crate) fn format_history_status_i18n(entry: &HistoryEntry, pt: bool) -> String {
    if let Some(resume_ms) = entry.resume_position_ms() {
        let pos_text = format_time(resume_ms as f64 / 1000.0);
        if let Some(dur_ms) = entry.duration_ms {
            let prefix = if pt {
                "↺ Retomar em"
            } else {
                "↺ Resume at"
            };
            format!(
                "{prefix} {pos_text} / {}",
                format_time(dur_ms as f64 / 1000.0)
            )
        } else if pt {
            format!("↺ Retomar em {pos_text}")
        } else {
            format!("↺ Resume at {pos_text}")
        }
    } else if entry.is_near_end() && entry.position_ms > 0 {
        if let Some(dur_ms) = entry.duration_ms {
            let dur_text = format_time(dur_ms as f64 / 1000.0);
            if pt {
                format!("✓ Assistido ({dur_text}) • Recomeça do início")
            } else {
                format!("✓ Watched ({dur_text}) • Starts from beginning")
            }
        } else if pt {
            "✓ Assistido • Recomeça do início".into()
        } else {
            "✓ Watched • Starts from beginning".into()
        }
    } else if let Some(dur_ms) = entry.duration_ms {
        let dur_text = format_time(dur_ms as f64 / 1000.0);
        if pt {
            format!("Duração: {dur_text}")
        } else {
            format!("Duration: {dur_text}")
        }
    } else if pt {
        "No histórico local".into()
    } else {
        "In local history".into()
    }
}

pub(crate) fn encode_utf16_clipboard_words(text: &str) -> Vec<u16> {
    let mut words: Vec<u16> = text.encode_utf16().collect();
    words.push(0);
    words
}

#[cfg(windows)]
pub(crate) fn copy_text_to_os_clipboard(text: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    let words = encode_utf16_clipboard_words(text);
    let byte_len = words
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| "Texto muito grande para a área de transferência.".to_owned())?;

    let mut opened = false;
    for attempt in 0..5 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        if attempt < 4 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    if !opened {
        return Err("Não foi possível acessar a área de transferência do Windows.".into());
    }
    let _guard = ClipboardGuard;

    unsafe {
        if EmptyClipboard() == 0 {
            return Err("Não foi possível limpar a área de transferência do Windows.".into());
        }
        let handle = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if handle.is_null() {
            return Err("Não foi possível alocar memória para a área de transferência.".into());
        }
        let locked = GlobalLock(handle);
        if locked.is_null() {
            let _ = GlobalFree(handle);
            return Err("Não foi possível gravar na área de transferência.".into());
        }
        std::ptr::copy_nonoverlapping(words.as_ptr(), locked.cast::<u16>(), words.len());
        let _ = GlobalUnlock(handle);
        if SetClipboardData(u32::from(CF_UNICODETEXT), handle).is_null() {
            let _ = GlobalFree(handle);
            return Err("Falha ao enviar o texto para a área de transferência.".into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn copy_text_to_os_clipboard(_text: &str) -> Result<(), String> {
    Ok(())
}

pub(crate) fn decode_utf16_clipboard_words(words: &[u16]) -> String {
    let end = words.iter().position(|&w| w == 0).unwrap_or(words.len());
    String::from_utf16_lossy(&words[..end])
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn read_text_from_os_clipboard() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    if unsafe { IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT)) } == 0 {
        return None;
    }
    if unsafe { OpenClipboard(std::ptr::null_mut()) } == 0 {
        return None;
    }
    let _guard = ClipboardGuard;

    unsafe {
        let handle = GetClipboardData(u32::from(CF_UNICODETEXT));
        if handle.is_null() {
            return None;
        }
        let locked = GlobalLock(handle);
        if locked.is_null() {
            return None;
        }
        let byte_size = GlobalSize(handle);
        let max_words = (byte_size / std::mem::size_of::<u16>()).min(16_384);
        let words = std::slice::from_raw_parts(locked.cast::<u16>(), max_words);
        let text = decode_utf16_clipboard_words(words);
        let _ = GlobalUnlock(handle);
        Some(text)
    }
}

#[cfg(not(windows))]
pub(crate) fn read_text_from_os_clipboard() -> Option<String> {
    None
}

pub(crate) fn copy_text_to_clipboard(ctx: &egui::Context, text: &str) -> Result<(), String> {
    ctx.copy_text(text.to_owned());
    copy_text_to_os_clipboard(text)
}

#[derive(Default)]
pub(crate) struct HomeCardActions {
    pub(crate) open_clicked: bool,
    pub(crate) enqueue_clicked: bool,
    pub(crate) toggle_watch_later: bool,
    pub(crate) open_save_modal: bool,
    pub(crate) share_clicked: bool,
    pub(crate) download_format: Option<downloads::DownloadFormat>,
    pub(crate) copy_url: Option<String>,
    pub(crate) open_channel: Option<ChannelTarget>,
    pub(crate) filter_channel: Option<String>,
    pub(crate) mute_channel: Option<String>,
    pub(crate) mute_topic: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActivePlayback {
    pub(crate) video_id: Option<String>,
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) channel: String,
    pub(crate) position_ms: u64,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) resume_target_ms: Option<u64>,
    pub(crate) resume_seek_pending: bool,
    pub(crate) explicit_seek_pending: bool,
    pub(crate) start_option_cleared: bool,
    pub(crate) has_live_sample: bool,
    pub(crate) stale_guard_pos_ms: Option<u64>,
    pub(crate) is_live: bool,
    pub(crate) live_dvr: bool,
    pub(crate) at_live_edge: bool,
    pub(crate) live_paused_since: Option<Instant>,
    pub(crate) live_paused_accumulated_secs: f64,
    pub(crate) live_behind_secs: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ChannelTab {
    #[default]
    Home,
    Videos,
    Shorts,
    Streams,
    Playlists,
    About,
    Search,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BrowseSection {
    #[default]
    Home,
    Explore,
    Subscriptions,
    Shorts,
    History,
    Queue,
    Library,
    Playlists,
    WatchLater,
    LikedVideos,
    Downloads,
    Insights,
    ReportHistory,
    YouTubeLabs,
}

pub(crate) struct SearchFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) result: Result<Vec<SearchResult>, String>,
}

pub(crate) struct ChannelFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) result: Result<search::ChannelFetchOutcome, String>,
}

pub(crate) struct ChannelMoreVideosFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) result: Result<search::ChannelVideosBatch, String>,
}

pub(crate) struct ChannelPlaylistsFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) result: Result<Vec<search::ChannelPlaylist>, String>,
}

pub(crate) struct ChannelPlaylistVideosFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) playlist_id: String,
    pub(crate) result: Result<Vec<SearchResult>, String>,
}

pub(crate) struct ChannelAboutFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) result: Result<search::ChannelAboutInfo, String>,
}

pub(crate) struct ChannelTabVideosFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) tab: search::ChannelFeedTab,
    pub(crate) result: Result<Vec<SearchResult>, String>,
}

pub(crate) struct ChannelSortedVideosFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) sort: search::ChannelVideosSort,
    pub(crate) result: Result<Vec<SearchResult>, String>,
}

pub(crate) struct ChannelSearchFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) query: String,
    pub(crate) result: Result<Vec<SearchResult>, String>,
}

pub(crate) enum HomeFetchMessage {
    Progress(oauth::SubscriptionProgress),
    Finished(
        Option<oauth::TokenSession>,
        Result<search::HomeFeed, String>,
    ),
    SubscriptionProgress {
        request_id: u64,
        progress: oauth::SubscriptionProgress,
    },
    SubscriptionFinished {
        request_id: u64,
        token: Option<oauth::TokenSession>,
        result: Result<oauth::SubscriptionFeedOutput, String>,
    },
    PlaylistProgress {
        batch: Vec<SearchResult>,
        total_results: Option<u64>,
    },
    PlaylistFinished {
        token: Option<oauth::TokenSession>,
        total_results: Option<u64>,
        result: Result<(), String>,
    },
}

pub(crate) enum LikedVideosFetchMessage {
    Progress {
        request_id: u64,
        batch: Vec<SearchResult>,
        total_results: Option<u64>,
    },
    Finished {
        request_id: u64,
        token: Option<oauth::TokenSession>,
        total_results: Option<u64>,
        result: Result<(), String>,
    },
}

pub(crate) struct HomeBatchFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) page: usize,
    pub(crate) result: Result<search::HomeFeed, String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BackupImportMode {
    Replace,
    #[default]
    Merge,
}

#[derive(Clone)]
pub(crate) struct ImportedUserData {
    pub(crate) library: library::Library,
    pub(crate) queue: Queue,
    pub(crate) history: History,
    pub(crate) followed_channels: followed_channels::FollowedChannels,
    pub(crate) playback_preferences: PlaybackPreferences,
    pub(crate) theme: Theme,
    pub(crate) buffer_profile: BufferProfile,
    pub(crate) feed: feed_prefs::FeedPreferences,
    pub(crate) app_preferences: settings::AppPreferences,
}

#[derive(Clone, Debug)]
pub(crate) struct BackupPreview {
    pub(crate) path: PathBuf,
    pub(crate) library_items: usize,
    pub(crate) favorites: usize,
    pub(crate) watch_later: usize,
    pub(crate) collections: usize,
    pub(crate) bookmarks: usize,
    pub(crate) queue_items: usize,
    pub(crate) history_entries: usize,
    pub(crate) followed_channels: usize,
    pub(crate) merge_added: usize,
    pub(crate) merge_conflicts: usize,
    pub(crate) merge_skipped: usize,
    pub(crate) history_progress_updates: usize,
}

#[derive(Default)]
pub(crate) struct FeedDerivedCache {
    pub(crate) profile: UserProfile,
    pub(crate) chips: Vec<FeedFilter>,
    pub(crate) ranked: Vec<ScoredVideo>,
    pub(crate) generation: u64,
    pub(crate) filter: FeedFilter,
    pub(crate) history_len: usize,
    pub(crate) queue_len: usize,
    pub(crate) selected_url: Option<String>,
    pub(crate) pause_recommendations: bool,
    pub(crate) hide_shorts_in_home: bool,
    pub(crate) muted_topics: Vec<String>,
    pub(crate) muted_channels: Vec<String>,
    pub(crate) pinned_topics: Vec<String>,
}

#[cfg(windows)]
pub(crate) struct PlayerTelemetry {
    pub(crate) events: Vec<queue::PlaybackEvent>,
    pub(crate) poll_error: Option<String>,
    pub(crate) paused: bool,
    pub(crate) muted: bool,
    pub(crate) ended: bool,
    pub(crate) seekable: Option<bool>,
    pub(crate) raw_path: Option<String>,
    pub(crate) raw_position: Option<f64>,
    pub(crate) raw_duration: Option<f64>,
    pub(crate) buffered_secs: Option<f64>,
    pub(crate) raw_volume: f64,
    pub(crate) speed: f64,
    pub(crate) current_height: Option<u32>,
    pub(crate) video_size: Option<VideoSize>,
}

#[cfg(windows)]
pub(crate) struct FullscreenBarContext {
    pub(crate) width: f32,
    pub(crate) compact_controls: bool,
    pub(crate) fullscreen: bool,
    pub(crate) theater_mode: bool,
    pub(crate) has_queued: bool,
    pub(crate) position: f64,
    pub(crate) duration: Option<f64>,
    pub(crate) is_live: bool,
    pub(crate) at_live_edge: bool,
    pub(crate) live_behind_secs: f64,
    pub(crate) palette: ThemePalette,
}

#[cfg(windows)]
#[derive(Default)]
pub(crate) struct FullscreenControlsActions {
    pub(crate) close_clicked: bool,
    pub(crate) fullscreen_toggled: bool,
    pub(crate) theater_toggled: bool,
    pub(crate) settings_toggled: bool,
    pub(crate) next_clicked: bool,
    pub(crate) go_live_clicked: bool,
    pub(crate) manual_seek_recorded: Option<f64>,
    pub(crate) flush_on_pause: bool,
    pub(crate) library_changed: bool,
}

pub(crate) struct OverlayPlaybackContext {
    pub(crate) video: Video,
    pub(crate) paused: bool,
    pub(crate) ended: bool,
    pub(crate) muted: bool,
    pub(crate) position: f64,
    pub(crate) duration: Option<f64>,
    pub(crate) buffered_secs: Option<f64>,
    pub(crate) raw_volume: f64,
    pub(crate) speed: f64,
    pub(crate) current_height: Option<u32>,
    pub(crate) selected_quality: Option<u32>,
    pub(crate) qualities: Vec<u32>,
    pub(crate) qualities_loading: bool,
    pub(crate) has_queued: bool,
    pub(crate) has_active_subtitle: bool,
    pub(crate) subtitles: Vec<overlay::OverlaySubtitleOption>,
    pub(crate) subtitles_loading: bool,
    pub(crate) chapters: Vec<overlay::OverlayChapter>,
    pub(crate) theater: bool,
    pub(crate) fullscreen: bool,
    pub(crate) mini_player: bool,
    pub(crate) autoplay: bool,
    pub(crate) loop_video: bool,
    pub(crate) is_live: bool,
    pub(crate) is_live_dvr: bool,
    pub(crate) at_live_edge: bool,
    pub(crate) live_behind_secs: f64,
    pub(crate) raw_path: Option<String>,
    pub(crate) raw_position: Option<f64>,
    pub(crate) now: Instant,
}

pub(crate) struct WatchChannelDisplay<'a> {
    pub(crate) channel_target: Option<&'a ChannelTarget>,
    pub(crate) channel_avatar_url: Option<&'a str>,
    pub(crate) subscriber_count_text: Option<&'a str>,
    pub(crate) content_width: f32,
    pub(crate) stats: &'a [String],
    pub(crate) technical_resolution: Option<&'a str>,
    pub(crate) palette: &'a ThemePalette,
    pub(crate) subscribed: Option<bool>,
    pub(crate) sub_pending: bool,
}

pub(crate) struct WatchMetadataActionState<'a> {
    pub(crate) video_id: Option<&'a str>,
    pub(crate) current_rating: Option<oauth::VideoRating>,
    pub(crate) rating_pending: bool,
    pub(crate) saved_in_library: bool,
    pub(crate) save_menu_open: bool,
    pub(crate) share_recently_copied: bool,
    pub(crate) ryd_stats: Option<&'a community_apis::RydStats>,
    pub(crate) pt: bool,
    pub(crate) palette: &'a ThemePalette,
}

#[derive(Default)]
pub(crate) struct WatchMetadataActionFlags {
    pub(crate) rating_to_apply: Option<Option<oauth::VideoRating>>,
    pub(crate) enqueue_video: bool,
    pub(crate) share_video: bool,
    pub(crate) toggle_save_menu: bool,
    pub(crate) open_transcript: bool,
    pub(crate) open_report_modal: bool,
    pub(crate) download_format: Option<downloads::DownloadFormat>,
}

#[derive(Default)]
pub(crate) struct QueueHeadingActions {
    pub(crate) advance: bool,
    pub(crate) shuffle: bool,
    pub(crate) cycle_repeat: bool,
    pub(crate) deduplicate: bool,
    pub(crate) clear: bool,
}

#[derive(Default)]
pub(crate) struct ChannelHeaderActions {
    pub(crate) back_to_results: bool,
    pub(crate) reload_target: Option<ChannelTarget>,
    pub(crate) force_refresh: bool,
    pub(crate) new_limit: Option<usize>,
}

pub(crate) struct ChannelTabLayout {
    pub(crate) compact: bool,
    pub(crate) card_max_w: f32,
    pub(crate) palette: ThemePalette,
}

pub(crate) struct ShortsWatchDetails<'a> {
    pub(crate) video: &'a Video,
    pub(crate) meta_line: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) channel_target: Option<&'a ChannelTarget>,
    pub(crate) video_id: Option<&'a str>,
    pub(crate) current_rating: Option<oauth::VideoRating>,
    pub(crate) rating_pending: bool,
    pub(crate) subscribed: Option<bool>,
    pub(crate) sub_pending: bool,
    pub(crate) saved_in_library: bool,
    pub(crate) save_menu_open: bool,
    pub(crate) comments_open: bool,
    pub(crate) has_prev_short: bool,
    pub(crate) has_next_short: bool,
    pub(crate) short_index_and_total: Option<(usize, usize)>,
    pub(crate) loop_video: bool,
    pub(crate) share_recently_copied: bool,
}

#[derive(Default)]
pub(crate) struct ShortsWatchActions {
    pub(crate) rating_to_apply: Option<Option<oauth::VideoRating>>,
    pub(crate) toggle_subscribe: bool,
    pub(crate) enqueue_video: bool,
    pub(crate) share_video: bool,
    pub(crate) toggle_save_menu: bool,
    pub(crate) toggle_comments: bool,
    pub(crate) toggle_loop: bool,
    prev_short: bool,
    next_short: bool,
    open_channel_target: Option<ChannelTarget>,
}

pub(crate) struct FastTube {
    pub(crate) video_metadata: HashMap<String, VideoMetadata>,
    pub(crate) selected_metadata: Option<VideoMetadata>,
    pub(crate) thumbnails: ThumbnailCache,
    pub(crate) avatars: thumbnails::AvatarCache,
    pub(crate) message: String,
    pub(crate) toasts: toast::ToastManager,
    pub(crate) selected: Option<Video>,
    pub(crate) scrolling: fastframe_scroll::Scrolling,
    pub(crate) autoscroll: app_state::AutoscrollState,
    pub(crate) in_app_ui_pass: bool,
    #[cfg(windows)]
    pub(crate) player: Option<player::Player>,

    pub(crate) feed: app_state::BrowseFeedState,
    pub(crate) feed_preferences: app_state::FeedPreferencesState,
    pub(crate) channels: app_state::ChannelState,
    pub(crate) auth: app_state::AuthState,
    pub(crate) playback: app_state::PlaybackState,
    pub(crate) settings: app_state::SettingsState,
    pub(crate) history: app_state::HistoryState,
    pub(crate) queue: app_state::QueueState,
    pub(crate) library: app_state::LibraryState,
    pub(crate) followed_channels: app_state::FollowedChannelsState,
    pub(crate) downloads: app_state::DownloadState,
    pub(crate) backup: app_state::BackupState,
    pub(crate) ui: app_state::NavigationUiState,
    pub(crate) watch: app_state::WatchState,
    pub(crate) desktop: desktop::DesktopIntegrations,
}

pub(crate) fn restore_session_state(
    loaded: Result<Option<oauth::TokenSession>, String>,
) -> (Option<oauth::TokenSession>, bool, Option<String>) {
    match loaded {
        Ok(Some(session)) if !session.has_comment_scope() => (
            None,
            true,
            Some("Sua sessão do Google foi criada em uma versão anterior sem permissão para comentários. Entre novamente uma única vez para atualizar o acesso.".to_owned()),
        ),
        Ok(session) => (session, false, None),
        Err(error) => (None, false, Some(error)),
    }
}

impl Default for FastTube {
    fn default() -> Self {
        #[cfg(not(test))]
        let initial_backup_status = Self::recover_interrupted_import();
        #[cfg(test)]
        let initial_backup_status = None;
        let (buffer_profile, settings_path, settings_error) = Self::load_initial_settings();
        let (playback_preferences, playback_settings_path, playback_settings_error) =
            Self::load_initial_playback_settings();
        let loaded_playback_settings_path = playback_settings_path.clone();
        let (theme, theme_settings_path, theme_settings_error) =
            Self::load_initial_theme_settings();
        let loaded_theme_settings_path = theme_settings_path.clone();
        let (app_prefs, app_prefs_path, app_prefs_error) = Self::load_initial_app_preferences();
        let loaded_app_prefs_path = app_prefs_path.clone();
        #[cfg(not(test))]
        if let Ok(effective) =
            settings::effective_cache_dir(app_prefs.cache_dir_override.as_deref())
        {
            search::set_yt_dlp_cache_dir(Some(settings::yt_dlp_cache_subdir(&effective)));
        }
        let (history, history_path, history_read_failed, history_error) =
            Self::load_initial_history();
        let loaded_history_path = history_path.clone();
        #[cfg(not(test))]
        let library_path = library::path().ok();
        #[cfg(test)]
        let library_path: Option<PathBuf> = None;
        #[cfg(not(test))]
        let (library, library_error) = match library_path.as_deref() {
            Some(path) => match library::load(path) {
                Ok(value) => (value, None),
                Err(error) => (
                    library::Library::default(),
                    Some(format!("Biblioteca local indisponível: {error}")),
                ),
            },
            None => (
                library::Library::default(),
                Some("Não foi possível localizar a pasta de dados local.".into()),
            ),
        };
        #[cfg(test)]
        let (library, library_error) = (library::Library::default(), None);
        #[cfg(not(test))]
        let feed_prefs_path = feed_prefs::path().ok();
        #[cfg(test)]
        let feed_prefs_path: Option<PathBuf> = None;
        let feed_prefs = feed_prefs_path
            .as_deref()
            .and_then(|path| feed_prefs::load(path).ok())
            .unwrap_or_default();
        #[cfg(not(test))]
        let queue_path = history::history_path()
            .ok()
            .map(|path| path.with_file_name("queue.json"));
        #[cfg(test)]
        let queue_path: Option<PathBuf> = None;
        let (queue, queue_read_failed, queue_error) = match queue_path.as_deref() {
            Some(path) => match Queue::load(path, DEFAULT_QUEUE_LIMIT) {
                Ok(queue) => (queue, false, None),
                Err(error) => (
                    Queue::new(DEFAULT_QUEUE_LIMIT),
                    true,
                    Some(format!(
                        "Não foi possível ler a fila salva: {error}. O arquivo foi preservado."
                    )),
                ),
            },
            None => (
                Queue::new(DEFAULT_QUEUE_LIMIT),
                true,
                if cfg!(test) {
                    None
                } else {
                    Some("Não foi possível localizar o arquivo da fila.".into())
                },
            ),
        };
        #[cfg(not(test))]
        let (oauth_session, discard_old_session, oauth_restore_error) =
            restore_session_state(oauth::load_session());
        #[cfg(not(test))]
        if discard_old_session {
            let _ = oauth::delete_session();
        }
        #[cfg(test)]
        let (oauth_session, oauth_restore_error): (
            Option<oauth::TokenSession>,
            Option<String>,
        ) = (None, None);
        let session_authenticated = oauth_session.is_some();
        let disk_image_cache =
            Self::disk_image_cache_for_override(app_prefs.cache_dir_override.as_deref());
        #[cfg(not(test))]
        let followed_path = followed_channels::path().ok();
        #[cfg(test)]
        let followed_path: Option<PathBuf> = None;
        #[cfg(not(test))]
        let (followed_data, followed_error) = match followed_path.as_deref() {
            Some(path) => match followed_channels::load(path) {
                Ok(data) => (data, None),
                Err(error) => (
                    followed_channels::FollowedChannels::default(),
                    Some(format!("Canais seguidos locais indisponíveis: {error}")),
                ),
            },
            None => (followed_channels::FollowedChannels::default(), None),
        };
        #[cfg(test)]
        let (followed_data, followed_error) =
            (followed_channels::FollowedChannels::default(), None);
        let mut thumbnails = ThumbnailCache::default();
        let mut avatars = thumbnails::AvatarCache::default();
        thumbnails.set_disk_cache(disk_image_cache.clone());
        avatars.set_disk_cache(disk_image_cache);
        Self {
            video_metadata: HashMap::new(),
            selected_metadata: None,
            thumbnails,
            avatars,
            message: oauth_restore_error.unwrap_or_default(),
            toasts: toast::ToastManager::default(),
            selected: None,
            scrolling: fastframe_scroll::Scrolling::default(),
            autoscroll: app_state::AutoscrollState::default(),
            in_app_ui_pass: false,
            #[cfg(windows)]
            player: None,
            desktop: desktop::DesktopIntegrations::default(),

            feed: app_state::BrowseFeedState {
                query: String::new(),
                search_filters: search::SearchFilters::default(),
                search_chip: search::SearchChip::default(),
                search_filters_open: false,
                search_save_modal_video: None,
                search_report_modal_video: None,
                search_report_reason: None,
                search_report_details: String::new(),
                search_reported_urls: std::collections::HashSet::new(),
                share_modal_video: None,
                share_start_at_enabled: false,
                share_start_at_text: "0:00".to_owned(),
                share_embed_open: false,
                share_targets_scroll_offset: 0.0,
                share_modal_copied_at: None,
                recent_searches: Vec::new(),
                search_suggest: app_state::SearchSuggestState::default(),
                report_history: Vec::new(),
                report_history_filter: 0,
                videos: Vec::new(),
                generation: 0,
                signal_revision: 0,
                metadata_revision: 0,
                filter: FeedFilter::All,
                cache_dirty: true,
                cache: FeedDerivedCache::default(),
                search: app_state::SearchJob {
                    rx: None,
                    cancel: None,
                    next_request_id: 0,
                    active_request_id: None,
                },
                home: app_state::HomeFeedState::default(),
            },
            feed_preferences: app_state::FeedPreferencesState {
                values: feed_prefs,
                path: feed_prefs_path,
            },
            channels: app_state::ChannelState {
                tab: ChannelTab::Home,
                video_limit: search::DEFAULT_CHANNEL_VIDEOS_LIMIT,
                ..app_state::ChannelState::default()
            },
            auth: app_state::AuthState {
                reauth_rx: None,
                reauth_cancel: None,
                authenticated: session_authenticated,
                session: oauth_session,
                ratings: HashMap::new(),
                rating_rx: None,
                rating_cancel: None,
                subscriptions: HashMap::new(),
                sub_check_rx: None,
                sub_check_cancel: None,
                sub_toggle_rx: None,
                sub_toggle_cancel: None,
                my_channel: None,
                my_channel_title: None,
                my_channel_avatar_url: None,
                my_channel_subscribers: None,
                my_channel_open_when_ready: false,
                my_channel_fetch_attempted: false,
                my_channel_rx: None,
                my_channel_cancel: None,
                remote_playlists: Vec::new(),
                remote_playlists_rx: None,
                remote_playlists_cancel: None,
                playlist_action_rx: None,
                playlist_action_cancel: None,
                report_rx: None,
                report_cancel: None,
                youtube_web_session: None,
                youtube_web_session_checked: false,
                restore_attempted: false,
            },
            playback: app_state::PlaybackState {
                autoplay: app_prefs.default_autoplay,
                loop_video: app_prefs.default_loop,
                mini_player_volume: f32::from(app_prefs.default_volume_percent.min(100)),
                diagnostics: DecodingDiagnostics::unknown(Some(
                    app_prefs.hwdec_mode.as_mpv_value(),
                )),
                ..app_state::PlaybackState::default()
            },
            settings: app_state::SettingsState {
                buffer: app_state::BufferSettingsState {
                    profile: buffer_profile,
                    path: settings_path,
                    error: settings_error,
                    notice: None,
                },
                playback: app_state::TrackedSettings {
                    value: playback_preferences,
                    path: playback_settings_path,
                    loaded_path: loaded_playback_settings_path,
                    error: playback_settings_error,
                },
                theme: app_state::TrackedSettings {
                    value: theme,
                    path: theme_settings_path,
                    loaded_path: loaded_theme_settings_path,
                    error: theme_settings_error,
                },
                prefs: app_state::TrackedSettings {
                    value: app_prefs,
                    path: app_prefs_path,
                    loaded_path: loaded_app_prefs_path,
                    error: app_prefs_error,
                },
                ..app_state::SettingsState::default()
            },
            history: app_state::HistoryState {
                data: history,
                path: history_path,
                loaded_path: loaded_history_path,
                read_failed: history_read_failed,
                error: history_error,
                notice: None,
                dirty: false,
                search_query: String::new(),
                filter: history::HistoryFilter::default(),
                save: app_state::HistorySaveState {
                    last_at: None,
                    last_position_ms: None,
                    last_duration_ms: None,
                },
            },
            queue: app_state::QueueState {
                data: queue,
                path: queue_path,
                read_failed: queue_read_failed,
                persistence_error: queue_error,
                ..app_state::QueueState::default()
            },
            library: app_state::LibraryState {
                data: library,
                revision: 0,
                path: library_path,
                error: library_error,
                notice: None,
                bookmark_note: String::new(),
                collection_name: String::new(),
                search_query: String::new(),
                bookmark_filter: String::new(),
                renaming_collection: None,
                rename_draft: String::new(),
                confirm_delete_collection: None,
                editing_bookmark_idx: None,
                editing_bookmark_note: String::new(),
                pending_bookmark_seek: None,
                selected_collection_idx: None,
                active_remote_playlist_title: None,
                playlist_chip_filter: 0,
                detail_chip_filter: 0,
                liked_videos: app_state::LikedVideosState::default(),
            },
            followed_channels: app_state::FollowedChannelsState {
                data: followed_data,
                path: followed_path,
                error: followed_error,
                ..app_state::FollowedChannelsState::default()
            },
            downloads: app_state::DownloadState::default(),
            backup: app_state::BackupState {
                path_text: String::new(),
                status: initial_backup_status,
                confirm_import: false,
                import_mode: BackupImportMode::default(),
                preview: None,
                pending_import: None,
            },
            ui: app_state::NavigationUiState::default(),
            watch: app_state::WatchState::default(),
        }
    }
}

impl FastTube {
    #[cfg(not(test))]
    pub(crate) fn default_user_data_paths() -> io::Result<Vec<(&'static str, PathBuf)>> {
        let history_path = history::history_path()?;
        Ok(vec![
            ("library", library::path()?),
            ("queue", history_path.with_file_name("queue.json")),
            ("history", history_path),
            ("followed", followed_channels::path()?),
            ("playback", settings::playback_path()?),
            ("theme", settings::theme_path()?),
            ("feed", feed_prefs::path()?),
            ("buffer", settings::settings_path()?),
            ("prefs", settings::app_preferences_path()?),
        ])
    }

    #[cfg(not(test))]
    pub(crate) fn recover_interrupted_import() -> Option<String> {
        let paths = Self::default_user_data_paths().ok()?;
        let root = paths.first()?.1.parent()?.to_path_buf();
        let refs: Vec<_> = paths
            .iter()
            .map(|(name, path)| (*name, path.as_path()))
            .collect();
        match backup::recover_pending_import(&root, &refs) {
            Ok(true) => Some(
                "Uma importação interrompida foi desfeita antes da abertura; os dados anteriores foram restaurados."
                    .into(),
            ),
            Ok(false) => None,
            Err(error) => Some(format!(
                "Foi encontrado um ponto de restauração de importação, mas ele não pôde ser aplicado: {error}"
            )),
        }
    }

    pub(crate) fn persist_queue(&mut self) {
        self.queue.persist();
    }

    pub(crate) fn load_initial_settings() -> (BufferProfile, Option<PathBuf>, Option<String>) {
        match settings::settings_path() {
            Ok(path) => match Self::load_settings_from_path(&path) {
                Ok(profile) => (profile, Some(path), None),
                Err(error) => (BufferProfile::default(), Some(path), Some(error)),
            },
            Err(error) => (
                BufferProfile::default(),
                None,
                Some(format!(
                    "Não foi possível localizar o arquivo de configurações ({error}). Usando o perfil Padrão (64/16 MiB, 30s)."
                )),
            ),
        }
    }

    pub(crate) fn load_settings_from_path(path: &Path) -> Result<BufferProfile, String> {
        settings::load(path).map_err(|error| {
            format!(
                "Erro ao ler as configurações em '{}' ({error}). Usando o perfil Padrão (64/16 MiB, 30s).",
                path.display()
            )
        })
    }

    pub(crate) fn load_initial_playback_settings()
    -> (PlaybackPreferences, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (PlaybackPreferences::default(), None, None);
        }
        match settings::playback_path() {
            Ok(path) => match app_state::load_playback_settings_from_path(&path) {
                Ok(preferences) => (preferences, Some(path), None),
                Err(error) => (PlaybackPreferences::default(), Some(path), Some(error)),
            },
            Err(error) => (
                PlaybackPreferences::default(),
                None,
                Some(format!(
                    "Erro ao ler preferências de reprodução ({error}); usando 1× e qualidade automática."
                )),
            ),
        }
    }

    pub(crate) fn ensure_playback_settings_synchronized(&mut self) {
        self.settings.ensure_playback_synchronized();
        let before = self.settings.prefs.loaded_path.clone();
        self.settings.ensure_prefs_synchronized();
        if before != self.settings.prefs.loaded_path {
            self.sync_yt_dlp_cache_dir();
            self.sync_disk_image_cache();
        }
    }

    pub(crate) fn apply_playback_preferences(&mut self, preferences: PlaybackPreferences) {
        self.settings.apply_playback_preferences(preferences);
    }

    pub(crate) fn load_initial_theme_settings() -> (Theme, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (Theme::default(), None, None);
        }
        match settings::theme_path() {
            Ok(path) => match app_state::load_theme_settings_from_path(&path) {
                Ok(theme) => (theme, Some(path), None),
                Err(error) => (Theme::default(), Some(path), Some(error)),
            },
            Err(error) => (
                Theme::default(),
                None,
                Some(format!(
                    "Erro ao ler tema da interface ({error}); usando o tema Escuro padrão."
                )),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn ensure_theme_settings_synchronized(&mut self) {
        self.settings.ensure_theme_synchronized();
    }

    pub(crate) fn sync_theme_visuals(&mut self, ctx: &egui::Context) {
        self.settings.sync_theme_visuals(ctx);
    }

    pub(crate) fn apply_theme(&mut self, theme: Theme) {
        self.settings.apply_theme(theme);
    }

    pub(crate) fn load_initial_app_preferences()
    -> (settings::AppPreferences, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (settings::AppPreferences::default(), None, None);
        }
        match settings::app_preferences_path() {
            Ok(path) => match app_state::load_app_preferences_from_path(&path) {
                Ok(prefs) => (prefs, Some(path), None),
                Err(error) => (settings::AppPreferences::default(), Some(path), Some(error)),
            },
            Err(error) => (
                settings::AppPreferences::default(),
                None,
                Some(format!(
                    "Erro ao ler preferências gerais ({error}); usando os padrões."
                )),
            ),
        }
    }

    pub(crate) fn sync_yt_dlp_cache_dir(&self) {
        if cfg!(test) && self.settings.prefs.value.cache_dir_override.is_none() {
            return;
        }
        if let Ok(effective) =
            settings::effective_cache_dir(self.settings.prefs.value.cache_dir_override.as_deref())
        {
            search::set_yt_dlp_cache_dir(Some(settings::yt_dlp_cache_subdir(&effective)));
        }
    }

    pub(crate) fn disk_image_cache_for_override(
        override_dir: Option<&str>,
    ) -> Option<std::sync::Arc<thumbnails::DiskImageCache>> {
        if cfg!(test) && override_dir.is_none() {
            return None;
        }
        settings::effective_cache_dir(override_dir)
            .ok()
            .map(|effective| {
                std::sync::Arc::new(thumbnails::DiskImageCache::new(
                    effective.join("thumbnails"),
                    thumbnails::DEFAULT_DISK_CACHE_MAX_BYTES,
                ))
            })
            .or_else(|| {
                if cfg!(test) {
                    None
                } else {
                    thumbnails::DiskImageCache::default_production()
                }
            })
    }

    pub(crate) fn sync_disk_image_cache(&mut self) {
        let disk_cache = Self::disk_image_cache_for_override(
            self.settings.prefs.value.cache_dir_override.as_deref(),
        );
        self.thumbnails.set_disk_cache(disk_cache.clone());
        self.avatars.set_disk_cache(disk_cache);
    }

    #[allow(dead_code)]
    pub(crate) fn audio_normalization_af(enabled: bool) -> &'static str {
        if enabled {
            "lavfi=[loudnorm=I=-16:TP=-1.5:LRA=11]"
        } else {
            ""
        }
    }

    #[inline]
    pub(crate) fn ui_pt(&self) -> bool {
        self.settings.prefs.value.language == i18n::UiLanguage::PtBr
    }

    #[inline]
    pub(crate) fn pick<'a>(&self, pt: &'a str, en: &'a str) -> &'a str {
        self.settings.prefs.value.language.pick(pt, en)
    }

    pub(crate) fn is_video_at_eof(&self) -> bool {
        #[cfg(windows)]
        {
            self.player
                .as_ref()
                .is_some_and(|player| player.flag_cstr(c"eof-reached") == Some(true))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    #[inline]
    pub(crate) fn is_watch_page_active(&self) -> bool {
        self.selected.is_some() && !self.playback.in_app_mini_player
    }

    #[inline]
    pub(crate) fn is_in_app_mini_player_active(&self) -> bool {
        self.selected.is_some() && self.playback.in_app_mini_player
    }

    pub(crate) fn has_modal_covering_mini_player(&self) -> bool {
        self.ui.settings_open
            || self.ui.shortcuts_open
            || self.ui.command_palette.open
            || self.feed.search_filters_open
            || self.feed.search_save_modal_video.is_some()
            || self.feed.search_report_modal_video.is_some()
            || self.feed.share_modal_video.is_some()
            || self.channels.about_modal_open
    }

    pub(crate) fn enter_in_app_mini_player(&mut self, ctx: &egui::Context) -> bool {
        if self.selected.is_none() {
            return false;
        }
        if self.playback.fullscreen {
            self.toggle_fullscreen(ctx);
        }
        if self.playback.mini_player {
            self.toggle_mini_player(ctx);
        }
        self.playback.in_app_mini_player = true;
        self.playback.player_menu = None;
        self.playback.player_context_menu = None;
        self.playback.seek_drag = None;
        self.sync_player_window_visibility();
        ctx.request_repaint();
        true
    }

    pub(crate) fn exit_in_app_mini_player(&mut self, ctx: &egui::Context) {
        self.playback.in_app_mini_player = false;
        self.ui.settings_open = false;
        self.ui.navigation_open = false;
        self.close_channel_view();
        self.sync_player_window_visibility();
        ctx.request_repaint();
    }

    pub(crate) fn toggle_in_app_mini_player_playback(&mut self) {
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let ended = player.flag_cstr(c"eof-reached") == Some(true);
            let paused = player.flag_cstr(c"pause") == Some(true);
            let result = if ended {
                player
                    .seek(0.0, "absolute+exact")
                    .and_then(|()| player.set("pause", "no"))
            } else {
                player.set("pause", if paused { "no" } else { "yes" })
            };
            match result {
                Ok(()) => {
                    self.playback.mini_player_paused = if ended { false } else { !paused };
                }
                Err(error) => {
                    self.message = error;
                }
            }
            return;
        }
        self.playback.mini_player_paused = !self.playback.mini_player_paused;
    }

    pub(crate) fn toggle_in_app_mini_player_mute(&mut self) {
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let muted = player
                .flag_cstr(c"mute")
                .unwrap_or(self.playback.mini_player_muted);
            let vol = player
                .double_cstr(c"volume")
                .map(|v| v.clamp(0.0, 100.0) as f32)
                .unwrap_or(self.playback.mini_player_volume);
            let (next_muted, next_vol) = if vol <= 0.5 {
                (false, 50.0)
            } else {
                (!muted, vol)
            };
            let mut result = player.set("mute", if next_muted { "yes" } else { "no" });
            if result.is_ok() && (next_vol - vol).abs() > 0.1 {
                result = player.set("volume", &format!("{next_vol:.0}"));
            }
            match result {
                Ok(()) => {
                    self.playback.mini_player_muted = next_muted;
                    self.playback.mini_player_volume = next_vol;
                }
                Err(error) => {
                    self.message = error;
                }
            }
            return;
        }
        let muted = self.playback.mini_player_muted;
        let vol = self.playback.mini_player_volume;
        if vol <= 0.5 {
            self.playback.mini_player_muted = false;
            self.playback.mini_player_volume = 50.0;
        } else {
            self.playback.mini_player_muted = !muted;
        }
    }

    pub(crate) fn set_in_app_mini_player_volume(&mut self, volume: f32) {
        let clamped = if volume.is_finite() {
            volume.clamp(0.0, 100.0).round()
        } else {
            self.playback.mini_player_volume.clamp(0.0, 100.0)
        };
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let mut result = player.set("volume", &format!("{clamped:.0}"));
            if result.is_ok() && clamped > 0.0 && player.flag_cstr(c"mute") == Some(true) {
                result = player.set("mute", "no");
            }
            match result {
                Ok(()) => {
                    self.playback.mini_player_volume = clamped;
                    if clamped > 0.0 {
                        self.playback.mini_player_muted = false;
                    }
                }
                Err(error) => {
                    self.message = error;
                }
            }
            return;
        }
        self.playback.mini_player_volume = clamped;
        if clamped > 0.0 && self.playback.mini_player_muted {
            self.playback.mini_player_muted = false;
        }
    }

    pub(crate) fn record_video_back_stack(&mut self, previous: Video, incoming_url: &str) {
        if Self::same_video_target(&previous.url, incoming_url) {
            return;
        }
        if self
            .playback
            .video_back_stack
            .last()
            .is_none_or(|top| !Self::same_video_target(&top.url, &previous.url))
        {
            self.playback.video_back_stack.push(previous);
            if self.playback.video_back_stack.len() > 50 {
                self.playback.video_back_stack.remove(0);
            }
        }
        self.playback.video_forward_stack.clear();
    }

    pub(crate) fn previous_history_video_candidate(&self) -> Option<Video> {
        let selected_id = self
            .selected
            .as_ref()
            .and_then(|v| history::youtube_video_id(&v.url));
        for entry in self.history.data.entries() {
            if selected_id.as_deref() == Some(entry.video_id.as_str()) {
                continue;
            }
            let Some(url) = history::youtube_watch_url(&entry.video_id) else {
                continue;
            };
            if self
                .playback
                .video_forward_stack
                .iter()
                .any(|v| Self::same_video_target(&v.url, &url))
            {
                continue;
            }
            if let Some(known) = self
                .feed
                .videos
                .iter()
                .find(|v| Self::same_video_target(&v.url, &url))
            {
                return Some(known.clone());
            }
            return Some(Video {
                title: entry.title.clone(),
                channel: entry.channel.clone(),
                duration: String::new(),
                url,
                thumbnail: format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", entry.video_id),
            });
        }
        None
    }

    pub(crate) fn has_previous_video_target(&self) -> bool {
        !self.playback.video_back_stack.is_empty()
            || (self.is_watch_short_mode() && self.next_or_prev_short(false).is_some())
            || self.previous_history_video_candidate().is_some()
    }

    pub(crate) fn has_next_video_target(&mut self) -> bool {
        !self.playback.video_forward_stack.is_empty()
            || !self.queue.data.is_empty()
            || (self.is_watch_short_mode() && self.next_or_prev_short(true).is_some())
            || self.next_recommended_video().is_some()
    }

    pub(crate) fn trigger_in_app_mini_player_previous(
        &mut self,
        is_live: bool,
        position_secs: f64,
    ) {
        let prev = self
            .playback
            .video_back_stack
            .pop()
            .or_else(|| {
                if self.is_watch_short_mode() {
                    self.next_or_prev_short(false)
                } else {
                    None
                }
            })
            .or_else(|| self.previous_history_video_candidate());
        if let Some(prev_video) = prev {
            if let Some(current) = self.selected.clone()
                && !Self::same_video_target(&current.url, &prev_video.url)
                && self
                    .playback
                    .video_forward_stack
                    .last()
                    .is_none_or(|top| !Self::same_video_target(&top.url, &current.url))
            {
                self.playback.video_forward_stack.push(current);
                if self.playback.video_forward_stack.len() > 50 {
                    self.playback.video_forward_stack.remove(0);
                }
            }
            self.playback.pending_open_keep_mini_player = true;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(prev_video);
        } else if !is_live && position_secs > 1.0 {
            self.seek_to_seconds(0.0);
        }
    }

    pub(crate) fn trigger_in_app_mini_player_next(&mut self) {
        if let Some(next_video) = self.playback.video_forward_stack.pop() {
            if let Some(current) = self.selected.clone()
                && !Self::same_video_target(&current.url, &next_video.url)
                && self
                    .playback
                    .video_back_stack
                    .last()
                    .is_none_or(|top| !Self::same_video_target(&top.url, &current.url))
            {
                self.playback.video_back_stack.push(current);
                if self.playback.video_back_stack.len() > 50 {
                    self.playback.video_back_stack.remove(0);
                }
            }
            self.playback.pending_open_keep_mini_player = true;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(next_video);
        } else if !self.queue.data.is_empty() {
            self.playback.pending_open_suppress_back_stack = false;
            self.queue.pending_advance = true;
        } else if self.is_watch_short_mode()
            && let Some(next_video) = self.next_or_prev_short(true)
        {
            self.playback.pending_open_keep_mini_player = true;
            self.playback.pending_open_suppress_back_stack = false;
            self.playback.pending_open_video = Some(next_video);
        } else if let Some(next_video) = self.next_recommended_video() {
            self.playback.pending_open_keep_mini_player = true;
            self.playback.pending_open_suppress_back_stack = false;
            self.playback.pending_open_video = Some(next_video);
        }
    }

    pub(crate) fn maybe_enter_auto_mini_player(&mut self, ctx: &egui::Context) -> bool {
        if self.is_in_app_mini_player_active() {
            return true;
        }
        if self.selected.is_some()
            && self.settings.prefs.value.auto_mini_player
            && !self.playback.mini_player
            && !self.is_video_at_eof()
        {
            return self.enter_in_app_mini_player(ctx);
        }
        false
    }

    pub(crate) fn apply_app_preferences(&mut self, next: settings::AppPreferences) {
        let prev = self.settings.prefs.value.clone();
        self.settings.apply_app_preferences(next);
        self.sync_yt_dlp_cache_dir();
        if prev.cache_dir_override != self.settings.prefs.value.cache_dir_override {
            self.sync_disk_image_cache();
        }
        if prev.hide_shorts_in_home != self.settings.prefs.value.hide_shorts_in_home
            || prev.pause_recommendations != self.settings.prefs.value.pause_recommendations
            || prev.muted_topics != self.settings.prefs.value.muted_topics
            || prev.muted_channels != self.settings.prefs.value.muted_channels
            || prev.pinned_topics != self.settings.prefs.value.pinned_topics
        {
            self.invalidate_feed_cache();
        }
        #[cfg(windows)]
        if prev.audio_normalization != self.settings.prefs.value.audio_normalization
            && let Some(player) = &self.player
        {
            let _ = player.apply_audio_filters(
                self.settings.prefs.value.audio_normalization,
                self.playback.audio_eq,
            );
        }
        if self.selected.is_none() {
            self.reset_decoding_diagnostics();
        }
    }

    pub(crate) fn remember_current_player_volume(&mut self) {
        if !self.settings.prefs.value.remember_volume {
            return;
        }
        #[cfg(windows)]
        {
            let Some(player) = &self.player else {
                return;
            };
            if let Some(vol) = player.number("volume") {
                let clamped = vol.round().clamp(0.0, 100.0) as u8;
                if clamped != self.settings.prefs.value.default_volume_percent {
                    let mut prefs = self.settings.prefs.value.clone();
                    prefs.default_volume_percent = clamped;
                    self.apply_app_preferences(prefs);
                }
            }
        }
    }

    pub(crate) fn maybe_auto_activate_subtitle(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        {
            if !self.settings.prefs.value.auto_subtitles
                || self.playback.auto_subtitle_attempted
                || self.selected.is_none()
                || !self.watch.extras.subtitles_loaded
            {
                return;
            }
            self.playback.auto_subtitle_attempted = true;
            let (has_active, _) = self.collect_overlay_subtitles();
            if has_active {
                return;
            }
            let pref = self
                .settings
                .prefs
                .value
                .preferred_subtitle_lang
                .trim()
                .to_ascii_lowercase()
                .replace('_', "-");
            let pref = if pref.is_empty() { "pt" } else { pref.as_str() };
            let matches_exact_or_region = |lang: &str| {
                let lower = lang.trim().to_ascii_lowercase().replace('_', "-");
                lower == pref
                    || lower
                        .strip_prefix(pref)
                        .is_some_and(|rest| rest.starts_with('-'))
            };
            let base_pref = pref.split_once('-').map(|(base, _)| base);
            let matches_base = |lang: &str| {
                let Some(base) = base_pref else {
                    return false;
                };
                lang.trim().to_ascii_lowercase().replace('_', "-") == base
            };
            let subs = &self.watch.extras.subtitles;
            let chosen = subs
                .iter()
                .filter(|t| !t.automatic && matches_exact_or_region(&t.language))
                .chain(
                    subs.iter()
                        .filter(|t| !t.automatic && matches_base(&t.language)),
                )
                .chain(
                    subs.iter()
                        .filter(|t| t.automatic && matches_exact_or_region(&t.language)),
                )
                .chain(
                    subs.iter()
                        .filter(|t| t.automatic && matches_base(&t.language)),
                )
                .next()
                .cloned();
            if let Some(track) = chosen {
                self.select_overlay_subtitle(Some(&format!("yt:{}", track.url)), ctx);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = ctx;
        }
    }

    pub(crate) fn seek_step_secs(&self) -> u32 {
        match self.settings.prefs.value.seek_step_secs {
            5 | 10 | 15 | 30 => self.settings.prefs.value.seek_step_secs,
            _ => 10,
        }
    }

    pub(crate) fn has_pending_hwdec_restart(&self) -> bool {
        self.playback
            .active_hwdec
            .is_some_and(|active| active != self.settings.prefs.value.hwdec_mode)
    }

    pub(crate) fn palette(&self) -> ThemePalette {
        self.settings.palette()
    }

    pub(crate) fn overlay_style(&self) -> overlay::OverlayStyle {
        self.settings.overlay_style()
    }

    pub(crate) fn load_initial_history() -> (History, Option<PathBuf>, bool, Option<String>) {
        match history::history_path() {
            Ok(path) => match app_state::load_history_from_path(&path) {
                Ok(loaded) => (loaded, Some(path), false, None),
                Err(error) => (History::default(), Some(path), true, Some(error)),
            },
            Err(error) => (
                History::default(),
                None,
                false,
                Some(format!(
                    "Não foi possível localizar o arquivo de histórico ({error}). O histórico funcionará apenas nesta sessão."
                )),
            ),
        }
    }

    pub(crate) fn ensure_history_synchronized(&mut self) {
        if self.history.ensure_synchronized() {
            self.invalidate_feed_cache();
        }
    }

    pub(crate) fn save_history_to_disk(&mut self, now: Instant) -> bool {
        let (saved, invalidated) = self
            .history
            .save_to_disk(now, self.playback.active.as_ref());
        if invalidated {
            self.invalidate_feed_cache();
        }
        saved
    }

    pub(crate) fn is_video_known_live(&self, video: &Video) -> bool {
        is_live_duration_label(&video.duration)
            || self
                .metadata_for(&video.url)
                .is_some_and(|meta| meta.is_live)
            || self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.url == video.url && active.is_live)
            || self.watch.live.info.as_ref().is_some_and(|info| {
                info.is_live && self.watch.live.video_url.as_deref() == Some(video.url.as_str())
            })
    }

    pub(crate) fn active_video_is_live(&self) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|video| self.is_video_known_live(video))
            || self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.is_live)
    }

    pub(crate) fn planned_resume_ms(&mut self, video: &Video) -> Option<u64> {
        if !self.settings.prefs.value.resume_enabled || self.is_video_known_live(video) {
            return None;
        }
        let (resume, invalidated) = self.history.planned_resume_ms(video);
        if invalidated {
            self.invalidate_feed_cache();
        }
        resume
    }

    pub(crate) fn record_active_playback_in_memory(&mut self) -> bool {
        if self.settings.prefs.value.pause_watch_history {
            return false;
        }
        let Some(active) = &mut self.playback.active else {
            return false;
        };
        if !active.has_live_sample {
            return false;
        }
        let Some(video_id) = active.video_id.clone() else {
            return false;
        };
        if !active.is_live
            && let Some(duration_ms) = active.duration_ms
        {
            if duration_ms == 0 {
                active.duration_ms = None;
            } else if active.position_ms > duration_ms {
                active.position_ms = duration_ms;
            }
        }
        let (record_position_ms, record_duration_ms) = if active.is_live {
            (0, None)
        } else {
            (active.position_ms, active.duration_ms)
        };
        let sanitized_title = sanitize_history_title(&active.title, &video_id);
        let sanitized_channel = sanitize_history_channel(&active.channel);
        let watched_at_ms = self
            .history
            .data
            .entries()
            .first()
            .filter(|existing| {
                existing.video_id == video_id
                    && existing.title == sanitized_title
                    && existing.channel == sanitized_channel
                    && existing.position_ms == record_position_ms
                    && existing.duration_ms == record_duration_ms
            })
            .and_then(|existing| existing.watched_at_ms)
            .or_else(|| Some(history::current_unix_epoch_ms()));
        let entry = HistoryEntry {
            video_id: video_id.clone(),
            title: sanitized_title,
            channel: sanitized_channel,
            position_ms: record_position_ms,
            duration_ms: record_duration_ms,
            watched_at_ms,
        };
        match self.history.data.record_active_tick(entry) {
            Ok(outcome) => {
                if outcome.changed {
                    self.history.dirty = true;
                }
                if outcome.ranking_changed {
                    self.invalidate_feed_cache();
                }
                outcome.changed
            }
            Err(_) => false,
        }
    }

    pub(crate) fn should_persist_playback_now(&self, now: Instant, force: bool) -> bool {
        self.history
            .should_persist_now(self.playback.active.as_ref(), now, force)
    }

    pub(crate) fn flush_playback_history_at(&mut self, now: Instant) {
        self.ensure_history_synchronized();
        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, true) {
            self.save_history_to_disk(now);
        }
    }

    pub(crate) fn activate_video_playback(
        &mut self,
        video: &Video,
        resume_target_ms: Option<u64>,
        previous_player_pos_ms: Option<u64>,
        now: Instant,
    ) {
        self.ensure_history_synchronized();
        let video_id = history::youtube_video_id(&video.url);
        let is_live = self.is_video_known_live(video);
        let live_dvr = self
            .watch
            .live
            .info
            .as_ref()
            .filter(|info| {
                info.is_live && self.watch.live.video_url.as_deref() == Some(video.url.as_str())
            })
            .is_some_and(|info| info.is_live_dvr);
        let duration_ms = if is_live {
            None
        } else {
            let fallback_duration_ms = parse_duration_ms(&video.duration);
            let existing_duration_ms = video_id
                .as_deref()
                .and_then(|id| self.history.data.get(id))
                .and_then(|entry| entry.duration_ms);
            existing_duration_ms.or(fallback_duration_ms)
        };
        let effective_resume_ms = if is_live { None } else { resume_target_ms };
        let initial_position_ms = effective_resume_ms.unwrap_or(0);
        self.playback.active = Some(ActivePlayback {
            video_id: video_id.clone(),
            url: video.url.clone(),
            title: video.title.clone(),
            channel: video.channel.clone(),
            position_ms: initial_position_ms,
            duration_ms,
            resume_target_ms: effective_resume_ms,
            resume_seek_pending: effective_resume_ms.is_some(),
            explicit_seek_pending: false,
            start_option_cleared: false,
            has_live_sample: false,
            stale_guard_pos_ms: previous_player_pos_ms,
            is_live,
            live_dvr,
            at_live_edge: is_live,
            live_paused_since: None,
            live_paused_accumulated_secs: 0.0,
            live_behind_secs: 0.0,
        });
        self.history.save.last_position_ms = None;
        self.history.save.last_duration_ms = None;
        if video_id.is_some() {
            self.history.notice = None;
        }
        let _ = now;
    }

    pub(crate) fn update_live_playback_state(
        &mut self,
        paused: bool,
        raw_position: Option<f64>,
        raw_duration: Option<f64>,
        seekable: Option<bool>,
        now: Instant,
    ) {
        let Some(active) = &mut self.playback.active else {
            return;
        };
        let live_info_for_active = self
            .watch
            .live
            .info
            .as_ref()
            .filter(|_| self.watch.live.video_url.as_deref() == Some(active.url.as_str()));
        let metadata_live = self
            .video_metadata
            .get(&active.url)
            .is_some_and(|meta| meta.is_live)
            || self
                .selected
                .as_ref()
                .filter(|v| v.url == active.url)
                .is_some_and(|v| is_live_duration_label(&v.duration));

        if (metadata_live || live_info_for_active.is_some_and(|info| info.is_live))
            && !active.is_live
        {
            active.is_live = true;
            active.at_live_edge = true;
        }
        if !active.is_live {
            active.live_dvr = false;
            active.at_live_edge = false;
            active.live_paused_since = None;
            active.live_paused_accumulated_secs = 0.0;
            active.live_behind_secs = 0.0;
            return;
        }

        active.live_dvr = match live_info_for_active.filter(|info| info.is_live) {
            Some(info) => info.is_live_dvr,
            None => seekable == Some(true) && raw_duration.is_some_and(|dur| dur > 30.0),
        };

        if paused {
            if active.live_paused_since.is_none() {
                active.live_paused_since = Some(now);
            }
        } else if let Some(since) = active.live_paused_since.take() {
            active.live_paused_accumulated_secs +=
                now.saturating_duration_since(since).as_secs_f64();
        }

        let paused_total_secs = active.live_paused_accumulated_secs
            + active.live_paused_since.map_or(0.0, |since| {
                now.saturating_duration_since(since).as_secs_f64()
            });

        if !active.live_dvr {
            if paused_total_secs >= 2.0 {
                active.at_live_edge = false;
                active.live_behind_secs = paused_total_secs;
            } else if active.at_live_edge {
                active.live_behind_secs = 0.0;
            } else {
                active.live_behind_secs = paused_total_secs.max(2.0);
            }
            return;
        }

        let (dvr_lag_secs, near_dvr_head) = match (raw_position, raw_duration) {
            (Some(pos), Some(dur)) if pos.is_finite() && dur.is_finite() && dur > 0.0 => {
                let lag = (dur - pos).max(0.0);
                let near = (pos / dur) >= 0.985 || lag <= 4.0;
                (lag, near)
            }
            _ => (0.0, false),
        };

        if active.at_live_edge {
            let drifted_far =
                raw_duration.is_some_and(|dur| dur > 20.0) && !near_dvr_head && dvr_lag_secs > 15.0;
            if paused_total_secs >= 2.0 || drifted_far {
                active.at_live_edge = false;
                active.live_behind_secs = dvr_lag_secs.max(paused_total_secs);
            } else {
                active.live_behind_secs = 0.0;
            }
        } else if near_dvr_head && paused_total_secs < 2.0 {
            active.at_live_edge = true;
            active.live_paused_accumulated_secs = 0.0;
            active.live_behind_secs = 0.0;
        } else {
            active.live_behind_secs = dvr_lag_secs.max(paused_total_secs);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_live_metadata_to_video(
        &mut self,
        video_url: &str,
        is_live: bool,
        is_live_dvr: Option<bool>,
        title: Option<&str>,
        channel: Option<&str>,
        channel_id: Option<&str>,
        channel_url: Option<&str>,
        channel_avatar_url: Option<&str>,
        subscriber_count_text: Option<&str>,
        description: Option<&str>,
        concurrent_view_count: Option<u64>,
        ctx: Option<&egui::Context>,
    ) {
        let mut changed = false;
        let meta = self.video_metadata.entry(video_url.to_owned()).or_default();
        if is_live && !meta.is_live {
            meta.is_live = true;
            changed = true;
        }
        if meta
            .channel_id
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(id) = channel_id.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.channel_id = Some(id.to_owned());
            changed = true;
        }
        if meta
            .channel_url
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(url) = channel_url.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.channel_url = Some(url.to_owned());
            changed = true;
        }
        if meta
            .channel_avatar_url
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(url) = channel_avatar_url.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.channel_avatar_url = Some(url.to_owned());
            changed = true;
        }
        if meta
            .subscriber_count_text
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(subs) = subscriber_count_text
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            meta.subscriber_count_text = Some(subs.to_owned());
            changed = true;
        }
        if meta
            .description
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(desc) = description.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.description = Some(desc.to_owned());
            changed = true;
        }
        if let Some(viewers) = concurrent_view_count
            && meta.view_count != Some(viewers)
        {
            meta.view_count = Some(viewers);
            changed = true;
        }
        let cloned_meta = meta.clone();

        if let Some(av) = channel_avatar_url.map(str::trim).filter(|s| !s.is_empty())
            && let Some(ch) = channel.map(str::trim).filter(|s| !s.is_empty())
        {
            let key = recommendations::normalize_channel_key(ch);
            if !key.is_empty() {
                self.feed
                    .home
                    .channel_avatars
                    .entry(key)
                    .or_insert_with(|| av.to_owned());
            }
        }

        if let Some(selected) = &mut self.selected
            && selected.url == video_url
        {
            if let Some(t) = title.map(str::trim).filter(|s| !s.is_empty())
                && (selected.title.trim().is_empty()
                    || selected.title == selected.url
                    || selected.title.starts_with("http"))
            {
                selected.title = t.to_owned();
                changed = true;
            }
            if let Some(ch) = channel.map(str::trim).filter(|s| !s.is_empty()) {
                let current_ch = selected.channel.trim();
                let looks_like_id = current_ch.is_empty()
                    || (current_ch.starts_with("UC")
                        && current_ch.len() >= 20
                        && !current_ch.contains(' '))
                    || channel_id.is_some_and(|id| id == current_ch);
                if looks_like_id {
                    selected.channel = ch.to_owned();
                    changed = true;
                }
            }
            if is_live && !is_live_duration_label(&selected.duration) {
                selected.duration = "Ao vivo".to_owned();
                changed = true;
            }
            self.selected_metadata = Some(cloned_meta.clone());
        }

        if let Some(active) = &mut self.playback.active
            && active.url == video_url
        {
            if is_live && !active.is_live {
                active.is_live = true;
                active.at_live_edge = true;
                active.resume_target_ms = None;
                active.resume_seek_pending = false;
            }
            if let Some(dvr) = is_live_dvr {
                active.live_dvr = dvr;
            }
            if let Some(selected) = &self.selected
                && selected.url == video_url
            {
                active.title = selected.title.clone();
                active.channel = selected.channel.clone();
            }
        }

        if changed {
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            self.invalidate_feed_cache();
        }

        if let Some(ctx) = ctx
            && self.auth.authenticated
            && self.auth.sub_check_rx.is_none()
            && self.selected.as_ref().is_some_and(|v| v.url == video_url)
            && let Some(target) = search::channel_target_for_metadata(&cloned_meta)
            && self
                .subscription_status_for(Some(&target), cloned_meta.channel_id.as_deref())
                .is_none()
        {
            self.start_check_subscription(target, cloned_meta.channel_id, ctx);
        }
    }

    pub(crate) fn go_live_edge(&mut self, video: &Video) {
        #[cfg(windows)]
        if let Some(player) = &self.player {
            if let Err(error) = player.quality(
                &video.url,
                self.playback.quality,
                &self.settings.playback.value.format(),
                false,
            ) {
                self.message = error;
                return;
            }
            self.playback.cached_raw_path_poll = None;
            let _ = player.set("pause", "no");
        }
        #[cfg(not(windows))]
        let _ = video;
        if let Some(active) = &mut self.playback.active
            && active.url == video.url
        {
            active.at_live_edge = true;
            active.live_paused_since = None;
            active.live_paused_accumulated_secs = 0.0;
            active.live_behind_secs = 0.0;
            active.resume_target_ms = None;
            active.resume_seek_pending = false;
            active.has_live_sample = false;
        }
        self.playback.overlay_last_activity = Instant::now();
    }

    pub(crate) fn start_watch_live_for_selected(&mut self, video: &Video, ctx: &egui::Context) {
        let video_id = history::youtube_video_id(&video.url);
        self.watch
            .live
            .reset_for_video(Some(&video.url), video_id.clone());
        if let Some(id) = video_id {
            let (rx, cancel) = live_chat::start_watch_live_fetch(
                self.watch.live.session_id,
                id,
                self.auth.youtube_web_session.clone(),
                ctx,
            );
            self.watch.live.watch_info_loading = true;
            self.watch.live.watch_rx = Some(rx);
            self.watch.live.watch_cancel = Some(cancel);
        }
    }

    pub(crate) fn start_live_chat_stream_if_needed(&mut self, ctx: &egui::Context) {
        let continuation = self.watch.live.active_continuation.clone().or_else(|| {
            self.watch
                .live
                .info
                .as_ref()
                .and_then(|info| info.initial_continuation.clone())
        });
        let Some(cont) = continuation else {
            return;
        };
        self.watch.live.stop_chat_stream();
        self.watch.live.active_continuation = Some(cont.clone());
        self.watch.live.chat_loading = true;
        self.watch.live.chat_error = None;
        let version = if self.watch.live.client_version.is_empty() {
            comment_actions::FALLBACK_CLIENT_VERSION.to_owned()
        } else {
            self.watch.live.client_version.clone()
        };
        let (rx, cancel) = live_chat::start_live_chat_stream(
            self.watch.live.session_id,
            self.watch.live.chat_stream_id,
            cont,
            version,
            self.watch.live.visitor_data.clone(),
            self.auth.youtube_web_session.clone(),
            ctx,
        );
        self.watch.live.stream_rx = Some(rx);
        self.watch.live.stream_cancel = Some(cancel);
    }

    pub(crate) fn switch_live_chat_mode(&mut self, mode_index: usize, ctx: &egui::Context) {
        let Some(mode) = self.watch.live.chat_modes.get(mode_index).cloned() else {
            return;
        };
        for (idx, item) in self.watch.live.chat_modes.iter_mut().enumerate() {
            item.selected = idx == mode_index;
        }
        self.watch.live.messages.clear();
        self.watch.live.seen_ids.clear();
        self.watch.live.auto_scroll = true;
        self.watch.live.active_continuation = Some(mode.continuation);
        self.start_live_chat_stream_if_needed(ctx);
    }

    pub(crate) fn send_live_chat_draft(
        &mut self,
        allow_interactive_login: bool,
        ctx: &egui::Context,
    ) {
        let Some(send_params) = self.watch.live.send_params.clone() else {
            self.watch.live.send_error =
                Some("O envio de mensagens ainda não está disponível neste chat.".into());
            return;
        };
        let text = self.watch.live.composer_draft.trim().to_owned();
        if text.is_empty() {
            self.watch.live.send_error = Some("Digite uma mensagem para enviar.".into());
            return;
        }
        if let Some(cancel) = self.watch.live.send_cancel.take() {
            cancel.cancel();
        }
        self.watch.live.sending = true;
        self.watch.live.send_error = None;
        let version = if self.watch.live.client_version.is_empty() {
            comment_actions::FALLBACK_CLIENT_VERSION.to_owned()
        } else {
            self.watch.live.client_version.clone()
        };
        let (rx, cancel) = live_chat::start_send_live_chat_message(
            self.watch.live.session_id,
            self.auth.youtube_web_session.clone(),
            allow_interactive_login,
            send_params,
            text,
            version,
            self.watch.live.visitor_data.clone(),
            ctx,
        );
        self.watch.live.send_rx = Some(rx);
        self.watch.live.send_cancel = Some(cancel);
    }

    pub(crate) fn poll_live_watch(&mut self, ctx: &egui::Context) {
        let watch_outcome = self.watch.live.watch_rx.as_ref().map(Receiver::try_recv);
        match watch_outcome {
            Some(Ok(live_chat::WatchLiveEvent::WebSessionUpdated(sess))) => {
                self.auth.youtube_web_session = Some(sess);
                self.auth.youtube_web_session_checked = true;
                // Drain WatchInfo if queued right after WebSessionUpdated.
                if let Some(Ok(live_chat::WatchLiveEvent::WatchInfo { session_id, result })) =
                    self.watch.live.watch_rx.as_ref().map(Receiver::try_recv)
                {
                    self.handle_watch_live_info_result(session_id, *result, ctx);
                }
            }
            Some(Ok(live_chat::WatchLiveEvent::WatchInfo { session_id, result })) => {
                self.handle_watch_live_info_result(session_id, *result, ctx);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.watch.live.watch_info_loading = false;
                self.watch.live.watch_rx = None;
                self.watch.live.watch_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }

        let mut stream_events = Vec::new();
        let mut stream_disconnected = false;
        if let Some(rx) = &self.watch.live.stream_rx {
            loop {
                match rx.try_recv() {
                    Ok(ev) => stream_events.push(ev),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        stream_disconnected = true;
                        break;
                    }
                }
            }
        }
        for ev in stream_events {
            match ev {
                live_chat::LiveChatStreamEvent::WebSessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                live_chat::LiveChatStreamEvent::Batch {
                    session_id,
                    stream_id,
                    result,
                } => {
                    if session_id != self.watch.live.session_id
                        || stream_id != self.watch.live.chat_stream_id
                    {
                        continue;
                    }
                    self.watch.live.chat_loading = false;
                    match result {
                        Ok(batch) => {
                            self.watch.live.chat_error = None;
                            self.watch.live.apply_batch(batch);
                        }
                        Err(err) => {
                            self.watch.live.chat_error = Some(err);
                        }
                    }
                }
            }
        }
        if stream_disconnected && self.watch.live.stream_rx.is_some() {
            self.watch.live.stream_rx = None;
            self.watch.live.stream_cancel = None;
            self.watch.live.chat_loading = false;
        }

        let send_outcome = self.watch.live.send_rx.as_ref().map(Receiver::try_recv);
        match send_outcome {
            Some(Ok(ev)) => {
                if ev.auth_expired {
                    self.auth.youtube_web_session = None;
                } else if let Some(sess) = ev.web_session {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                if ev.session_id == self.watch.live.session_id {
                    self.watch.live.sending = false;
                    self.watch.live.send_rx = None;
                    self.watch.live.send_cancel = None;
                    match ev.result {
                        Ok(msgs) => {
                            self.watch.live.composer_draft.clear();
                            self.watch.live.send_error = None;
                            for msg in msgs {
                                self.watch.live.push_message(msg);
                            }
                        }
                        Err(err) => {
                            self.watch.live.send_error = Some(err);
                        }
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.watch.live.sending = false;
                self.watch.live.send_rx = None;
                self.watch.live.send_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn is_playback_metadata_loading(&self) -> bool {
        self.playback.quality_rx.is_some()
            || (self.watch.live.watch_info_loading && self.playback.qualities.is_empty())
    }

    pub(crate) fn start_quality_metadata_fallback(&mut self, url: String, ctx: &egui::Context) {
        if let Some(cancel) = self.playback.quality_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        let (sender, receiver) = mpsc::channel();
        self.playback.quality_cancel = Some(cancel.clone());
        self.playback.quality_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(search::playback_metadata_with_cancel(&url, &cancel));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn maybe_start_deferred_quality_fallback(&mut self, ctx: &egui::Context) {
        if self.playback.quality_rx.is_some() || !self.playback.current_file_loaded {
            return;
        }
        let Some(url) = self.playback.quality_fallback_url.take() else {
            return;
        };
        if self.selected.as_ref().is_some_and(|v| v.url == url) {
            self.start_quality_metadata_fallback(url, ctx);
        }
    }

    pub(crate) fn prune_session_caches(&mut self) {
        if self.video_metadata.len() > 250 {
            let mut keep_urls = HashSet::new();
            if let Some(selected) = &self.selected {
                keep_urls.insert(selected.url.clone());
            }
            for video in &self.feed.videos {
                keep_urls.insert(video.url.clone());
            }
            for video in self.queue.data.videos() {
                keep_urls.insert(video.url.clone());
            }
            for video in &self.playback.video_back_stack {
                keep_urls.insert(video.url.clone());
            }
            for video in &self.playback.video_forward_stack {
                keep_urls.insert(video.url.clone());
            }
            if let Some(page) = &self.channels.page {
                for item in &page.videos {
                    keep_urls.insert(item.video.url.clone());
                }
                if let Some(featured) = &page.featured_video {
                    keep_urls.insert(featured.video.url.clone());
                }
                for shelf in &page.home_shelves {
                    for item in &shelf.items {
                        keep_urls.insert(item.video.url.clone());
                    }
                }
            }
            for item in &self.channels.videos.sorted_items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.shorts.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.streams.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.search.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.playlist_detail.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.library.liked_videos.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.followed_channels.feed_items {
                keep_urls.insert(item.video.url.clone());
            }
            self.video_metadata.retain(|url, _| keep_urls.contains(url));
            let min_cap = self.video_metadata.len().max(64);
            if self.video_metadata.capacity() > min_cap.saturating_mul(4) {
                self.video_metadata.shrink_to(min_cap);
            }
        }
        if self.feed.home.channel_avatars.len() > 300 {
            let mut keep_channels = HashSet::new();
            for ch in &self.feed.home.subscription_channels {
                let key = recommendations::normalize_channel_key(&ch.title);
                if !key.is_empty() {
                    keep_channels.insert(key);
                }
            }
            for video in &self.feed.videos {
                let key = recommendations::normalize_channel_key(&video.channel);
                if !key.is_empty() {
                    keep_channels.insert(key);
                }
            }
            if let Some(selected) = &self.selected {
                let key = recommendations::normalize_channel_key(&selected.channel);
                if !key.is_empty() {
                    keep_channels.insert(key);
                }
            }
            if let Some(page) = &self.channels.page {
                if let Some(title) = page.title.as_deref() {
                    let key = recommendations::normalize_channel_key(title);
                    if !key.is_empty() {
                        keep_channels.insert(key);
                    }
                }
                for item in &page.videos {
                    let key = recommendations::normalize_channel_key(&item.video.channel);
                    if !key.is_empty() {
                        keep_channels.insert(key);
                    }
                }
            }
            self.feed
                .home
                .channel_avatars
                .retain(|k, _| keep_channels.contains(k));
            let min_cap = self.feed.home.channel_avatars.len().max(64);
            if self.feed.home.channel_avatars.capacity() > min_cap.saturating_mul(4) {
                self.feed.home.channel_avatars.shrink_to(min_cap);
            }
        }
    }

    pub(crate) fn handle_watch_live_info_result(
        &mut self,
        session_id: u64,
        result: Result<live_chat::WatchLiveInfo, String>,
        ctx: &egui::Context,
    ) {
        if session_id != self.watch.live.session_id {
            return;
        }
        self.watch.live.watch_info_loading = false;
        self.watch.live.watch_rx = None;
        self.watch.live.watch_cancel = None;
        match result {
            Ok(info) => {
                self.watch.live.watch_info_error = None;
                if !info.client_version.is_empty() {
                    self.watch.live.client_version = info.client_version.clone();
                }
                if info.visitor_data.is_some() {
                    self.watch.live.visitor_data = info.visitor_data.clone();
                }
                if !info.modes.is_empty() {
                    self.watch.live.chat_modes = info.modes.clone();
                }
                if self.watch.live.active_continuation.is_none() {
                    self.watch.live.active_continuation = info.initial_continuation.clone();
                }
                self.watch.live.info_revision = self.watch.live.info_revision.wrapping_add(1);
                let video_url = self.watch.live.video_url.clone();
                let is_live = info.is_live;
                let is_live_dvr = info.is_live_dvr;
                let title = info.title.clone();
                let channel = info.channel.clone();
                let channel_id = info.channel_id.clone();
                let channel_url = info.channel_url.clone();
                let channel_avatar_url = info.channel_avatar_url.clone();
                let subscriber_count_text = info.subscriber_count_text.clone();
                let description = info.description.clone();
                let concurrent_view_count = info.concurrent_view_count;
                let heights = info.heights.clone();
                let subtitles = info.subtitles.clone();
                let subtitles_parsed = info.subtitles_parsed;
                let storyboard = info.storyboard.clone();
                self.watch.live.info = Some(info);
                if !heights.is_empty() {
                    self.playback.qualities = heights.clone();
                    self.playback.quality_error = None;
                    self.playback.quality_fallback_url = None;
                }
                if storyboard.is_some() {
                    self.playback.storyboard = storyboard.clone();
                    self.playback.storyboard_video_url = video_url.clone();
                }
                if subtitles_parsed {
                    self.watch
                        .extras
                        .apply_preloaded_subtitles(subtitles.clone());
                    self.watch.overlay_cache.mpv_subs_last_poll = None;
                    self.watch.overlay_cache.force_refresh = true;
                }
                if let Some(url) = video_url.as_deref() {
                    search::prime_playback_metadata_cache(
                        url,
                        &heights,
                        &subtitles,
                        subtitles_parsed,
                        search::PlaybackExtraInfo {
                            is_live,
                            title: title.clone(),
                            channel: channel.clone(),
                            channel_id: channel_id.clone(),
                            channel_url: channel_url.clone(),
                            description: description.clone(),
                            concurrent_view_count,
                            storyboard: storyboard.clone(),
                        },
                    );
                    self.apply_live_metadata_to_video(
                        url,
                        is_live,
                        Some(is_live_dvr),
                        title.as_deref(),
                        channel.as_deref(),
                        channel_id.as_deref(),
                        channel_url.as_deref(),
                        channel_avatar_url.as_deref(),
                        subscriber_count_text.as_deref(),
                        description.as_deref(),
                        concurrent_view_count,
                        Some(ctx),
                    );
                }
                if is_live && self.watch.live.has_chat() && !self.watch.live.chat_user_toggled {
                    self.watch.live.chat_visible = true;
                }
                if self.watch.live.chat_visible && self.watch.live.stream_rx.is_none() {
                    self.start_live_chat_stream_if_needed(ctx);
                }
                if heights.is_empty()
                    && self.playback.qualities.is_empty()
                    && self.playback.quality_rx.is_none()
                    && let Some(url) = video_url
                {
                    self.playback.quality_fallback_url = Some(url);
                    self.maybe_start_deferred_quality_fallback(ctx);
                }
            }
            Err(err) => {
                self.watch.live.watch_info_error = Some(err);
                if self.playback.qualities.is_empty()
                    && self.playback.quality_rx.is_none()
                    && let Some(url) = self.watch.live.video_url.clone()
                {
                    self.playback.quality_fallback_url = Some(url);
                    self.maybe_start_deferred_quality_fallback(ctx);
                }
            }
        }
    }

    pub(crate) fn current_player_live_pos_ms(&self) -> Option<u64> {
        self.playback
            .active
            .as_ref()
            .filter(|active| active.has_live_sample)
            .map(|active| active.position_ms)
    }

    #[cfg(windows)]
    pub(crate) fn sample_player_before_transition(&mut self, now: Instant) {
        let Some(active_video) = self.selected.clone() else {
            return;
        };
        let Some((raw_path, raw_pos, raw_dur, ended)) = self.player.as_ref().map(|player| {
            (
                player.property("path"),
                player.number("time-pos").filter(|&pos| pos >= 0.0),
                player.number("duration").filter(|&dur| dur > 0.0),
                player.property("eof-reached").as_deref() == Some("yes"),
            )
        }) else {
            return;
        };
        if raw_pos.is_some() {
            let _ = self.process_player_telemetry(
                &active_video,
                raw_path.as_deref(),
                raw_pos,
                raw_dur,
                ended,
                now,
            );
        }
    }

    pub(crate) fn process_player_telemetry(
        &mut self,
        video: &Video,
        raw_path: Option<&str>,
        raw_position: Option<f64>,
        raw_duration: Option<f64>,
        ended: bool,
        now: Instant,
    ) -> (f64, Option<f64>, Option<f64>, bool) {
        self.ensure_history_synchronized();
        if self
            .playback
            .active
            .as_ref()
            .is_none_or(|active| active.url != video.url)
        {
            let prev_pos = self
                .playback
                .active
                .as_ref()
                .filter(|active| active.has_live_sample)
                .map(|active| active.position_ms);
            self.flush_playback_history_at(now);
            let resume_ms = self.planned_resume_ms(video);
            self.activate_video_playback(video, resume_ms, prev_pos, now);
        }

        let path_matches = raw_path.is_some_and(|path| {
            let trimmed = path.trim();
            trimmed == video.url
                || (history::youtube_video_id(trimmed).is_some()
                    && history::youtube_video_id(trimmed) == history::youtube_video_id(&video.url))
        });

        let Some(active) = &mut self.playback.active else {
            return (0.0, None, None, false);
        };

        if !path_matches {
            return (
                active.position_ms as f64 / 1000.0,
                active.duration_ms.map(|dur| dur as f64 / 1000.0),
                None,
                false,
            );
        }

        let Some(pos_secs) = raw_position.filter(|pos| pos.is_finite() && *pos >= 0.0) else {
            return (
                active.position_ms as f64 / 1000.0,
                active.duration_ms.map(|dur| dur as f64 / 1000.0),
                None,
                false,
            );
        };

        let mut sample_pos_ms = (pos_secs * 1000.0).round() as u64;
        let sample_dur_ms = raw_duration
            .filter(|dur| dur.is_finite() && *dur > 0.0)
            .map(|dur| (dur * 1000.0).round() as u64)
            .filter(|&dur| dur > 0);

        if !active.has_live_sample
            && let Some(stale_ms) = active.stale_guard_pos_ms
        {
            let expected_ms = active.resume_target_ms.unwrap_or(0);
            if stale_ms.abs_diff(expected_ms) > 2_000 && sample_pos_ms.abs_diff(stale_ms) <= 1_500 {
                return (
                    active.position_ms as f64 / 1000.0,
                    active.duration_ms.map(|dur| dur as f64 / 1000.0),
                    None,
                    false,
                );
            }
        }

        active.stale_guard_pos_ms = None;
        active.has_live_sample = true;
        let mut clear_start_option = false;
        if !active.start_option_cleared {
            active.start_option_cleared = true;
            clear_start_option = true;
        }
        if let Some(dur_ms) = sample_dur_ms {
            active.duration_ms = Some(dur_ms);
        }

        let mut pending_seek_secs = None;
        if active.is_live {
            active.resume_target_ms = None;
            active.resume_seek_pending = false;
            active.explicit_seek_pending = false;
        } else if active.resume_seek_pending {
            active.resume_seek_pending = false;
            if let Some(target_ms) = active.resume_target_ms {
                if !active.explicit_seek_pending
                    && history::should_resume(target_ms, active.duration_ms).is_none()
                {
                    active.resume_target_ms = None;
                    sample_pos_ms = 0;
                    if pos_secs > 1.0 {
                        pending_seek_secs = Some(0.0);
                    }
                } else if active.explicit_seek_pending {
                    active.explicit_seek_pending = false;
                    if target_ms == 0 {
                        if pos_secs > 1.0 {
                            pending_seek_secs = Some(0.0);
                        }
                    } else if sample_pos_ms.abs_diff(target_ms) > 1_000 {
                        pending_seek_secs = Some(target_ms as f64 / 1000.0);
                    }
                } else if sample_pos_ms + 1_000 < target_ms {
                    pending_seek_secs = Some(target_ms as f64 / 1000.0);
                }
            } else {
                active.explicit_seek_pending = false;
            }
        }

        if let Some(target_ms) = active.resume_target_ms {
            if sample_pos_ms.saturating_add(1_000) < target_ms {
                return (
                    active.position_ms as f64 / 1000.0,
                    active.duration_ms.map(|dur| dur as f64 / 1000.0),
                    pending_seek_secs,
                    clear_start_option,
                );
            }
            active.resume_target_ms = None;
        }

        if ended
            && !active.is_live
            && let Some(dur_ms) = active.duration_ms
        {
            sample_pos_ms = sample_pos_ms.max(dur_ms);
        }
        if let Some(dur_ms) = active.duration_ms {
            sample_pos_ms = sample_pos_ms.min(dur_ms);
        }
        active.position_ms = sample_pos_ms;
        let effective_pos = active.position_ms as f64 / 1000.0;
        let effective_dur = active.duration_ms.map(|dur| dur as f64 / 1000.0);

        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, ended) {
            self.save_history_to_disk(now);
        }

        (
            effective_pos,
            effective_dur,
            pending_seek_secs,
            clear_start_option,
        )
    }

    pub(crate) fn record_manual_seek(
        &mut self,
        position_secs: f64,
        duration_secs: Option<f64>,
        now: Instant,
    ) {
        if !position_secs.is_finite() {
            return;
        }
        let Some(active) = &mut self.playback.active else {
            return;
        };
        if let Some(dur_secs) = duration_secs.filter(|dur| dur.is_finite() && *dur > 0.0) {
            let dur_ms = (dur_secs * 1000.0).round() as u64;
            if dur_ms > 0 {
                active.duration_ms = Some(dur_ms);
            }
        }
        let mut pos_ms = (position_secs.max(0.0) * 1000.0).round() as u64;
        if let Some(dur_ms) = active.duration_ms {
            pos_ms = pos_ms.min(dur_ms);
        }
        active.position_ms = pos_ms;
        active.has_live_sample = true;
        active.resume_target_ms = None;
        active.resume_seek_pending = false;
        active.explicit_seek_pending = false;
        if active.is_live
            && let Some(dur_ms) = active.duration_ms.filter(|&d| d > 0)
        {
            let ratio = pos_ms as f64 / dur_ms as f64;
            if ratio >= 0.985 {
                active.at_live_edge = true;
                active.live_behind_secs = 0.0;
            } else {
                active.at_live_edge = false;
                active.live_behind_secs = (dur_ms.saturating_sub(pos_ms) as f64) / 1000.0;
            }
            active.live_paused_accumulated_secs = 0.0;
        }
        self.desktop.notify_seeked(pos_ms as f64 / 1000.0);
        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, true) {
            self.save_history_to_disk(now);
        }
    }

    #[cfg(test)]
    pub(crate) fn record_video_in_history(
        &mut self,
        video: &Video,
        position_ms: u64,
        duration_ms: Option<u64>,
    ) -> io::Result<()> {
        if self.settings.prefs.value.pause_watch_history {
            return Ok(());
        }
        let (result, invalidated) = self.history.record_video(video, position_ms, duration_ms);
        if invalidated {
            self.invalidate_feed_cache();
        }
        result
    }

    pub(crate) fn clear_history(&mut self) {
        self.ensure_history_synchronized();
        self.history.data.clear();
        self.invalidate_feed_cache();
        self.history.dirty = false;
        self.history.save.last_position_ms = None;
        self.history.save.last_duration_ms = None;
        if let Some(active) = &mut self.playback.active {
            active.resume_target_ms = None;
            active.resume_seek_pending = false;
        }
        if self.history.read_failed {
            self.history.notice = Some(
                "Histórico da sessão limpo. O arquivo corrompido em disco foi preservado sem sobrescrita."
                    .into(),
            );
            return;
        }
        if self.history.path.is_some() {
            if self.save_history_to_disk(Instant::now()) {
                self.history.notice = Some("Histórico local limpo.".into());
            }
        } else {
            self.history.notice = Some("Histórico local limpo.".into());
        }
    }

    pub(crate) fn remove_history_entry(&mut self, video_id: &str) -> bool {
        self.ensure_history_synchronized();
        if !self.history.data.remove(video_id) {
            return false;
        }
        self.invalidate_feed_cache();
        if !self.history.read_failed && self.history.path.is_some() {
            self.save_history_to_disk(Instant::now());
        }
        true
    }

    pub(crate) fn video_for_history_entry(&self, entry: &HistoryEntry) -> Option<Video> {
        let canonical_url = history::youtube_watch_url(&entry.video_id)?;
        if let Some(existing) = self.feed.videos.iter().find(|video| {
            history::youtube_video_id(&video.url).as_deref() == Some(entry.video_id.as_str())
        }) {
            return Some(existing.clone());
        }
        if let Some(channel_video) = self.channels.page.as_ref().and_then(|page| {
            page.videos
                .iter()
                .find(|item| {
                    history::youtube_video_id(&item.video.url).as_deref()
                        == Some(entry.video_id.as_str())
                })
                .map(|item| item.video.clone())
        }) {
            return Some(channel_video);
        }
        if let Some(playlist_video) = self
            .channels
            .playlist_detail
            .items
            .iter()
            .find(|item| {
                history::youtube_video_id(&item.video.url).as_deref()
                    == Some(entry.video_id.as_str())
            })
            .map(|item| item.video.clone())
        {
            return Some(playlist_video);
        }
        let duration = entry
            .duration_ms
            .map(|ms| format_time(ms as f64 / 1000.0))
            .unwrap_or_default();
        Some(Video {
            title: entry.title.clone(),
            channel: entry.channel.clone(),
            url: canonical_url,
            thumbnail: String::new(),
            duration,
        })
    }

    pub(crate) fn apply_buffer_profile(&mut self, profile: BufferProfile) {
        let pending_restart = self
            .playback
            .active_buffer
            .is_some_and(|active| active.values() != profile.values());
        if self.settings.apply_buffer_profile(profile, pending_restart) {
            self.invalidate_feed_cache();
        }
    }

    pub(crate) fn has_pending_buffer_restart(&self) -> bool {
        self.playback
            .active_buffer
            .is_some_and(|active| active.values() != self.settings.buffer.profile.values())
    }

    pub(crate) fn reset_decoding_diagnostics(&mut self) {
        self.playback.diagnostics =
            DecodingDiagnostics::unknown(Some(self.settings.prefs.value.hwdec_mode.as_mpv_value()));
        self.playback.last_diagnostics_poll_at = None;
    }

    #[cfg(test)]
    pub(crate) fn refresh_decoding_diagnostics_with<F>(
        &mut self,
        now: Instant,
        has_active_video: bool,
        read_property: F,
    ) -> bool
    where
        F: FnMut(&str) -> Option<String>,
    {
        update_decoding_diagnostics(
            &mut self.playback.diagnostics,
            &mut self.playback.last_diagnostics_poll_at,
            self.ui.settings_open,
            has_active_video,
            now,
            read_property,
        )
    }

    pub(crate) fn refresh_player_diagnostics(&mut self, now: Instant) -> bool {
        if self.selected.is_none() {
            self.reset_decoding_diagnostics();
            return false;
        }
        #[cfg(windows)]
        if let Some(player) = &self.player {
            return update_decoding_diagnostics(
                &mut self.playback.diagnostics,
                &mut self.playback.last_diagnostics_poll_at,
                self.ui.settings_open,
                true,
                now,
                |name| player.property(name),
            );
        }
        let _ = now;
        false
    }

    pub(crate) fn should_show_video_window(&self) -> bool {
        if self.desktop.hidden_to_tray {
            return false;
        }
        #[cfg(windows)]
        if self
            .player
            .as_ref()
            .is_some_and(player::Player::is_render_api)
        {
            return self.selected.is_some();
        }
        if self.is_in_app_mini_player_active()
            && (self.has_modal_covering_mini_player() || self.ui.navigation_open)
        {
            return false;
        }
        !self.playback.audio_only
            && should_show_video_window(self.selected.is_some(), self.ui.settings_open)
    }

    pub(crate) fn sync_player_window_visibility(&mut self) {
        let visible = self.should_show_video_window();
        self.playback.window_visible = visible;
        #[cfg(windows)]
        if let Some(player) = &mut self.player {
            player.set_visible(visible);
        }
    }

    pub(crate) fn metadata_for(&self, url: &str) -> Option<&VideoMetadata> {
        self.video_metadata
            .get(url)
            .or_else(|| {
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|selected| selected.url == url)
                {
                    self.selected_metadata.as_ref()
                } else {
                    None
                }
            })
            .or_else(|| {
                self.channels.page.as_ref().and_then(|page| {
                    page.videos
                        .iter()
                        .find(|item| item.video.url == url)
                        .map(|item| &item.metadata)
                })
            })
            .or_else(|| {
                self.channels
                    .playlist_detail
                    .items
                    .iter()
                    .find(|item| item.video.url == url)
                    .map(|item| &item.metadata)
            })
    }

    pub(crate) fn channel_target_for_video(&self, video: &Video) -> Option<ChannelTarget> {
        self.metadata_for(&video.url)
            .and_then(search::channel_target_for_metadata)
    }

    pub(crate) fn is_channel_loading(&self) -> bool {
        self.channels.is_loading()
    }

    pub(crate) fn is_channel_more_videos_loading(&self) -> bool {
        self.channels.is_more_videos_loading()
    }

    pub(crate) fn is_channel_playlists_loading(&self) -> bool {
        self.channels.is_playlists_loading()
    }

    pub(crate) fn is_channel_playlist_videos_loading(&self) -> bool {
        self.channels.is_playlist_videos_loading()
    }

    pub(crate) fn is_channel_about_loading(&self) -> bool {
        self.channels.is_about_loading()
    }

    pub(crate) fn cancel_channel_subrequests(&mut self) {
        self.channels.cancel_subrequests();
    }

    pub(crate) fn reset_channel_tabs_state(&mut self, ctx: &egui::Context) {
        self.cancel_channel_subrequests();
        for item in &self.channels.shorts.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.streams.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.videos.sorted_items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.search.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for pl in &self.channels.playlists.items {
            let thumb = &pl.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.playlist_detail.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channels.tab = ChannelTab::Home;
        self.channels.about_modal_open = false;
        self.channels.videos.more_error = None;
        self.channels.videos.has_more = false;
        self.channels.videos.reached_max = false;
        self.channels.videos.sort = search::ChannelVideosSort::Latest;
        self.channels.videos.sorted_items.clear();
        self.channels.videos.sorted_error = None;
        self.channels.shorts.items.clear();
        self.channels.shorts.loaded = false;
        self.channels.shorts.error = None;
        self.channels.streams.items.clear();
        self.channels.streams.loaded = false;
        self.channels.streams.error = None;
        self.channels.playlists.items.clear();
        self.channels.playlists.loaded = false;
        self.channels.playlists.error = None;
        self.channels.playlist_detail.selected = None;
        self.channels.playlist_detail.items.clear();
        self.channels.playlist_detail.loaded = false;
        self.channels.playlist_detail.error = None;
        self.channels.about.info = None;
        self.channels.about.loaded = false;
        self.channels.about.error = None;
        self.channels.search = app_state::ChannelSearchState::default();
    }

    pub(crate) fn cancel_channel_loading(&mut self) {
        self.channels.cancel_loading();
    }

    pub(crate) fn current_channel_target(&self) -> Option<ChannelTarget> {
        self.channels.current_target()
    }

    pub(crate) fn apply_search_results(&mut self, ctx: &egui::Context, results: Vec<SearchResult>) {
        self.message = if results.is_empty() {
            "Nenhum vídeo encontrado.".into()
        } else {
            String::new()
        };
        if self.channels.view_open {
            for previous in &self.feed.videos {
                let thumb = &previous.thumbnail;
                if !thumb.is_empty()
                    && !results.iter().any(|item| &item.video.thumbnail == thumb)
                    && !self.channels.page.as_ref().is_some_and(|page| {
                        page.videos
                            .iter()
                            .any(|item| &item.video.thumbnail == thumb)
                            || page
                                .featured_video
                                .as_ref()
                                .is_some_and(|item| &item.video.thumbnail == thumb)
                            || page.home_shelves.iter().any(|shelf| {
                                shelf
                                    .items
                                    .iter()
                                    .any(|item| &item.video.thumbnail == thumb)
                            })
                    })
                    && !self
                        .channels
                        .videos
                        .sorted_items
                        .iter()
                        .any(|item| &item.video.thumbnail == thumb)
                    && !self
                        .channels
                        .search
                        .items
                        .iter()
                        .any(|item| &item.video.thumbnail == thumb)
                    && !self
                        .channels
                        .playlist_detail
                        .items
                        .iter()
                        .any(|item| &item.video.thumbnail == thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
        }
        let channel_videos_len = self
            .channels
            .page
            .as_ref()
            .map_or(0, |page| page.videos.len());
        let mut videos = Vec::with_capacity(results.len());
        let mut metadata_map = HashMap::with_capacity(
            results.len()
                + self.queue.data.len()
                + channel_videos_len
                + self.channels.videos.sorted_items.len()
                + self.channels.search.items.len()
                + self.channels.playlist_detail.items.len()
                + self.library.liked_videos.items.len(),
        );
        for item in results {
            if self.ui.search_results_mode
                && self.feed.search_reported_urls.contains(&item.video.url)
            {
                continue;
            }
            if (item.metadata.kind == search::SearchResultKind::Playlist
                || item.video.url.contains("playlist?list="))
                && !self.feed.home.playlist_urls.contains(&item.video.url)
            {
                self.feed.home.playlist_urls.push(item.video.url.clone());
            }
            let avatar_key = recommendations::normalize_channel_key(&item.video.channel);
            if !avatar_key.is_empty()
                && let Some(avatar_url) = item
                    .metadata
                    .channel_avatar_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
            {
                self.feed
                    .home
                    .channel_avatars
                    .insert(avatar_key, avatar_url.to_owned());
            }
            metadata_map.insert(item.video.url.clone(), item.metadata);
            videos.push(item.video);
        }
        if !self.channels.view_open {
            self.thumbnails
                .retain_for_videos(ctx, &self.feed.videos, &videos);
        }
        if let Some(page) = &self.channels.page {
            for item in &page.videos {
                metadata_map
                    .entry(item.video.url.clone())
                    .or_insert_with(|| item.metadata.clone());
            }
            if let Some(featured) = &page.featured_video {
                metadata_map
                    .entry(featured.video.url.clone())
                    .or_insert_with(|| featured.metadata.clone());
            }
            for shelf in &page.home_shelves {
                for item in &shelf.items {
                    metadata_map
                        .entry(item.video.url.clone())
                        .or_insert_with(|| item.metadata.clone());
                }
            }
        }
        for item in &self.channels.videos.sorted_items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for item in &self.channels.search.items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for item in &self.channels.playlist_detail.items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for item in &self.library.liked_videos.items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for queued in self.queue.data.videos() {
            if !metadata_map.contains_key(&queued.url)
                && let Some(existing) = self.video_metadata.get(&queued.url)
            {
                metadata_map.insert(queued.url.clone(), existing.clone());
            }
        }
        if let Some(selected) = &self.selected {
            if let Some(found) = metadata_map.get(&selected.url) {
                self.selected_metadata = Some(found.clone());
            } else if let Some(existing) = &self.selected_metadata {
                metadata_map.insert(selected.url.clone(), existing.clone());
            }
        }
        self.feed.videos = videos;
        self.feed.generation = self.feed.generation.wrapping_add(1);
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.video_metadata = metadata_map;
        self.feed.cache_dirty = true;
    }

    pub(crate) fn invalidate_feed_cache(&mut self) {
        self.feed.cache_dirty = true;
        self.feed.signal_revision = self.feed.signal_revision.wrapping_add(1);
    }

    pub(crate) fn ensure_feed_cache(&mut self) {
        let history_len = self.history.data.entries().len();
        let queue_len = self.queue.data.len();
        let selected_url = self.selected.as_ref().map(|video| video.url.clone());
        let pause_recommendations = self.settings.prefs.value.pause_recommendations;
        let muted_topics = &self.settings.prefs.value.muted_topics;
        let muted_channels = &self.settings.prefs.value.muted_channels;
        let pinned_topics = &self.settings.prefs.value.pinned_topics;
        let hide_shorts_in_home = self.settings.prefs.value.hide_shorts_in_home
            && self.ui.browse_section == BrowseSection::Home
            && !self.ui.search_results_mode
            && !matches!(self.feed.filter, FeedFilter::Shorts);
        let cache_is_current = self.feed.cache.generation == self.feed.generation
            && self.feed.cache.filter == self.feed.filter
            && self.feed.cache.history_len == history_len
            && self.feed.cache.queue_len == queue_len
            && self.feed.cache.selected_url == selected_url
            && self.feed.cache.pause_recommendations == pause_recommendations
            && self.feed.cache.hide_shorts_in_home == hide_shorts_in_home
            && self.feed.cache.muted_topics == *muted_topics
            && self.feed.cache.muted_channels == *muted_channels
            && self.feed.cache.pinned_topics == *pinned_topics;
        if !self.feed.cache_dirty && cache_is_current {
            return;
        }
        let mut profile = if pause_recommendations {
            UserProfile::default()
        } else {
            UserProfile::build(&self.history.data, &self.queue.data, self.selected.as_ref())
        };
        if !muted_topics.is_empty() {
            profile.apply_muted_topics(muted_topics);
        }
        if !muted_channels.is_empty() {
            profile.apply_muted_channels(muted_channels);
        }
        self.feed.cache.chips = RecommendationEngine::available_chips_with_pinned(
            &self.feed.videos,
            &self.video_metadata,
            &profile,
            pinned_topics,
        );
        self.feed.cache.ranked = RecommendationEngine::rank_videos(
            &self.feed.videos,
            &self.video_metadata,
            &profile,
            &self.feed.filter,
        );
        if hide_shorts_in_home {
            self.feed.cache.ranked.retain(|item| !item.is_short);
        }
        self.feed.cache.profile = profile;
        self.feed.cache.generation = self.feed.generation;
        self.feed.cache.filter = self.feed.filter.clone();
        self.feed.cache.history_len = history_len;
        self.feed.cache.queue_len = queue_len;
        self.feed.cache.selected_url = selected_url;
        self.feed.cache.pause_recommendations = pause_recommendations;
        self.feed.cache.hide_shorts_in_home = hide_shorts_in_home;
        self.feed.cache.muted_topics = muted_topics.clone();
        self.feed.cache.muted_channels = muted_channels.clone();
        self.feed.cache.pinned_topics = pinned_topics.clone();
        self.feed.cache_dirty = false;
    }

    pub(crate) fn cache_channel_avatar(&mut self, channel_name: &str, avatar_url: Option<&str>) {
        let key = recommendations::normalize_channel_key(channel_name);
        if key.is_empty() {
            return;
        }
        if let Some(url) = avatar_url.map(str::trim).filter(|url| !url.is_empty()) {
            self.feed.home.channel_avatars.insert(key, url.to_owned());
        }
    }

    pub(crate) fn populate_channel_avatars_from_subscriptions(&mut self) {
        let pairs: Vec<(String, String)> = self
            .feed
            .home
            .subscription_channels
            .iter()
            .filter_map(|ch| {
                let url = ch.avatar_url.as_deref()?.trim();
                if url.is_empty() {
                    return None;
                }
                let key = recommendations::normalize_channel_key(&ch.title);
                (!key.is_empty()).then(|| (key, url.to_owned()))
            })
            .collect();
        for (key, url) in pairs {
            self.feed.home.channel_avatars.insert(key, url);
        }
    }

    pub(crate) fn lookup_channel_avatar(&self, channel_name: &str) -> Option<&str> {
        let key = recommendations::normalize_channel_key(channel_name);
        if key.is_empty() {
            return None;
        }
        self.feed.home.channel_avatars.get(&key).map(String::as_str)
    }

    pub(crate) fn apply_channel_page(&mut self, ctx: &egui::Context, page: ChannelPage) {
        if let Some(title) = page.title.as_deref() {
            self.cache_channel_avatar(title, page.avatar_url.as_deref());
        }
        if let Some(previous_page) = &self.channels.page {
            let page_keeps_thumb = |thumb: &str| {
                page.videos.iter().any(|v| v.video.thumbnail == thumb)
                    || page
                        .featured_video
                        .as_ref()
                        .is_some_and(|v| v.video.thumbnail == thumb)
                    || page
                        .home_shelves
                        .iter()
                        .any(|s| s.items.iter().any(|v| v.video.thumbnail == thumb))
            };
            for old_item in &previous_page.videos {
                let thumb = &old_item.video.thumbnail;
                if !thumb.is_empty()
                    && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    && !page_keeps_thumb(thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
            if let Some(old_featured) = &previous_page.featured_video {
                let thumb = &old_featured.video.thumbnail;
                if !thumb.is_empty()
                    && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    && !page_keeps_thumb(thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
            for shelf in &previous_page.home_shelves {
                for old_item in &shelf.items {
                    let thumb = &old_item.video.thumbnail;
                    if !thumb.is_empty()
                        && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                        && !page_keeps_thumb(thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
            }
        }
        for item in &page.videos {
            self.video_metadata
                .insert(item.video.url.clone(), item.metadata.clone());
        }
        if let Some(featured) = &page.featured_video {
            self.video_metadata
                .insert(featured.video.url.clone(), featured.metadata.clone());
        }
        for shelf in &page.home_shelves {
            for item in &shelf.items {
                self.video_metadata
                    .insert(item.video.url.clone(), item.metadata.clone());
            }
        }
        if let Some(selected) = &self.selected
            && let Some(found) = self.video_metadata.get(&selected.url)
        {
            self.selected_metadata = Some(found.clone());
        }
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        let effective_limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        let loaded_count = page.videos.len();
        self.channels.videos.reached_max = loaded_count >= search::MAX_CHANNEL_VIDEOS_LIMIT;
        self.channels.videos.has_more =
            !self.channels.videos.reached_max && loaded_count >= effective_limit;
        self.channels.videos.more_error = None;
        self.channels.videos.sort = search::ChannelVideosSort::Latest;
        self.channels.videos.sorted_items.clear();
        self.channels.videos.sorted_error = None;
        if page.country.is_some()
            || page.joined_date_text.is_some()
            || !page.links.is_empty()
            || page.view_count.is_some()
            || page.view_count_text.is_some()
        {
            let info = search::ChannelAboutInfo {
                channel_id: page.channel_id.clone(),
                title: page.title.clone(),
                handle: page.handle.clone(),
                canonical_url: Some(page.canonical_url.clone()),
                description: page.description.clone(),
                subscriber_count: page.subscriber_count,
                view_count: page.view_count,
                subscriber_count_text: page.subscriber_count_text.clone(),
                video_count_text: page.video_count_text.clone(),
                view_count_text: page.view_count_text.clone(),
                country: page.country.clone(),
                joined_date_text: page.joined_date_text.clone(),
                links: page.links.clone(),
                avatar_url: page.avatar_url.clone(),
                banner_url: page.banner_url.clone(),
                verified: page.verified,
                tags: page.tags.clone(),
            };
            self.channels.about.info = Some(info);
            if page.country.is_some() || page.joined_date_text.is_some() {
                self.channels.about.loaded = true;
            }
        }
        let target = page.target.clone();
        let channel_id = page.channel_id.clone();
        self.channels.last_target = Some(target.clone());
        self.channels.error = None;
        self.channels.page = Some(page);
        if self.auth.authenticated {
            self.start_check_subscription(target, channel_id, ctx);
        }
    }

    pub(crate) fn begin_channel_request(
        &mut self,
        target: ChannelTarget,
        ctx: &egui::Context,
    ) -> Option<(u64, ChannelTarget)> {
        let Some(valid_target) = search::parse_channel_target(&target.canonical_url()) else {
            self.channels.error = Some(
                "Não foi possível abrir o canal: identificador ou URL do canal inválido ou não resolvido com segurança."
                    .into(),
            );
            return None;
        };
        let is_different_channel = self.channels.page.as_ref().is_some_and(|prev| {
            prev.target != valid_target && prev.canonical_url != valid_target.canonical_url()
        }) || (self.channels.page.is_none()
            && self.channels.last_target.as_ref() != Some(&valid_target));
        self.cancel_channel_subrequests();
        self.channels.next_request_id = self.channels.next_request_id.wrapping_add(1);
        let request_id = self.channels.next_request_id;
        self.channels.videos.active_request_id = Some(request_id);
        self.channels.loading_target = Some(valid_target.clone());
        self.channels.last_target = Some(valid_target.clone());
        self.channels.error = None;
        self.channels.videos.more_error = None;
        self.channels.view_open = true;
        self.ui.settings_open = false;
        self.sync_player_window_visibility();
        if is_different_channel {
            if let Some(previous_page) = &self.channels.page {
                for item in &previous_page.videos {
                    let thumb = &item.video.thumbnail;
                    if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
                if let Some(featured) = &previous_page.featured_video {
                    let thumb = &featured.video.thumbnail;
                    if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
                for shelf in &previous_page.home_shelves {
                    for item in &shelf.items {
                        let thumb = &item.video.thumbnail;
                        if !thumb.is_empty()
                            && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                        {
                            ctx.forget_image(thumb);
                        }
                    }
                }
                self.channels.page = None;
            }
            self.reset_channel_tabs_state(ctx);
        }
        Some((request_id, valid_target))
    }

    pub(crate) fn open_channel(&mut self, target: ChannelTarget, ctx: &egui::Context) -> bool {
        self.open_channel_with_options(target, ctx, false)
    }

    pub(crate) fn open_channel_with_options(
        &mut self,
        target: ChannelTarget,
        ctx: &egui::Context,
        force_refresh: bool,
    ) -> bool {
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        self.feed.home.rx = None;
        let Some((request_id, valid_target)) = self.begin_channel_request(target, ctx) else {
            return false;
        };
        if force_refresh {
            self.channels.playlists.loaded = false;
            self.channels.playlists.error = None;
            self.channels.about.loaded = false;
            self.channels.about.error = None;
        }
        let (sender, receiver) = mpsc::channel();
        if let Some(cancel) = self.channels.videos.cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.videos.cancel = Some(cancel.clone());
        self.channels.videos.rx = Some(receiver);
        let input = valid_target.canonical_url();
        let limit = self.channels.video_limit;
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result =
                search::fetch_channel_page_with_cancel(&input, limit, force_refresh, &cancel);
            let _ = sender.send(ChannelFetchMessage {
                request_id,
                target: valid_target,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn begin_channel_more_videos_request(
        &mut self,
    ) -> Option<(u64, ChannelTarget, usize, usize)> {
        self.channels.begin_more_videos_request()
    }

    pub(crate) fn load_more_channel_videos(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target, start, count)) = self.begin_channel_more_videos_request()
        else {
            return false;
        };
        if let Some(cancel) = self.channels.videos.more_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.videos.more_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.videos.more_rx = Some(receiver);
        let input = target.canonical_url();
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result =
                search::fetch_channel_videos_batch_with_cancel(&input, start, count, &cancel);
            let _ = sender.send(ChannelMoreVideosFetchMessage {
                request_id,
                target,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn cancel_channel_more_videos(&mut self) {
        self.channels.cancel_more_videos();
    }

    pub(crate) fn complete_channel_more_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelVideosBatch, String>,
    ) -> bool {
        if self.channels.videos.active_more_request_id != Some(request_id) {
            return false;
        }
        if self
            .channels
            .page
            .as_ref()
            .is_none_or(|page| &page.target != target)
        {
            self.channels.videos.active_more_request_id = None;
            self.channels.videos.more_rx = None;
            self.channels.videos.more_cancel = None;
            return false;
        }
        self.channels.videos.active_more_request_id = None;
        self.channels.videos.more_rx = None;
        self.channels.videos.more_cancel = None;
        match result {
            Ok(batch) => {
                self.channels.videos.more_error = None;
                self.channels.videos.has_more = batch.has_more;
                self.channels.videos.reached_max = batch.reached_max_limit;
                if let Some(page) = &mut self.channels.page {
                    for item in batch.videos {
                        if page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT {
                            self.channels.videos.reached_max = true;
                            self.channels.videos.has_more = false;
                            break;
                        }
                        if !page
                            .videos
                            .iter()
                            .any(|existing| existing.video.url == item.video.url)
                        {
                            self.video_metadata
                                .insert(item.video.url.clone(), item.metadata.clone());
                            page.videos.push(item);
                        }
                    }
                    if page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT {
                        self.channels.videos.reached_max = true;
                        self.channels.videos.has_more = false;
                    }
                    let effective_limit = self
                        .channels
                        .video_limit
                        .max(page.videos.len())
                        .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
                    search::update_cached_channel_videos(
                        &page.target,
                        effective_limit,
                        &page.videos,
                    );
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            }
            Err(error) => {
                self.channels.videos.more_error = Some(error);
            }
        }
        true
    }

    pub(crate) fn begin_channel_playlists_request(&mut self) -> Option<(u64, ChannelTarget)> {
        self.channels.begin_playlists_request()
    }

    pub(crate) fn start_channel_playlists(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.begin_channel_playlists_request() else {
            return false;
        };
        if let Some(cancel) = self.channels.playlists.cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.playlists.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.playlists.rx = Some(receiver);
        let input = target.canonical_url();
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_channel_playlists_with_cancel(
                &input,
                search::DEFAULT_CHANNEL_PLAYLISTS_LIMIT,
                &cancel,
            );
            let _ = sender.send(ChannelPlaylistsFetchMessage {
                request_id,
                target,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn cancel_channel_playlists(&mut self) {
        self.channels.cancel_playlists();
    }

    pub(crate) fn complete_channel_playlists(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<Vec<search::ChannelPlaylist>, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.playlists.active_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target) {
            self.channels.playlists.active_request_id = None;
            self.channels.playlists.rx = None;
            self.channels.playlists.cancel = None;
            return false;
        }
        self.channels.playlists.active_request_id = None;
        self.channels.playlists.rx = None;
        self.channels.playlists.cancel = None;
        match result {
            Ok(playlists) => {
                for old_pl in &self.channels.playlists.items {
                    let thumb = &old_pl.thumbnail;
                    if !thumb.is_empty()
                        && !playlists.iter().any(|p| p.thumbnail == *thumb)
                        && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
                self.channels.playlists.items = playlists;
                self.channels.playlists.loaded = true;
                self.channels.playlists.error = None;
            }
            Err(error) => {
                self.channels.playlists.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn begin_channel_playlist_videos_request(
        &mut self,
        playlist: search::ChannelPlaylist,
        ctx: &egui::Context,
    ) -> Option<(u64, ChannelTarget, String)> {
        if !search::is_valid_playlist_id(&playlist.id) {
            self.channels.playlist_detail.error =
                Some("Não foi possível abrir a playlist: identificador inválido.".into());
            return None;
        }
        let target = self
            .channels
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_channel_target())?;
        if let Some(cancel) = self.channels.playlist_detail.cancel.take() {
            cancel.cancel();
        }
        for old_item in &self.channels.playlist_detail.items {
            let thumb = &old_item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channels.playlist_detail.items.clear();
        self.channels.playlist_detail.loaded = false;
        self.channels.playlist_detail.error = None;
        let playlist_id = playlist.id.clone();
        self.channels.playlist_detail.selected = Some(playlist);
        self.channels.next_request_id = self.channels.next_request_id.wrapping_add(1);
        let request_id = self.channels.next_request_id;
        self.channels.playlist_detail.active_request_id = Some(request_id);
        Some((request_id, target, playlist_id))
    }

    pub(crate) fn open_channel_public_playlist(
        &mut self,
        playlist: search::ChannelPlaylist,
        ctx: &egui::Context,
    ) -> bool {
        let Some((request_id, target, playlist_id)) =
            self.begin_channel_playlist_videos_request(playlist, ctx)
        else {
            return false;
        };
        let cancel = process::Cancellation::default();
        self.channels.playlist_detail.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.playlist_detail.rx = Some(receiver);
        let ctx_clone = ctx.clone();
        let pid_clone = playlist_id.clone();
        std::thread::spawn(move || {
            let result = search::fetch_public_playlist_videos_with_cancel(
                &pid_clone,
                search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT,
                &cancel,
            );
            let _ = sender.send(ChannelPlaylistVideosFetchMessage {
                request_id,
                target,
                playlist_id: pid_clone,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn close_channel_public_playlist(&mut self, ctx: &egui::Context) {
        if let Some(cancel) = self.channels.playlist_detail.cancel.take() {
            cancel.cancel();
        }
        self.channels.playlist_detail.rx = None;
        self.channels.playlist_detail.active_request_id = None;
        self.channels.playlist_detail.error = None;
        self.channels.playlist_detail.loaded = false;
        self.channels.playlist_detail.selected = None;
        for old_item in &self.channels.playlist_detail.items {
            let thumb = &old_item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channels.playlist_detail.items.clear();
    }

    pub(crate) fn cancel_channel_playlist_videos(&mut self) {
        self.channels.cancel_playlist_videos();
    }

    pub(crate) fn complete_channel_playlist_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        playlist_id: &str,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.channels.playlist_detail.active_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target)
            || self
                .channels
                .playlist_detail
                .selected
                .as_ref()
                .is_none_or(|pl| pl.id != playlist_id)
        {
            self.channels.playlist_detail.active_request_id = None;
            self.channels.playlist_detail.rx = None;
            self.channels.playlist_detail.cancel = None;
            return false;
        }
        self.channels.playlist_detail.active_request_id = None;
        self.channels.playlist_detail.rx = None;
        self.channels.playlist_detail.cancel = None;
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                self.channels.playlist_detail.items = videos;
                self.channels.playlist_detail.loaded = true;
                self.channels.playlist_detail.error = None;
            }
            Err(error) => {
                self.channels.playlist_detail.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn begin_channel_about_request(&mut self) -> Option<(u64, ChannelTarget)> {
        self.channels.begin_about_request()
    }

    pub(crate) fn start_channel_about(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.begin_channel_about_request() else {
            return false;
        };
        if let Some(cancel) = self.channels.about.cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.about.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.about.rx = Some(receiver);
        let input = target.canonical_url();
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_channel_about_with_cancel(&input, &cancel);
            let _ = sender.send(ChannelAboutFetchMessage {
                request_id,
                target,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn cancel_channel_about(&mut self) {
        self.channels.cancel_about();
    }

    pub(crate) fn complete_channel_about(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelAboutInfo, String>,
    ) -> bool {
        if self.channels.about.active_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target) {
            self.channels.about.active_request_id = None;
            self.channels.about.rx = None;
            self.channels.about.cancel = None;
            return false;
        }
        self.channels.about.active_request_id = None;
        self.channels.about.rx = None;
        self.channels.about.cancel = None;
        match result {
            Ok(about) => {
                if let Some(page) = &mut self.channels.page {
                    if page.channel_id.is_none() {
                        page.channel_id = about.channel_id.clone();
                    }
                    if page.title.is_none() {
                        page.title = about.title.clone();
                    }
                    if page.handle.is_none() {
                        page.handle = about.handle.clone();
                    }
                    if let Some(desc) = &about.description {
                        let current_len = page.description.as_ref().map_or(0, String::len);
                        if desc.len() >= current_len {
                            page.description = Some(desc.clone());
                        }
                    }
                    if about.subscriber_count.is_some() {
                        page.subscriber_count = about.subscriber_count;
                    }
                    if about.view_count.is_some() {
                        page.view_count = about.view_count;
                    }
                    if page.subscriber_count_text.is_none() {
                        page.subscriber_count_text = about.subscriber_count_text.clone();
                    }
                    if page.video_count_text.is_none() {
                        page.video_count_text = about.video_count_text.clone();
                    }
                    if page.view_count_text.is_none() {
                        page.view_count_text = about.view_count_text.clone();
                    }
                    if page.country.is_none() {
                        page.country = about.country.clone();
                    }
                    if page.joined_date_text.is_none() {
                        page.joined_date_text = about.joined_date_text.clone();
                    }
                    if page.links.is_empty() && !about.links.is_empty() {
                        page.links = about.links.clone();
                    }
                    if page.avatar_url.is_none() {
                        page.avatar_url = about.avatar_url.clone();
                    }
                    if page.banner_url.is_none() {
                        page.banner_url = about.banner_url.clone();
                    }
                    if page.verified.is_none() {
                        page.verified = about.verified;
                    }
                    if !about.tags.is_empty() {
                        page.tags = about.tags.clone();
                    }
                }
                self.channels.about.info = Some(about);
                self.channels.about.loaded = true;
                self.channels.about.error = None;
            }
            Err(error) => {
                self.channels.about.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn start_channel_shorts(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.channels.begin_shorts_request() else {
            return false;
        };
        if let Some(cancel) = self.channels.shorts.cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.shorts.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.shorts.rx = Some(receiver);
        let input = target.canonical_url();
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_channel_tab_videos_with_cancel(
                &input,
                search::ChannelFeedTab::Shorts,
                limit,
                &cancel,
            );
            let _ = sender.send(ChannelTabVideosFetchMessage {
                request_id,
                target,
                tab: search::ChannelFeedTab::Shorts,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn cancel_channel_shorts(&mut self) {
        self.channels.cancel_shorts();
    }

    pub(crate) fn start_channel_streams(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.channels.begin_streams_request() else {
            return false;
        };
        if let Some(cancel) = self.channels.streams.cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.streams.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.streams.rx = Some(receiver);
        let input = target.canonical_url();
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_channel_tab_videos_with_cancel(
                &input,
                search::ChannelFeedTab::Streams,
                limit,
                &cancel,
            );
            let _ = sender.send(ChannelTabVideosFetchMessage {
                request_id,
                target,
                tab: search::ChannelFeedTab::Streams,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn cancel_channel_streams(&mut self) {
        self.channels.cancel_streams();
    }

    pub(crate) fn complete_channel_tab_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        tab: search::ChannelFeedTab,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        let state = match tab {
            search::ChannelFeedTab::Shorts => &mut self.channels.shorts,
            search::ChannelFeedTab::Streams => &mut self.channels.streams,
            search::ChannelFeedTab::Videos => return false,
        };
        if state.active_request_id != Some(request_id) {
            return false;
        }
        let current_target = self.channels.current_target();
        let state = match tab {
            search::ChannelFeedTab::Shorts => &mut self.channels.shorts,
            search::ChannelFeedTab::Streams => &mut self.channels.streams,
            search::ChannelFeedTab::Videos => return false,
        };
        if current_target.as_ref() != Some(target) {
            state.active_request_id = None;
            state.rx = None;
            state.cancel = None;
            return false;
        }
        state.active_request_id = None;
        state.rx = None;
        state.cancel = None;
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                let state = match tab {
                    search::ChannelFeedTab::Shorts => &mut self.channels.shorts,
                    search::ChannelFeedTab::Streams => &mut self.channels.streams,
                    search::ChannelFeedTab::Videos => return false,
                };
                state.items = videos;
                state.loaded = true;
                state.error = None;
            }
            Err(error) => {
                let state = match tab {
                    search::ChannelFeedTab::Shorts => &mut self.channels.shorts,
                    search::ChannelFeedTab::Streams => &mut self.channels.streams,
                    search::ChannelFeedTab::Videos => return false,
                };
                state.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_channel_tab_feeds(&mut self) {
        let shorts_msg = self.channels.shorts.rx.as_ref().map(|rx| rx.try_recv());
        match shorts_msg {
            Some(Ok(msg)) => {
                self.complete_channel_tab_videos(msg.request_id, &msg.target, msg.tab, msg.result);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.channels.shorts.rx = None;
                self.channels.shorts.cancel = None;
                self.channels.shorts.active_request_id = None;
                self.channels.shorts.error =
                    Some("A consulta de Shorts do canal foi encerrada inesperadamente.".into());
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }

        let streams_msg = self.channels.streams.rx.as_ref().map(|rx| rx.try_recv());
        match streams_msg {
            Some(Ok(msg)) => {
                self.complete_channel_tab_videos(msg.request_id, &msg.target, msg.tab, msg.result);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.channels.streams.rx = None;
                self.channels.streams.cancel = None;
                self.channels.streams.active_request_id = None;
                self.channels.streams.error = Some(
                    "A consulta de transmissões ao vivo do canal foi encerrada inesperadamente."
                        .into(),
                );
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_channel_sorted_videos(
        &mut self,
        sort: search::ChannelVideosSort,
        ctx: &egui::Context,
    ) -> bool {
        if sort == search::ChannelVideosSort::Latest {
            self.channels.cancel_sorted_videos();
            self.channels.videos.sort = sort;
            self.channels.videos.sorted_error = None;
            return false;
        }
        let Some(page) = self.channels.page.as_ref() else {
            return false;
        };
        let continuation = match sort {
            search::ChannelVideosSort::Latest => None,
            search::ChannelVideosSort::Popular => page.videos_popular_continuation.clone(),
            search::ChannelVideosSort::Oldest => page.videos_oldest_continuation.clone(),
        };
        let mut fallback = page.videos.clone();
        match sort {
            search::ChannelVideosSort::Latest => {}
            search::ChannelVideosSort::Popular => {
                fallback.sort_by(|a, b| {
                    b.metadata
                        .view_count
                        .unwrap_or(0)
                        .cmp(&a.metadata.view_count.unwrap_or(0))
                });
            }
            search::ChannelVideosSort::Oldest => {
                fallback.reverse();
            }
        }
        let channel_title = page.title.clone();
        let channel_id = page.channel_id.clone();
        let canonical_url = page.canonical_url.clone();
        let verified = page.verified == Some(true);

        self.channels.videos.sort = sort;
        self.channels.videos.sorted_error = None;
        let Some(token) = continuation.filter(|t| !t.trim().is_empty()) else {
            self.channels.cancel_sorted_videos();
            self.channels.videos.sorted_items = fallback;
            return false;
        };
        let Some((request_id, target)) = self.channels.begin_sorted_videos_request() else {
            return false;
        };
        if let Some(cancel) = self.channels.videos.sorted_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.videos.sorted_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.videos.sorted_rx = Some(receiver);
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_channel_sorted_videos_with_cancel(
                &token,
                channel_title.as_deref(),
                channel_id.as_deref(),
                &canonical_url,
                verified,
                limit,
                &cancel,
            );
            let _ = sender.send(ChannelSortedVideosFetchMessage {
                request_id,
                target,
                sort,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn complete_channel_sorted_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        sort: search::ChannelVideosSort,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.channels.videos.active_sorted_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target) {
            self.channels.videos.active_sorted_request_id = None;
            self.channels.videos.sorted_rx = None;
            self.channels.videos.sorted_cancel = None;
            return false;
        }
        self.channels.videos.active_sorted_request_id = None;
        self.channels.videos.sorted_rx = None;
        self.channels.videos.sorted_cancel = None;
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                if self.channels.videos.sort == sort {
                    self.channels.videos.sorted_items = videos;
                }
                self.channels.videos.sorted_error = None;
            }
            Err(error) => {
                self.channels.videos.sorted_error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_channel_sorted_videos(&mut self) {
        let msg = self
            .channels
            .videos
            .sorted_rx
            .as_ref()
            .map(|rx| rx.try_recv());
        match msg {
            Some(Ok(msg)) => {
                self.complete_channel_sorted_videos(
                    msg.request_id,
                    &msg.target,
                    msg.sort,
                    msg.result,
                );
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.channels.videos.sorted_rx = None;
                self.channels.videos.sorted_cancel = None;
                self.channels.videos.active_sorted_request_id = None;
                self.channels.videos.sorted_error =
                    Some("A ordenação de vídeos do canal foi encerrada inesperadamente.".into());
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_channel_search(&mut self, query: &str, ctx: &egui::Context) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            self.channels.cancel_channel_search();
            self.channels.search.items.clear();
            self.channels.search.loaded = false;
            self.channels.search.error = None;
            return false;
        }
        let query_owned = trimmed.to_owned();
        self.channels.search.query = query_owned.clone();
        self.channels.search.input_open = true;
        self.channels.tab = ChannelTab::Search;
        let Some((request_id, target)) = self.channels.begin_channel_search_request() else {
            return false;
        };
        if let Some(cancel) = self.channels.search.cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channels.search.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channels.search.rx = Some(receiver);
        let input = target.canonical_url();
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result =
                search::fetch_channel_search_with_cancel(&input, &query_owned, limit, &cancel);
            let _ = sender.send(ChannelSearchFetchMessage {
                request_id,
                target,
                query: query_owned,
                result,
            });
            ctx_clone.request_repaint();
        });
        true
    }

    pub(crate) fn complete_channel_search(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        query: String,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.channels.search.active_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target) {
            self.channels.search.active_request_id = None;
            self.channels.search.rx = None;
            self.channels.search.cancel = None;
            return false;
        }
        self.channels.search.active_request_id = None;
        self.channels.search.rx = None;
        self.channels.search.cancel = None;
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                self.channels.search.query = query;
                self.channels.search.items = videos;
                self.channels.search.loaded = true;
                self.channels.search.error = None;
            }
            Err(error) => {
                self.channels.search.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_channel_search(&mut self) {
        let msg = self.channels.search.rx.as_ref().map(|rx| rx.try_recv());
        match msg {
            Some(Ok(msg)) => {
                self.complete_channel_search(msg.request_id, &msg.target, msg.query, msg.result);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.channels.search.rx = None;
                self.channels.search.cancel = None;
                self.channels.search.active_request_id = None;
                self.channels.search.error =
                    Some("A busca interna no canal foi encerrada inesperadamente.".into());
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn record_recent_search(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with(':')
        {
            return;
        }
        let lower = trimmed.to_lowercase();
        self.feed
            .recent_searches
            .retain(|existing| existing.to_lowercase() != lower);
        self.feed.recent_searches.insert(0, trimmed.to_owned());
        self.feed.recent_searches.truncate(12);
    }

    pub(crate) fn remove_recent_search(&mut self, query: &str) {
        let lower = query.trim().to_lowercase();
        self.feed
            .recent_searches
            .retain(|existing| existing.to_lowercase() != lower);
    }

    pub(crate) fn mute_channel_preference(&mut self, channel: &str) {
        let trimmed = channel.trim();
        if trimmed.is_empty() {
            return;
        }
        let key = recommendations::normalize_channel_key(trimmed);
        let mut prefs = self.settings.prefs.value.clone();
        if !prefs
            .muted_channels
            .iter()
            .any(|existing| recommendations::normalize_channel_key(existing) == key)
        {
            prefs.muted_channels.push(trimmed.to_owned());
            self.apply_app_preferences(prefs);
        }
        self.invalidate_feed_cache();
        let notice = format!("Menos vídeos de \"{trimmed}\" no feed.");
        self.message = notice.clone();
        self.toasts.info(notice);
    }

    pub(crate) fn mute_topic_preference(&mut self, topic: &str) {
        let trimmed = topic.trim();
        if trimmed.is_empty() {
            return;
        }
        let lower = trimmed.to_lowercase();
        let mut prefs = self.settings.prefs.value.clone();
        if !prefs
            .muted_topics
            .iter()
            .any(|existing| existing.trim().to_lowercase() == lower)
        {
            prefs.muted_topics.push(trimmed.to_owned());
            self.apply_app_preferences(prefs);
        }
        self.invalidate_feed_cache();
        let notice = format!("Tópico \"{trimmed}\" silenciado.");
        self.message = notice.clone();
        self.toasts.info(notice);
    }

    pub(crate) fn is_topic_pinned(&self, topic: &str) -> bool {
        let lower = topic.trim().to_lowercase();
        !lower.is_empty()
            && self
                .settings
                .prefs
                .value
                .pinned_topics
                .iter()
                .any(|existing| existing.trim().to_lowercase() == lower)
    }

    pub(crate) fn toggle_pinned_topic(&mut self, topic: &str) {
        let trimmed = topic.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            return;
        }
        let lower = trimmed.to_lowercase();
        let mut prefs = self.settings.prefs.value.clone();
        if let Some(pos) = prefs
            .pinned_topics
            .iter()
            .position(|existing| existing.trim().to_lowercase() == lower)
        {
            prefs.pinned_topics.remove(pos);
            self.apply_app_preferences(prefs);
            self.invalidate_feed_cache();
            let notice = format!("Tópico \"{trimmed}\" desafixado.");
            self.message = notice.clone();
            self.toasts.info(notice);
        } else if prefs.pinned_topics.len() < 24 {
            prefs.pinned_topics.push(trimmed.to_owned());
            self.apply_app_preferences(prefs);
            self.invalidate_feed_cache();
            let notice = format!("Tópico \"{trimmed}\" fixado nos filtros.");
            self.message = notice.clone();
            self.toasts.success(notice);
        } else {
            let warn = "Limite de 24 tópicos fixados atingido.".to_owned();
            self.message = warn.clone();
            self.toasts.warning(warn);
        }
    }

    pub(crate) fn start_search_suggestions_fetch(&mut self, query: &str, ctx: &egui::Context) {
        let trimmed = query.trim();
        if trimmed.len() < 2
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with(':')
        {
            if let Some(cancel) = self.feed.search_suggest.cancel.take() {
                cancel.cancel();
            }
            self.feed.search_suggest.rx = None;
            self.feed.search_suggest.active_request_id = None;
            self.feed.search_suggest.remote_suggestions.clear();
            return;
        }
        if let Some(cancel) = self.feed.search_suggest.cancel.take() {
            cancel.cancel();
        }
        self.feed.search_suggest.next_request_id =
            self.feed.search_suggest.next_request_id.wrapping_add(1);
        let request_id = self.feed.search_suggest.next_request_id;
        self.feed.search_suggest.active_request_id = Some(request_id);
        self.feed.search_suggest.last_fetched_query = trimmed.to_owned();
        let (sender, receiver) = mpsc::channel();
        let cancel = process::Cancellation::default();
        self.feed.search_suggest.cancel = Some(cancel.clone());
        self.feed.search_suggest.rx = Some(receiver);
        let owned_query = trimmed.to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_search_suggestions(&owned_query, &cancel);
            let _ = sender.send(app_state::SearchSuggestFetchMessage {
                request_id,
                query: owned_query,
                result,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn poll_search_suggestions(&mut self, ctx: &egui::Context) {
        loop {
            let outcome = self
                .feed
                .search_suggest
                .rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let is_current_id =
                        self.feed.search_suggest.active_request_id == Some(msg.request_id);
                    let is_current_query = msg
                        .query
                        .trim()
                        .eq_ignore_ascii_case(self.feed.query.trim());
                    if is_current_id {
                        self.feed.search_suggest.active_request_id = None;
                        self.feed.search_suggest.rx = None;
                        self.feed.search_suggest.cancel = None;
                        if is_current_query && let Ok(items) = msg.result {
                            self.feed.search_suggest.remote_suggestions = items;
                        }
                    }
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.feed.search_suggest.active_request_id = None;
                    self.feed.search_suggest.rx = None;
                    self.feed.search_suggest.cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }

        if let Some(pending) = self.feed.search_suggest.pending_fetch_query.clone() {
            let elapsed_ready = self
                .feed
                .search_suggest
                .last_input_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(220));
            if elapsed_ready {
                self.feed.search_suggest.pending_fetch_query = None;
                if pending.trim().eq_ignore_ascii_case(self.feed.query.trim()) {
                    self.start_search_suggestions_fetch(&pending, ctx);
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }
    }

    pub(crate) fn subscription_status_for(
        &self,
        target: Option<&ChannelTarget>,
        channel_id: Option<&str>,
    ) -> Option<&oauth::SubscriptionStatus> {
        if let Some(cid) = channel_id.map(str::trim).filter(|s| !s.is_empty())
            && let Some(status) = self.auth.subscriptions.get(cid)
        {
            return Some(status);
        }
        if let Some(t) = target {
            if let Some(status) = self.auth.subscriptions.get(&t.canonical_url()) {
                return Some(status);
            }
            if let ChannelTarget::ChannelId(cid) = t
                && let Some(status) = self.auth.subscriptions.get(cid)
            {
                return Some(status);
            }
        }
        None
    }

    pub(crate) fn start_check_subscription(
        &mut self,
        target: ChannelTarget,
        channel_id_hint: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        if let Some(cancel) = self.auth.sub_check_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.auth.sub_check_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.sub_check_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| {
                    if let Some(cid) = channel_id_hint
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty())
                    {
                        Ok(cid)
                    } else {
                        oauth::resolve_channel_id(&session.access_token, &target, &cancel)
                    }
                })
                .and_then(|cid| oauth::check_subscription(&session.access_token, &cid, &cancel))
                .map(|status| (target.canonical_url(), status));
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn start_toggle_subscription(
        &mut self,
        target: ChannelTarget,
        channel_id_hint: Option<String>,
        ctx: &egui::Context,
    ) {
        if self.auth.sub_toggle_rx.is_some() {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            self.start_reauth(ctx);
            return;
        };
        let existing = self
            .subscription_status_for(Some(&target), channel_id_hint.as_deref())
            .cloned();
        if let Some(cancel) = self.auth.sub_toggle_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.auth.sub_toggle_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.sub_toggle_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| {
                    if let Some(cid) = existing
                        .as_ref()
                        .map(|s| s.channel_id.trim().to_owned())
                        .filter(|s| !s.is_empty())
                        .or_else(|| {
                            channel_id_hint
                                .map(|s| s.trim().to_owned())
                                .filter(|s| !s.is_empty())
                        })
                    {
                        Ok(cid)
                    } else {
                        oauth::resolve_channel_id(&session.access_token, &target, &cancel)
                    }
                })
                .and_then(|cid| {
                    if let Some(sub_id) = existing.and_then(|s| s.subscription_id) {
                        oauth::unsubscribe_channel(&session.access_token, &sub_id, &cid, &cancel)
                    } else {
                        oauth::subscribe_channel(&session.access_token, &cid, &cancel)
                    }
                })
                .map(|status| (target.canonical_url(), status));
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_subscriptions(&mut self, _ctx: &egui::Context) {
        let check_msg = self.auth.sub_check_rx.as_ref().map(|rx| rx.try_recv());
        match check_msg {
            Some(Ok((session, result))) => {
                self.auth.sub_check_rx = None;
                self.auth.sub_check_cancel = None;
                let _ = self.set_oauth_session(session);
                if let Ok((key, status)) = result {
                    self.auth
                        .subscriptions
                        .insert(status.channel_id.clone(), status.clone());
                    self.auth.subscriptions.insert(key, status);
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auth.sub_check_rx = None;
                self.auth.sub_check_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }

        let toggle_msg = self.auth.sub_toggle_rx.as_ref().map(|rx| rx.try_recv());
        match toggle_msg {
            Some(Ok((session, result))) => {
                self.auth.sub_toggle_rx = None;
                self.auth.sub_toggle_cancel = None;
                let _ = self.set_oauth_session(session);
                match result {
                    Ok((key, status)) => {
                        let subscribed = status.subscription_id.is_some();
                        self.auth
                            .subscriptions
                            .insert(status.channel_id.clone(), status.clone());
                        self.auth.subscriptions.insert(key, status);
                        self.feed.home.subscriptions_cached_at = None;
                        self.feed.home.subscription_channels_loaded = false;
                        self.message = if subscribed {
                            "Inscrição realizada com sucesso.".into()
                        } else {
                            "Inscrição cancelada.".into()
                        };
                    }
                    Err(error) => {
                        self.message = error;
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auth.sub_toggle_rx = None;
                self.auth.sub_toggle_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_fetch_my_channel(&mut self, open_when_ready: bool, ctx: &egui::Context) {
        if self.auth.my_channel_rx.is_some() {
            if open_when_ready {
                self.auth.my_channel_open_when_ready = true;
            }
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        if let Some(cancel) = self.auth.my_channel_cancel.take() {
            cancel.cancel();
        }
        self.auth.my_channel_open_when_ready = open_when_ready;
        self.auth.my_channel_fetch_attempted = true;
        let cancel = process::Cancellation::default();
        self.auth.my_channel_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.my_channel_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| oauth::my_channel_profile(&session.access_token, &cancel));
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_my_channel(&mut self, ctx: &egui::Context) {
        let msg = self.auth.my_channel_rx.as_ref().map(|rx| rx.try_recv());
        match msg {
            Some(Ok((session, result))) => {
                self.auth.my_channel_rx = None;
                self.auth.my_channel_cancel = None;
                let open_when_ready = std::mem::take(&mut self.auth.my_channel_open_when_ready);
                let _ = self.set_oauth_session(session);
                match result {
                    Ok(profile) => {
                        if let Some(title) = profile.title.as_deref() {
                            self.cache_channel_avatar(title, profile.avatar_url.as_deref());
                        }
                        self.auth.my_channel = Some(profile.target.clone());
                        self.auth.my_channel_title = profile.title;
                        self.auth.my_channel_avatar_url = profile.avatar_url;
                        self.auth.my_channel_subscribers = profile.subscriber_count;
                        if open_when_ready {
                            self.open_channel(profile.target, ctx);
                        }
                    }
                    Err(error) => {
                        if open_when_ready {
                            self.message = error;
                        }
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auth.my_channel_rx = None;
                self.auth.my_channel_cancel = None;
                self.auth.my_channel_open_when_ready = false;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_video_report(
        &mut self,
        video_id: String,
        reason: oauth::AbuseReportReason,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        if let Some(cancel) = self.auth.report_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.auth.report_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.report_rx = Some(receiver);
        let reason_label = reason.label().to_owned();
        self.message = format!("Enviando denúncia ({reason_label})...");
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| {
                    oauth::report_video_abuse(&session.access_token, &video_id, reason, &cancel)
                })
                .map(|()| reason_label);
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_report(&mut self, _ctx: &egui::Context) {
        let msg = self.auth.report_rx.as_ref().map(|rx| rx.try_recv());
        match msg {
            Some(Ok((session, result))) => {
                self.auth.report_rx = None;
                self.auth.report_cancel = None;
                let _ = self.set_oauth_session(session);
                match result {
                    Ok(reason_label) => {
                        self.message = format!(
                            "Denúncia enviada ao YouTube ({reason_label}). Resultado ocultado."
                        );
                    }
                    Err(error) => {
                        self.message = format!(
                            "Resultado ocultado. Não foi possível enviar a denúncia ao YouTube: {error}"
                        );
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auth.report_rx = None;
                self.auth.report_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_fetch_remote_playlists(&mut self, ctx: &egui::Context) {
        if self.auth.remote_playlists_rx.is_some() {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        if let Some(cancel) = self.auth.remote_playlists_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.auth.remote_playlists_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.remote_playlists_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| oauth::playlists_feed(&session.access_token, &cancel))
                .map(|feed| {
                    feed.items
                        .into_iter()
                        .filter_map(|item| {
                            let id = item
                                .video
                                .url
                                .strip_prefix("fasttube-google-playlist:")?
                                .to_owned();
                            Some((id, item.video.title))
                        })
                        .collect::<Vec<_>>()
                });
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_remote_playlists(&mut self, _ctx: &egui::Context) {
        let msg = self
            .auth
            .remote_playlists_rx
            .as_ref()
            .map(|rx| rx.try_recv());
        match msg {
            Some(Ok((session, result))) => {
                self.auth.remote_playlists_rx = None;
                self.auth.remote_playlists_cancel = None;
                let _ = self.set_oauth_session(session);
                match result {
                    Ok(playlists) => {
                        self.auth.remote_playlists = playlists;
                    }
                    Err(error) => {
                        self.message = error;
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auth.remote_playlists_rx = None;
                self.auth.remote_playlists_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_create_remote_playlist(
        &mut self,
        title: String,
        video_id: Option<String>,
        ctx: &egui::Context,
    ) {
        if self.auth.playlist_action_rx.is_some() {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            self.start_reauth(ctx);
            return;
        };
        if let Some(cancel) = self.auth.playlist_action_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.auth.playlist_action_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.playlist_action_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| {
                    oauth::create_playlist(&session.access_token, &title, "private", &cancel)
                })
                .and_then(|(pl_id, pl_title)| {
                    if let Some(vid) = video_id.as_deref() {
                        oauth::add_video_to_playlist(&session.access_token, &pl_id, vid, &cancel)?;
                        Ok((
                            Some((pl_id, pl_title.clone())),
                            format!("Playlist '{pl_title}' criada e vídeo salvo."),
                        ))
                    } else {
                        Ok((
                            Some((pl_id, pl_title.clone())),
                            format!("Playlist '{pl_title}' criada."),
                        ))
                    }
                });
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn start_add_to_remote_playlist(
        &mut self,
        playlist_id: String,
        playlist_title: String,
        video_id: String,
        ctx: &egui::Context,
    ) {
        if self.auth.playlist_action_rx.is_some() {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            self.start_reauth(ctx);
            return;
        };
        if let Some(cancel) = self.auth.playlist_action_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.auth.playlist_action_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.playlist_action_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| {
                    oauth::add_video_to_playlist(
                        &session.access_token,
                        &playlist_id,
                        &video_id,
                        &cancel,
                    )
                })
                .map(|_| (None, format!("Vídeo salvo na playlist '{playlist_title}'.")));
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_playlist_action(&mut self, _ctx: &egui::Context) {
        let msg = self
            .auth
            .playlist_action_rx
            .as_ref()
            .map(|rx| rx.try_recv());
        match msg {
            Some(Ok((session, result))) => {
                self.auth.playlist_action_rx = None;
                self.auth.playlist_action_cancel = None;
                let _ = self.set_oauth_session(session);
                match result {
                    Ok((created, notice)) => {
                        if let Some((pl_id, pl_title)) = created
                            && !self
                                .auth
                                .remote_playlists
                                .iter()
                                .any(|(id, _)| id == &pl_id)
                        {
                            self.auth.remote_playlists.push((pl_id, pl_title));
                        }
                        self.message = notice;
                    }
                    Err(error) => {
                        self.message = error;
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auth.playlist_action_rx = None;
                self.auth.playlist_action_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn seek_to_seconds(&mut self, secs: f64) {
        let is_live = self.active_video_is_live();
        let live_dvr = self
            .playback
            .active
            .as_ref()
            .is_some_and(|active| active.live_dvr);
        if is_live && !live_dvr {
            return;
        }
        let target = secs.max(0.0);
        #[cfg(windows)]
        if let Some(player) = self.player.as_ref() {
            let duration = player.number("duration").filter(|&dur| dur > 0.0);
            if is_live
                && let Some(dur) = duration
                && (dur <= 1.5 || target >= (dur - 1.5).max(0.0))
                && let Some(video) = self.selected.clone()
            {
                self.go_live_edge(&video);
                return;
            }
            let clamped = if is_live {
                duration.map_or(target, |dur| target.min((dur - 1.5).max(0.0)))
            } else {
                target
            };
            let _ = player.seek(clamped, "absolute+exact");
            self.record_manual_seek(clamped, duration, Instant::now());
        } else {
            self.record_manual_seek(target, None, Instant::now());
        }
        #[cfg(not(windows))]
        {
            self.record_manual_seek(target, None, Instant::now());
        }
    }

    pub(crate) fn next_recommended_video(&mut self) -> Option<Video> {
        self.ensure_feed_cache();
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str());
        for ranked in &self.feed.cache.ranked {
            let v = &ranked.video;
            if Some(v.url.as_str()) != selected_url
                && !v.url.starts_with("fasttube-google-playlist:")
                && !self.feed.home.playlist_urls.contains(&v.url)
            {
                return Some(v.clone());
            }
        }
        for v in &self.feed.videos {
            if Some(v.url.as_str()) != selected_url
                && !v.url.starts_with("fasttube-google-playlist:")
                && !self.feed.home.playlist_urls.contains(&v.url)
            {
                return Some(v.clone());
            }
        }
        None
    }

    pub(crate) fn shorts_feed_candidates(&self) -> Vec<&Video> {
        self.feed
            .videos
            .iter()
            .filter(|v| {
                self.ui.browse_section == BrowseSection::Shorts
                    || recommendations::is_short_video(v, self.metadata_for(&v.url))
                    || v.url.contains("/shorts/")
            })
            .collect()
    }

    pub(crate) fn short_index_and_total(&self) -> Option<(usize, usize)> {
        let shorts = self.shorts_feed_candidates();
        if shorts.is_empty() {
            return None;
        }
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str())?;
        let idx = shorts.iter().position(|v| v.url.as_str() == selected_url)?;
        Some((idx + 1, shorts.len()))
    }

    pub(crate) fn next_or_prev_short(&self, forward: bool) -> Option<Video> {
        let shorts = self.shorts_feed_candidates();
        if shorts.is_empty() {
            return None;
        }
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str());
        if let Some(idx) = shorts
            .iter()
            .position(|v| Some(v.url.as_str()) == selected_url)
        {
            let next_idx = if forward {
                idx.checked_add(1)?
            } else {
                idx.checked_sub(1)?
            };
            shorts.get(next_idx).map(|v| (*v).clone())
        } else if forward {
            shorts.first().map(|v| (*v).clone())
        } else {
            None
        }
    }

    pub(crate) fn toggle_collection_video(&mut self, name: &str, video: &Video) {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > 128 {
            return;
        }
        let Some(vid) = history::youtube_video_id(&video.url) else {
            return;
        };
        let total_items = self.library.data.item_count();
        if let Some(collection) = self
            .library
            .data
            .collections
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(trimmed))
        {
            if let Some(pos) = collection
                .videos
                .iter()
                .position(|v| history::youtube_video_id(&v.video.url).as_deref() == Some(&vid))
            {
                collection.videos.remove(pos);
            } else if total_items < 500 {
                collection.videos.insert(
                    0,
                    library::VideoRecord {
                        video: video.clone(),
                    },
                );
            }
        } else if self.library.data.collections.len() < 100 && total_items < 500 {
            self.library.data.collections.push(library::Collection {
                name: trimmed.to_owned(),
                videos: vec![library::VideoRecord {
                    video: video.clone(),
                }],
            });
        }
        self.save_library();
    }

    pub(crate) fn video_saved_in_library(&self, video: &Video) -> bool {
        let vid = history::youtube_video_id(&video.url);
        let matches = |record: &library::VideoRecord| {
            if let Some(target_id) = vid.as_deref() {
                history::youtube_video_id(&record.video.url).as_deref() == Some(target_id)
            } else {
                record.video.url == video.url
            }
        };
        self.library.data.watch_later.iter().any(matches)
            || self.library.data.favorites.iter().any(matches)
            || self
                .library
                .data
                .collections
                .iter()
                .any(|c| c.videos.iter().any(matches))
    }

    pub(crate) fn save_menu_ui(&mut self, ui: &mut egui::Ui, video: &Video, ctx: &egui::Context) {
        if self.save_menu_contents_ui(ui, video, ctx) {
            self.playback.save_menu_open = false;
        }
    }

    pub(crate) fn save_menu_contents_ui(
        &mut self,
        ui: &mut egui::Ui,
        video: &Video,
        ctx: &egui::Context,
    ) -> bool {
        let mut close_requested = false;
        let palette = self.palette();
        let border = palette.card_alt_stroke;
        let panel_alt = palette.card_alt_bg;
        let text = palette.header_button_fg;
        let muted = palette.neutral_status_fg;
        let vid = history::youtube_video_id(&video.url);

        egui::Frame::NONE
            .fill(panel_alt)
            .stroke(egui::Stroke::new(1.0, border))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Salvar vídeo em...")
                            .color(text)
                            .size(13.0)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("✕").color(muted).size(12.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            close_requested = true;
                        }
                    });
                });
                ui.add_space(6.0);

                let in_watch_later = self.library.data.watch_later.iter().any(|v| {
                    vid.as_deref().is_some_and(|id| {
                        history::youtube_video_id(&v.video.url).as_deref() == Some(id)
                    }) || v.video.url == video.url
                });
                let mut wl_checked = in_watch_later;
                if ui
                    .checkbox(
                        &mut wl_checked,
                        RichText::new("⏱ Assistir mais tarde")
                            .color(text)
                            .size(12.5),
                    )
                    .changed()
                    && let Some(id) = vid.as_deref()
                {
                    if in_watch_later {
                        self.library.data.watch_later.retain(|v| {
                            history::youtube_video_id(&v.video.url).as_deref() != Some(id)
                        });
                    } else if self.library.data.item_count() < 500 {
                        self.library.data.watch_later.insert(
                            0,
                            library::VideoRecord {
                                video: video.clone(),
                            },
                        );
                    }
                    self.save_library();
                }

                let in_favorites = self.library.data.favorites.iter().any(|v| {
                    vid.as_deref().is_some_and(|id| {
                        history::youtube_video_id(&v.video.url).as_deref() == Some(id)
                    }) || v.video.url == video.url
                });
                let mut fav_checked = in_favorites;
                if ui
                    .checkbox(
                        &mut fav_checked,
                        RichText::new("★ Favoritos").color(text).size(12.5),
                    )
                    .changed()
                    && let Some(id) = vid.as_deref()
                {
                    if in_favorites {
                        self.library.data.favorites.retain(|v| {
                            history::youtube_video_id(&v.video.url).as_deref() != Some(id)
                        });
                    } else if self.library.data.item_count() < 500 {
                        self.library.data.favorites.insert(
                            0,
                            library::VideoRecord {
                                video: video.clone(),
                            },
                        );
                    }
                    self.save_library();
                }

                let collection_names: Vec<(String, bool)> = self
                    .library
                    .data
                    .collections
                    .iter()
                    .map(|c| {
                        let present = c.videos.iter().any(|v| {
                            vid.as_deref().is_some_and(|id| {
                                history::youtube_video_id(&v.video.url).as_deref() == Some(id)
                            }) || v.video.url == video.url
                        });
                        (c.name.clone(), present)
                    })
                    .collect();
                for (col_name, present) in collection_names {
                    let mut checked = present;
                    if ui
                        .checkbox(
                            &mut checked,
                            RichText::new(format!("📁 {col_name}"))
                                .color(text)
                                .size(12.5),
                        )
                        .changed()
                    {
                        self.toggle_collection_video(&col_name, video);
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.library.collection_name)
                            .hint_text("Nova coleção local...")
                            .desired_width(180.0),
                    );
                    let can_create = !self.library.collection_name.trim().is_empty();
                    let create_btn = egui::Button::new(
                        RichText::new("+ Criar coleção")
                            .color(palette.header_button_fg)
                            .size(12.0),
                    )
                    .fill(palette.header_button_bg)
                    .corner_radius(egui::CornerRadius::same(14));
                    if ui.add_enabled(can_create, create_btn).clicked() {
                        let name = std::mem::take(&mut self.library.collection_name);
                        self.toggle_collection_video(&name, video);
                    }
                });

                if self.auth.authenticated {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Playlists da conta Google")
                            .color(muted)
                            .size(11.5)
                            .strong(),
                    );
                    let remote_list = self.auth.remote_playlists.clone();
                    if remote_list.is_empty() && self.auth.remote_playlists_rx.is_some() {
                        ui.label(
                            RichText::new("Carregando playlists...")
                                .color(muted)
                                .size(11.5),
                        );
                    } else {
                        for (pl_id, pl_title) in remote_list {
                            let btn = egui::Button::new(
                                RichText::new(format!("+ Salvar em {pl_title}"))
                                    .color(palette.header_button_fg)
                                    .size(12.0),
                            )
                            .fill(palette.header_button_bg)
                            .corner_radius(egui::CornerRadius::same(14));
                            if ui.add(btn).clicked()
                                && let Some(id) = vid.clone()
                            {
                                self.start_add_to_remote_playlist(pl_id, pl_title, id, ctx);
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut self.playback.new_remote_playlist_title,
                            )
                            .hint_text("Nova playlist no YouTube...")
                            .desired_width(180.0),
                        );
                        let can_create_remote =
                            !self.playback.new_remote_playlist_title.trim().is_empty()
                                && self.auth.playlist_action_rx.is_none();
                        let create_remote_btn = egui::Button::new(
                            RichText::new("+ Criar no YouTube")
                                .color(palette.header_button_fg)
                                .size(12.0),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(egui::CornerRadius::same(14));
                        if ui
                            .add_enabled(can_create_remote, create_remote_btn)
                            .clicked()
                        {
                            let title =
                                std::mem::take(&mut self.playback.new_remote_playlist_title);
                            self.start_create_remote_playlist(title, vid.clone(), ctx);
                        }
                    });
                }
            });
        close_requested
    }

    pub(crate) fn render_timestamp_lines_ui(
        ui: &mut egui::Ui,
        lines: &[watch_extras::CachedTimestampLine],
        color: Color32,
        font_size: f32,
        link_color: Color32,
    ) -> Option<f64> {
        let mut clicked_secs = None;
        for line in lines {
            if line.timestamps.is_empty() {
                ui.label(RichText::new(&line.text).color(color).size(font_size));
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut cursor = 0;
                for ts in &line.timestamps {
                    if ts.byte_start > cursor
                        && let Some(prefix) = line.text.get(cursor..ts.byte_start)
                    {
                        ui.label(RichText::new(prefix).color(color).size(font_size));
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(&ts.label)
                                    .color(link_color)
                                    .size(font_size)
                                    .strong(),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(4)),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        clicked_secs = Some(ts.seconds as f64);
                    }
                    cursor = ts.byte_end;
                }
                if cursor < line.text.len()
                    && let Some(suffix) = line.text.get(cursor..)
                {
                    ui.label(RichText::new(suffix).color(color).size(font_size));
                }
            });
        }
        clicked_secs
    }

    pub(crate) fn render_cached_timestamp_lines_ui(
        &mut self,
        ui: &mut egui::Ui,
        lines: &[watch_extras::CachedTimestampLine],
        color: Color32,
        font_size: f32,
    ) {
        let link_color = if self.settings.theme.value == Theme::Dark {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        if let Some(secs) = Self::render_timestamp_lines_ui(ui, lines, color, font_size, link_color)
        {
            self.seek_to_seconds(secs);
        }
    }

    pub(crate) fn render_text_with_timestamps_ui(
        &mut self,
        ui: &mut egui::Ui,
        text: &str,
        color: Color32,
        font_size: f32,
    ) {
        let link_color = if self.settings.theme.value == Theme::Dark {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let mut clicked_secs = None;
        for line in text.lines() {
            let timestamps = watch_extras::extract_timestamps(line);
            if timestamps.is_empty() {
                ui.label(RichText::new(line).color(color).size(font_size));
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut cursor = 0;
                for ts in timestamps {
                    if ts.byte_start > cursor
                        && let Some(prefix) = line.get(cursor..ts.byte_start)
                    {
                        ui.label(RichText::new(prefix).color(color).size(font_size));
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(&ts.label)
                                    .color(link_color)
                                    .size(font_size)
                                    .strong(),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(4)),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        clicked_secs = Some(ts.seconds as f64);
                    }
                    cursor = ts.byte_end;
                }
                if cursor < line.len()
                    && let Some(suffix) = line.get(cursor..)
                {
                    ui.label(RichText::new(suffix).color(color).size(font_size));
                }
            });
        }
        if let Some(secs) = clicked_secs {
            self.seek_to_seconds(secs);
        }
    }

    #[cfg(test)]
    pub(crate) fn open_channel_for_metadata(
        &mut self,
        metadata: &VideoMetadata,
        ctx: &egui::Context,
    ) -> bool {
        let Some(target) = search::channel_target_for_metadata(metadata) else {
            self.channels.error = Some(
                "Não foi possível abrir o canal: os metadados do vídeo não fornecem um ID UC ou handle @ válido."
                    .into(),
            );
            return false;
        };
        self.open_channel(target, ctx)
    }

    pub(crate) fn complete_channel_request(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<ChannelPage, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.videos.active_request_id != Some(request_id)
            || self.channels.loading_target.as_ref() != Some(target)
        {
            return false;
        }
        self.channels.videos.active_request_id = None;
        self.channels.loading_target = None;
        self.channels.videos.rx = None;
        let is_my_channel = self.auth.my_channel.as_ref() == Some(target)
            || matches!(target, ChannelTarget::Handle(h) if h.eq_ignore_ascii_case("@voce"));
        let resolved = match result {
            Ok(page) => Ok(page),
            Err(error)
                if is_my_channel
                    || error.contains("does not have a")
                    || error.contains("has no videos") =>
            {
                Ok(search::empty_channel_page_for_target(target))
            }
            Err(error) => Err(error),
        };
        match resolved {
            Ok(mut page) => {
                if is_my_channel {
                    if page.title.as_deref().unwrap_or("").trim().is_empty() {
                        page.title =
                            self.auth.my_channel_title.clone().or_else(|| {
                                Some(self.pick("Seu canal", "Your channel").to_owned())
                            });
                    }
                    if page.avatar_url.is_none() {
                        page.avatar_url = self.auth.my_channel_avatar_url.clone();
                    }
                    if page.subscriber_count.is_none() {
                        page.subscriber_count = self.auth.my_channel_subscribers;
                    }
                }
                self.apply_channel_page(ctx, page);
            }
            Err(error) => {
                self.channels.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn complete_channel_outcome(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelFetchOutcome, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.videos.active_request_id != Some(request_id)
            || self.channels.loading_target.as_ref() != Some(target)
        {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.channels.from_cache = outcome.from_cache;
                self.complete_channel_request(request_id, target, Ok(outcome.page), ctx)
            }
            Err(error) => {
                self.channels.from_cache = false;
                self.complete_channel_request(request_id, target, Err(error), ctx)
            }
        }
    }

    pub(crate) fn close_channel_view(&mut self) {
        self.channels.close_view();
    }

    pub(crate) fn enqueue_video(&mut self, video: Video) -> Result<(), Video> {
        let title = video.title.clone();
        match self.queue.data.add(video) {
            Ok(()) => {
                self.set_pending_queue_warning();
                self.queue.last_rejected_video = None;
                self.queue.notice = Some(format!(
                    "Adicionado à fila ({}/{}): {title}",
                    self.queue.data.len(),
                    self.queue.data.limit()
                ));
                self.toasts.success(format!("Adicionado à fila: {title}"));
                self.persist_queue();
                Ok(())
            }
            Err(rejected) => {
                self.queue.notice = None;
                self.queue.error = Some(format!(
                    "Fila cheia ({}/{}): '{}' foi rejeitado sem perda.",
                    self.queue.data.len(),
                    self.queue.data.limit(),
                    rejected.title
                ));
                self.toasts.warning(format!(
                    "Fila cheia ({}/{})",
                    self.queue.data.len(),
                    self.queue.data.limit()
                ));
                self.queue.last_rejected_video = Some(rejected.clone());
                Err(rejected)
            }
        }
    }

    pub(crate) fn remove_from_queue(&mut self, index: usize) -> Option<Video> {
        let removed_entry_id = self.queue.data.entry_id(index);
        match self.queue.data.remove(index) {
            Some(removed) => {
                self.invalidate_feed_cache();
                let pending_id_matches = removed_entry_id
                    .zip(self.queue.pending_entry_id)
                    .is_some_and(|(removed, pending)| removed == pending);
                let legacy_video_matches = self.queue.pending_entry_id.is_none()
                    && self.queue.pending_video.as_ref() == Some(&removed);
                if pending_id_matches || legacy_video_matches {
                    self.cancel_pending_queue_video();
                }
                self.set_pending_queue_warning();
                self.queue.notice = Some(format!(
                    "Removido da fila ({}/{}): {}",
                    self.queue.data.len(),
                    self.queue.data.limit(),
                    removed.title
                ));
                self.persist_queue();
                Some(removed)
            }
            None => {
                self.queue.error = Some(format!("Índice inválido para remoção na fila: {index}."));
                None
            }
        }
    }

    pub(crate) fn reorder_queue(&mut self, from: usize, to: usize) -> bool {
        if self.queue.data.reorder(from, to) {
            self.set_pending_queue_warning();
            self.queue.notice = Some(format!("Fila reordenada ({} → {}).", from + 1, to + 1));
            self.persist_queue();
            true
        } else {
            self.queue.error = Some(format!(
                "Não foi possível reordenar a fila ({from} → {to})."
            ));
            false
        }
    }

    pub(crate) fn advance_queue_at(&mut self, now: Instant) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error =
                Some("Aguarde a confirma\u{00e7}\u{00e3}o do item atual antes de avan\u{00e7}ar novamente.".into());
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
            }
        }
        let Some((next_video, next_entry_id)) =
            self.queue.data.first_entry_id().and_then(|entry_id| {
                self.queue
                    .data
                    .videos()
                    .first()
                    .cloned()
                    .map(|video| (video, entry_id))
            })
        else {
            self.queue.error = Some("A fila está vazia; nenhum próximo vídeo para avançar.".into());
            return None;
        };
        let previous_player_pos_ms = self.current_player_live_pos_ms();
        #[cfg(windows)]
        self.sample_player_before_transition(now);
        self.flush_playback_history_at(now);
        let resume_target_ms = self.planned_resume_ms(&next_video);
        self.playback.quality = None;
        self.playback.qualities.clear();
        self.playback.quality_rx = None;
        self.playback.quality_error = None;
        self.playback.quality_fallback_url = None;
        self.playback.current_file_loaded = false;
        self.playback.cached_raw_path = None;
        self.playback.cached_raw_path_poll = None;
        self.playback.seek_drag = None;
        self.reset_decoding_diagnostics();
        self.selected_metadata = self.metadata_for(&next_video.url).cloned();
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.watch.overlay_cache = app_state::OverlayCache::default();
        self.playback.last_video_size_url = None;
        self.playback.last_video_size = None;
        self.activate_video_playback(&next_video, resume_target_ms, previous_player_pos_ms, now);
        self.watch.live.reset_for_video(
            Some(&next_video.url),
            history::youtube_video_id(&next_video.url),
        );
        if let Some(prev) = self.selected.clone() {
            self.record_video_back_stack(prev, &next_video.url);
        }
        self.playback.mini_player_paused = false;
        self.selected = Some(next_video.clone());
        self.invalidate_feed_cache();
        self.queue.pending_video = Some(next_video.clone());
        self.queue.pending_entry_id = Some(next_entry_id);
        self.queue.correlation.begin();
        self.sync_player_window_visibility();
        self.set_pending_queue_warning();
        Some(next_video)
    }

    pub(crate) fn advance_queue_with_frame(
        &mut self,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error =
                Some("Aguarde a confirma\u{00e7}\u{00e3}o do item atual antes de avan\u{00e7}ar novamente.".into());
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
                self.report_pending_queue_load_error();
            }
        }
        let Some((candidate, entry_id)) = self.queue.data.first_entry_id().and_then(|entry_id| {
            self.queue
                .data
                .videos()
                .first()
                .cloned()
                .map(|video| (video, entry_id))
        }) else {
            self.queue.error = Some("A fila está vazia; nenhum próximo vídeo para avançar.".into());
            return None;
        };
        if !self.open_video(candidate.clone(), frame, ctx) {
            self.queue.error = Some(
                "Não foi possível iniciar o próximo vídeo; o item foi mantido na fila.".into(),
            );
            return None;
        }
        self.queue.pending_video = Some(candidate.clone());
        self.queue.pending_entry_id = Some(entry_id);
        self.queue.correlation.begin();
        self.set_pending_queue_warning();
        self.queue.notice = Some(format!(
            "Abrindo da fila ({} itens; aguardando o início): {}",
            self.queue.data.len(),
            candidate.title
        ));
        Some(candidate)
    }

    pub(crate) fn request_play_queue_entry(&mut self, entry_id: u64) {
        if self.in_app_ui_pass {
            self.queue.pending_play_entry_id = Some(entry_id);
        } else {
            let _ = self.play_queue_entry_at(entry_id, Instant::now());
        }
    }

    pub(crate) fn play_queue_entry_at(&mut self, entry_id: u64, now: Instant) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error = Some(
                "Aguarde a confirmação do item atual antes de abrir outro item da fila.".into(),
            );
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
            }
        }
        let Some(index) = self.queue.data.index_of_entry(entry_id) else {
            self.queue.error = Some("Este item não está mais na fila.".into());
            return None;
        };
        if index > 0 {
            let _ = self.queue.data.move_to_top(index);
            self.persist_queue();
        }
        let next_video = self.queue.data.videos().first().cloned()?;
        let previous_player_pos_ms = self.current_player_live_pos_ms();
        #[cfg(windows)]
        self.sample_player_before_transition(now);
        self.flush_playback_history_at(now);
        let resume_target_ms = self.planned_resume_ms(&next_video);
        self.playback.quality = None;
        self.playback.qualities.clear();
        self.playback.quality_rx = None;
        self.playback.quality_error = None;
        self.playback.quality_fallback_url = None;
        self.playback.current_file_loaded = false;
        self.playback.cached_raw_path = None;
        self.playback.cached_raw_path_poll = None;
        self.playback.seek_drag = None;
        self.reset_decoding_diagnostics();
        self.selected_metadata = self.metadata_for(&next_video.url).cloned();
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.watch.overlay_cache = app_state::OverlayCache::default();
        self.playback.last_video_size_url = None;
        self.playback.last_video_size = None;
        self.activate_video_playback(&next_video, resume_target_ms, previous_player_pos_ms, now);
        self.watch.live.reset_for_video(
            Some(&next_video.url),
            history::youtube_video_id(&next_video.url),
        );
        if let Some(prev) = self.selected.clone() {
            self.record_video_back_stack(prev, &next_video.url);
        }
        self.playback.mini_player_paused = false;
        self.selected = Some(next_video.clone());
        self.invalidate_feed_cache();
        self.queue.pending_video = Some(next_video.clone());
        self.queue.pending_entry_id = Some(entry_id);
        self.queue.correlation.begin();
        self.sync_player_window_visibility();
        self.set_pending_queue_warning();
        Some(next_video)
    }

    pub(crate) fn play_queue_entry_with_frame(
        &mut self,
        entry_id: u64,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error = Some(
                "Aguarde a confirmação do item atual antes de abrir outro item da fila.".into(),
            );
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
                self.report_pending_queue_load_error();
            }
        }
        let Some(index) = self.queue.data.index_of_entry(entry_id) else {
            self.queue.error = Some("Este item não está mais na fila.".into());
            return None;
        };
        if index > 0 {
            let _ = self.queue.data.move_to_top(index);
            self.persist_queue();
        }
        let candidate = self.queue.data.videos().first().cloned()?;
        if !self.open_video(candidate.clone(), frame, ctx) {
            self.queue.error = Some(
                "Não foi possível iniciar o vídeo selecionado; o item foi mantido na fila.".into(),
            );
            return None;
        }
        self.queue.pending_video = Some(candidate.clone());
        self.queue.pending_entry_id = Some(entry_id);
        self.queue.correlation.begin();
        self.set_pending_queue_warning();
        self.queue.notice = Some(format!(
            "Abrindo da fila ({} itens; aguardando o início): {}",
            self.queue.data.len(),
            candidate.title
        ));
        Some(candidate)
    }

    pub(crate) fn move_queue_item_to_top(&mut self, index: usize) -> bool {
        if self.queue.data.move_to_top(index) {
            self.set_pending_queue_warning();
            self.queue.notice = Some("Item movido para tocar em seguida.".into());
            self.persist_queue();
            true
        } else {
            false
        }
    }

    pub(crate) fn shuffle_queue(&mut self) {
        if self.queue.data.len() <= 1 {
            return;
        }
        self.queue.data.shuffle();
        self.set_pending_queue_warning();
        self.queue.notice = Some(self.pick("Fila embaralhada.", "Queue shuffled.").into());
        self.persist_queue();
    }

    pub(crate) fn cycle_repeat_mode(&mut self) {
        self.queue.repeat_mode = self.queue.repeat_mode.cycle();
        let label = if self.ui_pt() {
            self.queue.repeat_mode.label_pt()
        } else {
            self.queue.repeat_mode.label_en()
        };
        let msg = format!("{}: {label}", self.pick("Modo de repetição", "Repeat mode"));
        self.queue.notice = Some(msg.clone());
        self.toasts.info(msg);
    }

    pub(crate) fn deduplicate_queue(&mut self) -> usize {
        let removed = self.queue.data.deduplicate();
        if let Some(pending_id) = self.queue.pending_entry_id
            && self.queue.data.index_of_entry(pending_id).is_none()
        {
            self.cancel_pending_queue_video();
        }
        self.set_pending_queue_warning();
        if removed > 0 {
            self.invalidate_feed_cache();
            self.queue.notice = Some(format!("{removed} duplicado(s) removido(s) da fila."));
            self.persist_queue();
        } else {
            self.queue.notice = Some("Nenhum item duplicado encontrado na fila.".into());
        }
        removed
    }

    pub(crate) fn clear_queue(&mut self) {
        self.cancel_pending_queue_video();
        self.queue.pending_advance = false;
        self.queue.pending_play_entry_id = None;
        if self.queue.data.is_empty() {
            self.set_pending_queue_warning();
            self.queue.error = None;
            self.queue.notice = Some("A fila j\u{00e1} est\u{00e1} vazia.".into());
            return;
        }
        self.queue.data.clear();
        self.invalidate_feed_cache();
        self.set_pending_queue_warning();
        self.queue.error = None;
        self.queue.last_rejected_video = None;
        self.queue.notice = Some("Fila de reprodução limpa.".into());
        self.persist_queue();
    }

    pub(crate) fn save_queue_as_collection(
        &mut self,
        requested_name: &str,
    ) -> Result<String, &'static str> {
        let resolved_name = if requested_name.trim().is_empty() {
            self.library
                .data
                .next_available_collection_name("Fila salva")
        } else {
            requested_name.trim().to_owned()
        };
        let (col_idx, added) = match self
            .library
            .data
            .create_collection_with_videos(&resolved_name, self.queue.data.videos())
        {
            Ok(res) => res,
            Err(err) => {
                self.queue.error = Some(err.to_owned());
                return Err(err);
            }
        };
        let created = self.library.data.collections[col_idx].name.clone();
        self.queue.save_collection_name.clear();
        self.save_library();
        self.queue.error = None;
        self.queue.notice = Some(format!(
            "Fila salva como coleção '{created}' ({added} vídeo(s))."
        ));
        Ok(created)
    }

    pub(crate) fn enqueue_all_videos(&mut self, videos: &[Video]) -> (usize, usize) {
        if videos.is_empty() {
            return (0, 0);
        }
        let mut added = 0usize;
        let mut rejected = 0usize;
        let mut first_rejected: Option<Video> = None;
        for video in videos {
            match self.queue.data.add(video.clone()) {
                Ok(()) => added += 1,
                Err(rej) => {
                    rejected += 1;
                    if first_rejected.is_none() {
                        first_rejected = Some(rej);
                    }
                }
            }
        }
        if added > 0 {
            self.invalidate_feed_cache();
            self.persist_queue();
        }
        if rejected == 0 {
            self.queue.error = None;
            self.queue.last_rejected_video = None;
            self.queue.notice = Some(format!(
                "{added} vídeo(s) adicionado(s) à fila ({}/{}).",
                self.queue.data.len(),
                self.queue.data.limit()
            ));
        } else {
            self.queue.last_rejected_video = first_rejected;
            self.queue.error = Some(format!(
                "Fila cheia ({}/{}): {added} adicionado(s) e {rejected} não coube(ram).",
                self.queue.data.len(),
                self.queue.data.limit()
            ));
        }
        (added, rejected)
    }

    pub(crate) fn play_all_videos(&mut self, videos: &[Video]) -> Option<Video> {
        let first = videos.first()?.clone();
        if videos.len() > 1 {
            let (added, rejected) = self.enqueue_all_videos(&videos[1..]);
            if rejected == 0 {
                self.library.notice = Some(format!(
                    "Reproduzindo '{}' e {added} item(ns) na fila.",
                    first.title
                ));
            } else {
                self.library.notice = Some(format!(
                    "Reproduzindo '{}': {added} na fila e {rejected} fora do limite.",
                    first.title
                ));
            }
        } else {
            self.library.notice = None;
        }
        Some(first)
    }

    pub(crate) fn report_pending_queue_playback_observed(&mut self) {
        self.queue.notice = Some(
            "Reprodu\u{00e7}\u{00e3}o observada; aguardando confirma\u{00e7}\u{00e3}o do libmpv."
                .into(),
        );
    }

    pub(crate) fn set_pending_queue_warning(&mut self) {
        self.queue.error = None;
    }

    pub(crate) fn report_pending_queue_load_error(&mut self) {
        if self.queue.pending_video.is_some() {
            self.queue.error = Some(
                "O libmpv relatou um erro durante a abertura; o item permanece na fila. Confira o player e remova-o manualmente pelo botão × se desejar.".into(),
            );
        }
    }

    pub(crate) fn cancel_pending_queue_video(&mut self) {
        self.queue.pending_video = None;
        self.queue.pending_entry_id = None;
        self.queue.correlation.cancel();
    }

    pub(crate) fn handle_queue_player_event(&mut self, event: PlaybackEvent) {
        match event {
            PlaybackEvent::StartFile { .. } => {
                self.playback.cached_raw_path_poll = None;
                self.watch.overlay_cache.chapters_last_poll = None;
                self.watch.overlay_cache.mpv_subs_last_poll = None;
                self.watch.overlay_cache.force_refresh = true;
            }
            PlaybackEvent::FileLoaded => {
                self.playback.current_file_loaded = true;
                self.playback.cached_raw_path_poll = None;
                self.watch.overlay_cache.chapters_last_poll = None;
                self.watch.overlay_cache.mpv_subs_last_poll = None;
                self.watch.overlay_cache.force_refresh = true;
            }
            PlaybackEvent::EndFile { .. } => {}
        }
        match self.queue.correlation.handle(event) {
            QueueLoadResult::Confirmed { .. } => {
                let entry_id = self.queue.pending_entry_id.take();
                if let Some(candidate) = self.queue.pending_video.take() {
                    let removed = match entry_id {
                        Some(entry_id) => self.queue.data.remove_entry(entry_id),
                        None => self
                            .queue
                            .data
                            .videos()
                            .iter()
                            .position(|video| *video == candidate)
                            .and_then(|index| self.queue.data.remove(index)),
                    };
                    if removed.is_some() {
                        self.invalidate_feed_cache();
                        self.persist_queue();
                        self.queue.error = None;
                        self.queue.notice = Some(format!(
                            "Reproduzindo da fila ({} restantes): {}",
                            self.queue.data.len(),
                            candidate.title
                        ));
                    } else {
                        self.queue.error = Some("O item confirmado nao esta mais na fila; nenhum outro item foi removido.".into());
                    }
                }
            }
            QueueLoadResult::Failed { .. } => {
                self.queue.pending_video = None;
                self.queue.pending_entry_id = None;
                self.queue.error =
                    Some("O libmpv falhou ao carregar o item; ele continua na fila.".into());
            }
            QueueLoadResult::None => {}
        }
    }

    pub(crate) fn queue_auto_advance_ready(&self, ended: bool) -> bool {
        ended
            && self.queue.pending_video.is_none()
            && !self.queue.data.is_empty()
            && self.selected.is_some()
            && self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.has_live_sample)
    }

    pub(crate) fn request_queue_advance(&mut self) {
        if self.in_app_ui_pass {
            self.queue.pending_advance = true;
        } else {
            let _ = self.advance_queue_at(Instant::now());
        }
    }

    pub(crate) fn begin_search_request(&mut self) -> Option<(u64, String)> {
        let query = self.feed.query.trim().to_owned();
        if query.is_empty() {
            return None;
        }
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feed.home.batch_cancel.take() {
            cancel.cancel();
        }
        self.feed.home.batch_rx = None;
        self.feed.home.batch_active_request_id = None;
        self.feed.home.rx = None;
        self.feed.home.subscriptions_active_request_id = None;
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        self.feed.home.title = None;
        self.feed.home.playlist_urls.clear();
        self.feed.home.playlist_total_results = None;
        self.library.active_remote_playlist_title = None;
        self.feed.search.next_request_id = self.feed.search.next_request_id.wrapping_add(1);
        let request_id = self.feed.search.next_request_id;
        self.feed.search.active_request_id = Some(request_id);
        self.channels.view_open = false;
        self.cancel_channel_subrequests();
        // Cancel any in-flight channel fetch when a new search replaces navigation.
        if self.channels.videos.active_request_id.is_some() {
            if let Some(cancel) = self.channels.videos.cancel.take() {
                cancel.cancel();
            }
            self.channels.videos.active_request_id = None;
            self.channels.loading_target = None;
            self.channels.videos.rx = None;
        }
        self.message = "Pesquisando...".into();
        Some((request_id, query))
    }

    pub(crate) fn complete_search_request(
        &mut self,
        request_id: u64,
        result: Result<Vec<SearchResult>, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.feed.search.active_request_id != Some(request_id) {
            return false;
        }
        self.feed.search.active_request_id = None;
        self.feed.search.rx = None;
        match result {
            Ok(results) => self.apply_search_results(ctx, results),
            Err(error) => self.message = error,
        }
        true
    }

    pub(crate) fn restore_cached_home_feed(&mut self, ctx: &egui::Context) -> bool {
        if !self.feed.home.cached_loaded || self.feed.home.cached_items.is_empty() {
            return false;
        }
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.playlist_urls = self.feed.home.cached_playlists.clone();
        self.feed.home.title = None;
        self.feed.home.limit_reached = false;
        let cached = self.feed.home.cached_items.clone();
        self.apply_search_results(ctx, cached);
        true
    }

    pub(crate) fn effective_subscriptions_cache_path(&self) -> Option<PathBuf> {
        if let Some(path) = &self.feed.home.subscriptions_disk_cache_path {
            return Some(path.clone());
        }
        #[cfg(not(test))]
        {
            oauth::subscriptions_cache_path().ok()
        }
        #[cfg(test)]
        {
            None
        }
    }

    pub(crate) fn ensure_subscriptions_disk_cache_loaded(&mut self) {
        if self.feed.home.subscriptions_disk_cache_loaded {
            return;
        }
        self.feed.home.subscriptions_disk_cache_loaded = true;
        let Some(path) = self.effective_subscriptions_cache_path() else {
            return;
        };
        let Ok(Some(disk_cache)) = oauth::load_subscriptions_cache_from_path(&path) else {
            return;
        };
        let (saved_at_unix_secs, channels, items, rss_cache) = disk_cache.into_parts();
        if !rss_cache.is_empty() && self.feed.home.subscriptions_rss_cache.is_empty() {
            self.feed.home.subscriptions_rss_cache = rss_cache;
        }
        if !self.feed.home.subscription_channels_loaded && !channels.is_empty() {
            self.feed.home.subscription_channels = channels;
            self.feed.home.subscription_channels_loaded = true;
            self.populate_channel_avatars_from_subscriptions();
        }
        if !self.feed.home.subscriptions_cached_loaded && !items.is_empty() {
            self.feed.home.subscriptions_cached_items = items;
            self.feed.home.subscriptions_cached_loaded = true;
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let age_secs = now_secs.saturating_sub(saved_at_unix_secs);
            self.feed.home.subscriptions_cached_at =
                Instant::now().checked_sub(Duration::from_secs(age_secs));
        }
    }

    pub(crate) fn persist_subscriptions_disk_cache(&self) {
        let Some(path) = self.effective_subscriptions_cache_path() else {
            return;
        };
        let saved_at_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let feed = search::HomeFeed {
            items: self.feed.home.subscriptions_cached_items.clone(),
            playlists: Vec::new(),
            channels: self.feed.home.subscription_channels.clone(),
        };
        let cache = oauth::SubscriptionsDiskCache::from_feed(
            &feed,
            &self.feed.home.subscriptions_rss_cache,
            saved_at_unix_secs,
        );
        let _ = oauth::save_subscriptions_cache_to_path(&path, &cache);
    }

    pub(crate) fn prioritized_subscription_channel_ids(&self) -> Vec<String> {
        let profile =
            UserProfile::build(&self.history.data, &self.queue.data, self.selected.as_ref());
        if profile.channel_weights.is_empty() || self.feed.home.subscription_channels.is_empty() {
            return Vec::new();
        }
        let mut ranked: Vec<(f64, usize, String)> = self
            .feed
            .home
            .subscription_channels
            .iter()
            .enumerate()
            .filter_map(|(idx, ch)| {
                let weight = profile
                    .channel_weights
                    .get(&recommendations::normalize_channel_key(&ch.title))
                    .copied()
                    .unwrap_or(0.0);
                (weight > 0.0).then(|| (weight, idx, ch.channel_id.clone()))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        ranked.into_iter().map(|(_, _, id)| id).collect()
    }

    pub(crate) fn start_fast_subscriptions_feed(
        &mut self,
        force_refresh_channels: bool,
        progressive_streaming: bool,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = self.auth.session.clone() else {
            self.start_browser_feed(":ytrec".into(), true, ctx);
            return;
        };
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feed.home.batch_cancel.take() {
            cancel.cancel();
        }
        self.feed.home.batch_rx = None;
        self.feed.home.batch_active_request_id = None;
        if let Some(cancel) = self.feed.search.cancel.take() {
            cancel.cancel();
        }
        self.feed.search.rx = None;
        self.feed.search.active_request_id = None;

        self.feed.home.subscriptions_next_request_id = self
            .feed
            .home
            .subscriptions_next_request_id
            .wrapping_add(1)
            .max(1);
        let request_id = self.feed.home.subscriptions_next_request_id;
        self.feed.home.subscriptions_active_request_id = Some(request_id);
        self.feed.home.subscriptions_progressive_streaming = progressive_streaming;
        self.feed.home.subscriptions_refreshing_in_background = !progressive_streaming;
        self.feed.home.limit_reached = false;
        self.feed.home.pending_items.clear();
        self.feed.home.subscription_channels_error = None;
        self.feed.home.title = Some("Inscri\u{00e7}\u{00f5}es: envios recentes".into());
        self.feed.home.playlist_urls.clear();
        self.message = if progressive_streaming {
            "Consultando suas inscri\u{00e7}\u{00f5}es...".into()
        } else {
            "Atualizando inscri\u{00e7}\u{00f5}es em segundo plano...".into()
        };

        let known_channels = (!force_refresh_channels
            && self.feed.home.subscription_channels_loaded
            && !self.feed.home.subscription_channels.is_empty())
        .then(|| self.feed.home.subscription_channels.clone());
        let prioritized_channel_ids = self.prioritized_subscription_channel_ids();
        let rss_cache = self.feed.home.subscriptions_rss_cache.clone();

        let cancel = process::Cancellation::default();
        self.feed.home.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.feed.home.rx = Some(receiver);
        let ctx_clone = ctx.clone();

        std::thread::spawn(move || {
            let result = if let Err(error) = session.refresh_if_needed(&cancel) {
                (Some(session), Err(error))
            } else {
                let progress_sender = sender.clone();
                let progress_ctx = ctx_clone.clone();
                let options = oauth::SubscriptionFeedOptions {
                    known_channels,
                    prioritized_channel_ids,
                    rss_cache,
                };
                let feed = oauth::subscriptions_feed_fast(
                    &session.access_token,
                    options,
                    &cancel,
                    move |progress| {
                        let _ = progress_sender.send(HomeFetchMessage::SubscriptionProgress {
                            request_id,
                            progress,
                        });
                        progress_ctx.request_repaint();
                    },
                );
                (Some(session), feed)
            };
            let _ = sender.send(HomeFetchMessage::SubscriptionFinished {
                request_id,
                token: result.0,
                result: result.1,
            });
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn open_subscriptions_section(&mut self, force_refresh: bool, ctx: &egui::Context) {
        self.ui.settings_open = false;
        self.ui.search_results_mode = false;
        self.ui.browse_section = BrowseSection::Subscriptions;
        self.close_channel_view();
        if self.selected.is_some() {
            if !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
        } else {
            self.sync_player_window_visibility();
        }
        self.feed.home.title = Some(
            self.pick(
                "Inscrições: envios recentes",
                "Subscriptions: recent uploads",
            )
            .into(),
        );
        self.feed.home.playlist_urls.clear();

        if !self.auth.authenticated {
            if self.followed_channels.data.is_empty() {
                self.followed_channels.feed_items.clear();
                self.followed_channels.new_video_urls.clear();
                self.apply_search_results(ctx, Vec::new());
                self.message = self
                    .pick(
                        "Siga canais localmente sem login para acompanhar os envios recentes aqui.",
                        "Follow channels locally without signing in to track recent uploads here.",
                    )
                    .into();
                return;
            }
            let cache_fresh = !force_refresh
                && !self.followed_channels.feed_items.is_empty()
                && self
                    .followed_channels
                    .loaded_at
                    .is_some_and(|at| at.elapsed() < SUBSCRIPTIONS_CACHE_TTL);
            if cache_fresh {
                let items = self.followed_channels.feed_items.clone();
                self.apply_search_results(ctx, items);
                self.message = format!(
                    "{} {}",
                    self.followed_channels.feed_items.len(),
                    self.pick(
                        "envios recentes dos canais seguidos localmente.",
                        "recent uploads from locally followed channels.",
                    )
                );
                return;
            }
            self.start_local_followed_feed(force_refresh, ctx);
            return;
        }

        self.ensure_subscriptions_disk_cache_loaded();

        let has_cached_items = self.feed.home.subscriptions_cached_loaded
            && !self.feed.home.subscriptions_cached_items.is_empty();
        if has_cached_items {
            let cached = self.feed.home.subscriptions_cached_items.clone();
            self.apply_search_results(ctx, cached);
            self.feed.home.subscription_channels_error = None;
        } else if !force_refresh && self.feed.home.subscriptions_active_request_id.is_none() {
            self.apply_search_results(ctx, Vec::new());
        }

        let cache_is_fresh = !force_refresh
            && self.feed.home.subscriptions_cached_loaded
            && self
                .feed
                .home
                .subscriptions_cached_at
                .is_some_and(|at| at.elapsed() < SUBSCRIPTIONS_CACHE_TTL);

        if cache_is_fresh {
            self.feed.home.subscriptions_refreshing_in_background = false;
            self.feed.home.subscriptions_progressive_streaming = false;
            self.message = if self.feed.home.subscriptions_cached_items.is_empty() {
                "Nenhum envio encontrado nas inscri\u{00e7}\u{00f5}es consultadas.".into()
            } else {
                format!(
                    "{} envios em cache dos canais que voc\u{00ea} acompanha.",
                    self.feed.home.subscriptions_cached_items.len()
                )
            };
            return;
        }

        if !force_refresh && self.feed.home.subscriptions_active_request_id.is_some() {
            if !has_cached_items {
                self.feed.home.subscriptions_progressive_streaming = true;
            }
            return;
        }

        let progressive_streaming = !has_cached_items;
        self.start_fast_subscriptions_feed(force_refresh, progressive_streaming, ctx);
    }

    pub(crate) fn save_followed_channels(&mut self) {
        #[cfg(test)]
        {
            if let Some(path) = self.followed_channels.path.as_deref() {
                if let Err(err) = followed_channels::save(path, &self.followed_channels.data) {
                    self.followed_channels.error = Some(err.to_string());
                } else {
                    self.followed_channels.error = None;
                }
            }
        }
        #[cfg(not(test))]
        {
            let target = self
                .followed_channels
                .path
                .clone()
                .or_else(|| followed_channels::path().ok());
            if let Some(path) = target {
                if let Err(err) = followed_channels::save(&path, &self.followed_channels.data) {
                    self.followed_channels.error = Some(err.to_string());
                } else {
                    self.followed_channels.path = Some(path);
                    self.followed_channels.error = None;
                }
            }
        }
    }

    pub(crate) fn mark_followed_video_seen(&mut self, channel_id: Option<&str>, video_url: &str) {
        let Some(video_id) = history::youtube_video_id(video_url) else {
            return;
        };
        let mut changed = self.followed_channels.new_video_urls.remove(video_url);
        if let Some(cid) = channel_id.map(str::trim).filter(|s| !s.is_empty())
            && self.followed_channels.data.mark_seen(cid, &video_id)
        {
            changed = true;
        }
        if changed {
            self.save_followed_channels();
        }
    }

    pub(crate) fn toggle_local_follow_channel(
        &mut self,
        channel_id: String,
        title: String,
        handle: Option<String>,
        url: String,
        avatar_url: Option<String>,
    ) {
        let target = if channel_id.starts_with("UC") {
            Some(ChannelTarget::ChannelId(channel_id.clone()))
        } else {
            handle.clone().map(ChannelTarget::Handle)
        };
        let is_following = self
            .followed_channels
            .data
            .is_following_target(target.as_ref(), Some(&channel_id));
        if is_following {
            if self
                .followed_channels
                .data
                .unfollow(target.as_ref(), Some(&channel_id))
            {
                self.save_followed_channels();
                let msg = format!(
                    "{}: {title}",
                    self.pick("Deixou de seguir localmente", "Unfollowed locally")
                );
                self.message = msg.clone();
                self.toasts.info(msg);
            }
        } else {
            let entry = followed_channels::FollowedChannel {
                channel_id,
                title: title.clone(),
                handle,
                url,
                avatar_url,
                followed_at_ms: followed_channels::now_ms(),
                last_seen_video_id: None,
            };
            match self.followed_channels.data.follow(entry) {
                Ok(_) => {
                    self.followed_channels.loaded_at = None;
                    self.save_followed_channels();
                    let msg = format!(
                        "{}: {title}",
                        self.pick("Seguindo canal localmente", "Following channel locally")
                    );
                    self.message = msg.clone();
                    self.toasts.success(msg);
                }
                Err(err) => {
                    self.message = err.to_owned();
                    self.toasts.warning(err);
                }
            }
        }
    }

    pub(crate) fn start_local_followed_feed(&mut self, _force_refresh: bool, ctx: &egui::Context) {
        if self.followed_channels.data.is_empty() {
            self.followed_channels.feed_items.clear();
            self.followed_channels.new_video_urls.clear();
            self.apply_search_results(ctx, Vec::new());
            return;
        }
        if let Some(cancel) = self.followed_channels.feed_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.followed_channels.feed_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.followed_channels.feed_rx = Some(receiver);
        let channels = self.followed_channels.data.channels.clone();
        self.message = self
            .pick(
                "Atualizando envios dos canais seguidos...",
                "Refreshing uploads from followed channels...",
            )
            .into();
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = followed_channels::fetch_local_followed_feed(&channels, &cancel);
            let _ = sender.send(result);
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_local_followed_feed(&mut self, ctx: &egui::Context) {
        let outcome = self
            .followed_channels
            .feed_rx
            .as_ref()
            .map(|rx| rx.try_recv());
        match outcome {
            Some(Ok(Ok(output))) => {
                self.followed_channels.feed_rx = None;
                self.followed_channels.feed_cancel = None;
                self.followed_channels.loaded_at = Some(Instant::now());
                self.followed_channels.feed_items = output.items.clone();
                self.followed_channels.new_video_urls = output.new_video_urls;
                if self.ui.browse_section == BrowseSection::Subscriptions
                    && !self.auth.authenticated
                    && !self.ui.search_results_mode
                    && !self.channels.view_open
                {
                    let count = output.items.len();
                    self.apply_search_results(ctx, output.items);
                    self.message = if count == 0 {
                        self.pick(
                            "Nenhum envio recente encontrado nos canais seguidos.",
                            "No recent uploads found in followed channels.",
                        )
                        .into()
                    } else if output.errors > 0 {
                        format!(
                            "{count} {} ({} {}).",
                            self.pick("envios carregados", "uploads loaded"),
                            output.errors,
                            self.pick("falha(s) de canal", "channel failure(s)")
                        )
                    } else {
                        format!(
                            "{count} {}",
                            self.pick(
                                "envios recentes dos canais seguidos.",
                                "recent uploads from followed channels.",
                            )
                        )
                    };
                }
            }
            Some(Ok(Err(error))) => {
                self.followed_channels.feed_rx = None;
                self.followed_channels.feed_cancel = None;
                self.followed_channels.error = Some(error.clone());
                if self.ui.browse_section == BrowseSection::Subscriptions
                    && !self.auth.authenticated
                {
                    self.message = error;
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.followed_channels.feed_rx = None;
                self.followed_channels.feed_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_download_video(
        &mut self,
        video: Video,
        format: downloads::DownloadFormat,
        ctx: &egui::Context,
    ) {
        let dir = downloads::DownloadManager::effective_download_dir(
            self.settings.prefs.value.download_dir_override.as_deref(),
        );
        let title = video.title.clone();
        self.downloads.manager.enqueue(video, format, dir, ctx);
        let msg = format!(
            "{}: {title}",
            self.pick("Download adicionado à fila", "Download added to queue")
        );
        self.downloads.notice = Some(msg.clone());
        self.toasts.info(msg);
    }

    pub(crate) fn poll_downloads(&mut self, ctx: &egui::Context) {
        if let Some((_id, result)) = self.downloads.manager.poll(ctx) {
            match result {
                Ok(()) => {
                    let msg = self
                        .pick("Download concluído.", "Download completed.")
                        .to_owned();
                    self.downloads.notice = Some(msg.clone());
                    self.toasts.success(msg);
                }
                Err(err) => {
                    let msg = format!(
                        "{}: {err}",
                        self.pick("Falha no download", "Download failed")
                    );
                    self.downloads.notice = Some(msg.clone());
                    self.toasts.warning(msg);
                }
            }
        }
    }

    pub(crate) fn start_remote_playlist_fetch(
        &mut self,
        list_id: &str,
        url: &str,
        ctx: &egui::Context,
    ) {
        if let Some(cancel) = self.queue.remote_playlist_cancel.take() {
            cancel.cancel();
        }
        self.queue.remote_playlist_next_id =
            self.queue.remote_playlist_next_id.wrapping_add(1).max(1);
        let request_id = self.queue.remote_playlist_next_id;
        self.queue.remote_playlist_active_id = Some(request_id);
        self.queue.remote_playlist_id = Some(list_id.to_owned());
        self.feed.home.title = Some(format!(
            "{} ({list_id})",
            self.pick("Playlist pública", "Public playlist")
        ));
        self.apply_search_results(ctx, Vec::new());
        self.message = self
            .pick(
                "Carregando vídeos da playlist...",
                "Loading playlist videos...",
            )
            .into();
        let cancel = process::Cancellation::default();
        self.queue.remote_playlist_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.queue.remote_playlist_rx = Some(receiver);
        let url_owned = url.to_owned();
        let list_id_owned = list_id.to_owned();
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_public_playlist_videos_with_cancel(&url_owned, 100, &cancel);
            let _ = sender.send(app_state::RemotePlaylistFetchMessage {
                request_id,
                playlist_id: list_id_owned,
                result,
            });
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_remote_playlist(&mut self, ctx: &egui::Context) {
        let outcome = self
            .queue
            .remote_playlist_rx
            .as_ref()
            .map(|rx| rx.try_recv());
        match outcome {
            Some(Ok(msg)) => {
                self.queue.remote_playlist_rx = None;
                self.queue.remote_playlist_cancel = None;
                if self.queue.remote_playlist_active_id != Some(msg.request_id) {
                    return;
                }
                self.queue.remote_playlist_active_id = None;
                match msg.result {
                    Ok(items) => {
                        let count = items.len();
                        self.queue.remote_playlist_id = Some(msg.playlist_id.clone());
                        self.apply_search_results(ctx, items);
                        self.message = if count == 0 {
                            self.pick(
                                "Nenhum vídeo público encontrado nesta playlist.",
                                "No public videos found in this playlist.",
                            )
                            .into()
                        } else {
                            format!(
                                "{count} {} ({})",
                                self.pick(
                                    "vídeos carregados na playlist",
                                    "videos loaded in playlist"
                                ),
                                msg.playlist_id
                            )
                        };
                    }
                    Err(err) => {
                        self.message = err;
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.queue.remote_playlist_rx = None;
                self.queue.remote_playlist_cancel = None;
                self.queue.remote_playlist_active_id = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_community_extras_fetch(&mut self, video_url: &str, ctx: &egui::Context) {
        if let Some(cancel) = self.watch.community_cancel.take() {
            cancel.cancel();
        }
        self.watch.community_rx = None;
        self.watch.community_active_id = None;
        self.watch.show_original_title = false;
        let ryd_enabled = self.settings.prefs.value.ryd_enabled;
        let dearrow_enabled = self.settings.prefs.value.dearrow_enabled;
        if !ryd_enabled && !dearrow_enabled {
            return;
        }
        let Some(video_id) = history::youtube_video_id(video_url) else {
            return;
        };
        if let Some(cached) = self.watch.community_cache.get(&video_id) {
            let needs_ryd = ryd_enabled && cached.ryd.is_none();
            let needs_dearrow = dearrow_enabled && cached.dearrow.is_none();
            if !needs_ryd && !needs_dearrow {
                return;
            }
        }
        self.watch.community_next_id = self.watch.community_next_id.wrapping_add(1).max(1);
        let request_id = self.watch.community_next_id;
        self.watch.community_active_id = Some(request_id);
        let cancel = process::Cancellation::default();
        self.watch.community_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.watch.community_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let outcome = community_apis::fetch_community_extras(
                &video_id,
                ryd_enabled,
                dearrow_enabled,
                &cancel,
            );
            let _ = sender.send(app_state::CommunityExtrasFetchMessage {
                request_id,
                outcome,
            });
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_community_extras(&mut self) {
        let outcome = self.watch.community_rx.as_ref().map(|rx| rx.try_recv());
        match outcome {
            Some(Ok(msg)) => {
                self.watch.community_rx = None;
                self.watch.community_cancel = None;
                if self.watch.community_active_id == Some(msg.request_id) {
                    self.watch.community_active_id = None;
                    let vid = msg.outcome.video_id.clone();
                    self.watch.community_cache.insert(vid, msg.outcome);
                    self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.watch.community_rx = None;
                self.watch.community_cancel = None;
                self.watch.community_active_id = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_ytdlp_check(&mut self, ctx: &egui::Context) {
        if self.settings.ytdlp_rx.is_some() {
            return;
        }
        let cancel = process::Cancellation::default();
        self.settings.ytdlp_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.settings.ytdlp_rx = Some(receiver);
        self.settings.ytdlp_status = Some((
            true,
            self.pick(
                "Consultando versão mais recente do yt-dlp no GitHub...",
                "Checking latest yt-dlp version on GitHub...",
            )
            .into(),
        ));
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = updater::check_for_update(&cancel);
            let _ = sender.send(app_state::YtDlpUpdaterMessage::Checked(result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn start_ytdlp_update(
        &mut self,
        download_url: String,
        install_path: PathBuf,
        version: String,
        ctx: &egui::Context,
    ) {
        if self.settings.ytdlp_rx.is_some() {
            return;
        }
        let cancel = process::Cancellation::default();
        self.settings.ytdlp_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.settings.ytdlp_rx = Some(receiver);
        self.settings.ytdlp_status = Some((
            true,
            format!(
                "{} ({version})...",
                self.pick(
                    "Baixando e instalando yt-dlp",
                    "Downloading and installing yt-dlp"
                )
            ),
        ));
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = updater::download_and_install(&download_url, &install_path, &cancel);
            let _ = sender.send(app_state::YtDlpUpdaterMessage::Updated { version, result });
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn poll_ytdlp_updater(&mut self) {
        let outcome = self.settings.ytdlp_rx.as_ref().map(|rx| rx.try_recv());
        match outcome {
            Some(Ok(app_state::YtDlpUpdaterMessage::Checked(Ok(info)))) => {
                self.settings.ytdlp_rx = None;
                self.settings.ytdlp_cancel = None;
                let msg = if info.update_available {
                    format!(
                        "{}: {} -> {}",
                        self.pick("Atualização disponível", "Update available"),
                        info.current_version.as_deref().unwrap_or("?"),
                        info.latest_version
                    )
                } else {
                    format!(
                        "{} ({})",
                        self.pick("yt-dlp já está atualizado", "yt-dlp is up to date"),
                        info.latest_version
                    )
                };
                self.settings.ytdlp_check = Some(info);
                self.settings.ytdlp_status = Some((true, msg));
            }
            Some(Ok(app_state::YtDlpUpdaterMessage::Checked(Err(err)))) => {
                self.settings.ytdlp_rx = None;
                self.settings.ytdlp_cancel = None;
                self.settings.ytdlp_status = Some((false, err));
            }
            Some(Ok(app_state::YtDlpUpdaterMessage::Updated { version, result })) => {
                self.settings.ytdlp_rx = None;
                self.settings.ytdlp_cancel = None;
                match result {
                    Ok(path) => {
                        if let Some(check) = &mut self.settings.ytdlp_check {
                            check.current_version = Some(version.clone());
                            check.update_available = false;
                            check.install_path = path.clone();
                        }
                        self.settings.ytdlp_status = Some((
                            true,
                            format!(
                                "{} ({version}): {}",
                                self.pick("yt-dlp atualizado", "yt-dlp updated"),
                                path.display()
                            ),
                        ));
                    }
                    Err(err) => {
                        self.settings.ytdlp_status = Some((false, err));
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.settings.ytdlp_rx = None;
                self.settings.ytdlp_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn maybe_auto_check_app_updates(&mut self, ctx: &egui::Context) {
        if self.settings.app_update_auto_checked || !self.settings.prefs.value.auto_check_updates {
            return;
        }
        self.settings.app_update_auto_checked = true;
        self.start_app_update_check(true, ctx);
    }

    pub(crate) fn open_settings_tab(&mut self, tab: app_state::SettingsTab, _ctx: &egui::Context) {
        self.settings.active_tab = tab;
        self.ui.settings_open = true;
        self.sync_player_window_visibility();
    }

    pub(crate) fn start_app_update_check(&mut self, silent: bool, ctx: &egui::Context) {
        if self.settings.app_update_rx.is_some() {
            return;
        }
        let cancel = process::Cancellation::default();
        self.settings.app_update_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.settings.app_update_rx = Some(receiver);
        if !silent {
            self.settings.app_update_status = Some((
                true,
                self.pick(
                    "Consultando release mais recente do FastTube no GitHub...",
                    "Checking latest FastTube release on GitHub...",
                )
                .into(),
            ));
        }
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = updater::check_fasttube_update(&cancel);
            let _ = sender.send(app_state::AppUpdaterMessage::Checked { silent, result });
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn start_app_update_download(&mut self, ctx: &egui::Context) {
        if self.settings.app_update_rx.is_some() {
            return;
        }
        let Some(check) = self.settings.app_update_check.clone() else {
            return;
        };
        if !check.update_available {
            return;
        }
        let cancel = process::Cancellation::default();
        self.settings.app_update_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.settings.app_update_rx = Some(receiver);
        self.settings.app_update_prepared = None;
        self.settings.app_update_status = Some((
            true,
            format!(
                "{} (v{})...",
                self.pick(
                    "Baixando e verificando SHA-256 da atualização",
                    "Downloading and verifying update SHA-256",
                ),
                check.latest_version
            ),
        ));
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = updater::download_and_prepare_app_update(&check, &cancel);
            let _ = sender.send(app_state::AppUpdaterMessage::Prepared(result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn apply_prepared_app_update(&mut self, ctx: &egui::Context) {
        let Some(prepared) = self.settings.app_update_prepared.clone() else {
            return;
        };
        match updater::launch_app_update_helper(&prepared) {
            Ok(()) => {
                self.settings.app_update_status = Some((
                    true,
                    format!(
                        "{} (v{})...",
                        self.pick(
                            "Aplicando atualização e reiniciando",
                            "Applying update and restarting",
                        ),
                        prepared.version
                    ),
                ));
                self.desktop.tray_quit_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(err) => {
                self.settings.app_update_status = Some((false, err.clone()));
                self.toasts.error(err);
            }
        }
    }

    pub(crate) fn active_app_update_available(&self) -> Option<&updater::AppUpdateCheck> {
        let check = self.settings.app_update_check.as_ref()?;
        if !check.update_available {
            return None;
        }
        if self
            .settings
            .prefs
            .value
            .skipped_app_update_version
            .as_deref()
            .is_some_and(|skipped| skipped == check.latest_version)
        {
            return None;
        }
        Some(check)
    }

    pub(crate) fn poll_app_updater(&mut self, ctx: &egui::Context) {
        let outcome = self.settings.app_update_rx.as_ref().map(|rx| rx.try_recv());
        match outcome {
            Some(Ok(app_state::AppUpdaterMessage::Checked { silent, result })) => {
                self.settings.app_update_rx = None;
                self.settings.app_update_cancel = None;
                match result {
                    Ok(info) => {
                        if info.update_available {
                            self.settings.app_update_banner_dismissed = false;
                            let msg = format!(
                                "{}: v{} -> v{}",
                                self.pick(
                                    "Nova versão do FastTube disponível",
                                    "New FastTube version available"
                                ),
                                info.current_version,
                                info.latest_version
                            );
                            self.settings.app_update_status = Some((true, msg.clone()));
                            let skipped = self
                                .settings
                                .prefs
                                .value
                                .skipped_app_update_version
                                .as_deref()
                                == Some(info.latest_version.as_str());
                            if !skipped {
                                self.toasts.info(msg);
                            }
                        } else if !silent {
                            self.settings.app_update_status = Some((
                                true,
                                format!(
                                    "{} (v{})",
                                    self.pick(
                                        "O FastTube já está na versão mais recente",
                                        "FastTube is already up to date",
                                    ),
                                    info.current_version
                                ),
                            ));
                        }
                        self.settings.app_update_check = Some(info);
                    }
                    Err(err) => {
                        if !silent {
                            self.settings.app_update_status = Some((false, err));
                        }
                    }
                }
            }
            Some(Ok(app_state::AppUpdaterMessage::Prepared(result))) => {
                self.settings.app_update_rx = None;
                self.settings.app_update_cancel = None;
                match result {
                    Ok(prepared) => {
                        self.settings.app_update_prepared = Some(prepared.clone());
                        self.settings.app_update_status = Some((
                            true,
                            format!(
                                "{} v{}.",
                                self.pick(
                                    "Atualização verificada e pronta para instalar:",
                                    "Update verified and ready to install:",
                                ),
                                prepared.version
                            ),
                        ));
                        self.apply_prepared_app_update(ctx);
                    }
                    Err(err) => {
                        self.settings.app_update_status = Some((false, err.clone()));
                        self.toasts.error(err);
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.settings.app_update_rx = None;
                self.settings.app_update_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn start_subscription_channels_fetch(&mut self, ctx: &egui::Context) {
        if self.feed.home.channels_rx.is_some() || !self.auth.authenticated {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        if let Some(cancel) = self.feed.home.channels_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.feed.home.channels_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.feed.home.channels_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| oauth::list_subscriptions(&session.access_token, &cancel));
            let _ = sender.send((Some(session), result));
            ctx_clone.request_repaint();
        });
    }

    pub(crate) fn start_fast_home_feed(&mut self, append: bool, ctx: &egui::Context) {
        if append && (self.feed.home.batch_rx.is_some() || !self.feed.home.has_more) {
            return;
        }
        if let Some(cancel) = self.feed.home.batch_cancel.take() {
            cancel.cancel();
        }
        self.feed.home.batch_next_request_id = self.feed.home.batch_next_request_id.wrapping_add(1);
        let request_id = self.feed.home.batch_next_request_id;
        self.feed.home.batch_active_request_id = Some(request_id);
        let page = if append { self.feed.home.page } else { 0 };
        self.feed.home.title = None;
        self.feed.home.limit_reached = false;
        if !append && self.feed.videos.is_empty() {
            self.feed.home.has_more = true;
        }

        let profile =
            UserProfile::build(&self.history.data, &self.queue.data, self.selected.as_ref());
        let plan = RecommendationEngine::build_home_query_plan(
            &profile,
            &self.feed.home.subscription_channels,
            &self.feed.filter,
            page,
        );
        let request = search::HomeBatchRequest {
            video_queries: plan.video_queries,
            channel_targets: plan.channel_targets,
            playlist_query: plan.playlist_query,
            shorts_query: plan.shorts_query,
        };
        let cancel = process::Cancellation::default();
        self.feed.home.batch_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.feed.home.batch_rx = Some(receiver);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = search::fetch_home_batch(&request, &cancel);
            let _ = sender.send(HomeBatchFetchMessage {
                request_id,
                page,
                result,
            });
            ctx_clone.request_repaint();
        });
    }

    #[cfg(test)]
    pub(crate) fn start_public_home(&mut self, ctx: &egui::Context) {
        if self.feed.search.rx.is_some()
            || self.feed.home.rx.is_some()
            || self.feed.home.batch_rx.is_some()
        {
            return;
        }
        self.ui.browse_section = BrowseSection::Home;
        self.ui.search_results_mode = false;
        self.feed.filter = FeedFilter::All;
        if self.restore_cached_home_feed(ctx) {
            return;
        }
        self.start_fast_home_feed(false, ctx);
    }

    pub(crate) fn clear_visual_caches(&mut self, ctx: &egui::Context) {
        let current_videos = std::mem::take(&mut self.feed.videos);
        self.thumbnails.reset(ctx, &current_videos);
        self.thumbnails.clear_storyboards(ctx);
        self.avatars.evict_all_gpu_textures(ctx);
        self.feed.videos = current_videos;
        self.playback.qualities.clear();
        search::clear_in_memory_caches();
        self.prune_session_caches();
        self.feed.cache_dirty = true;
        self.settings.buffer.notice = Some(
            "Miniaturas e resoluções em cache foram liberadas. O feed continua disponível e os dados locais não foram alterados."
                .into(),
        );
    }

    pub(crate) fn start_search(&mut self, ctx: &egui::Context) {
        let Some((request_id, query)) = self.begin_search_request() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        if let Some(cancel) = self.feed.search.cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.feed.search.cancel = Some(cancel.clone());
        self.feed.search.rx = Some(receiver);
        let filters =
            search::effective_search_filters(self.feed.search_filters, self.feed.search_chip);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = search::find_filtered_with_cancel(&query, filters, &cancel);
            let _ = sender.send(SearchFetchMessage { request_id, result });
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_liked_videos_fetch(&mut self, force_refresh: bool, ctx: &egui::Context) {
        if !self.auth.authenticated {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        if !force_refresh
            && (self.library.liked_videos.loaded || self.library.liked_videos.rx.is_some())
        {
            return;
        }
        if let Some(cancel) = self.library.liked_videos.cancel.take() {
            cancel.cancel();
        }
        self.library.liked_videos.next_request_id =
            self.library.liked_videos.next_request_id.wrapping_add(1);
        let request_id = self.library.liked_videos.next_request_id;
        self.library.liked_videos.active_request_id = Some(request_id);
        self.library.liked_videos.items.clear();
        self.library.liked_videos.seen_keys.clear();
        self.library.liked_videos.total_results = None;
        self.library.liked_videos.loaded = false;
        self.library.liked_videos.error = None;
        self.library.liked_videos.revision = self.library.liked_videos.revision.wrapping_add(1);

        let cancel = process::Cancellation::default();
        self.library.liked_videos.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.library.liked_videos.rx = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Err(error) = session.refresh_if_needed(&cancel) {
                let _ = sender.send(LikedVideosFetchMessage::Finished {
                    request_id,
                    token: Some(session),
                    total_results: None,
                    result: Err(error),
                });
                ctx.request_repaint();
                return;
            }
            let progress_sender = sender.clone();
            let progress_ctx = ctx.clone();
            let outcome = oauth::liked_videos_feed_progressive(
                &session.access_token,
                &cancel,
                move |progress| {
                    let _ = progress_sender.send(LikedVideosFetchMessage::Progress {
                        request_id,
                        batch: progress.batch,
                        total_results: progress.total_results,
                    });
                    progress_ctx.request_repaint();
                },
            );
            let (total_results, result) = match outcome {
                Ok(out) => (out.total_results, Ok(())),
                Err(err) => (None, Err(err)),
            };
            let _ = sender.send(LikedVideosFetchMessage::Finished {
                request_id,
                token: Some(session),
                total_results,
                result,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn cancel_liked_videos_fetch(&mut self) {
        if let Some(cancel) = self.library.liked_videos.cancel.take() {
            cancel.cancel();
        }
        self.library.liked_videos.rx = None;
        self.library.liked_videos.active_request_id = None;
        self.library.liked_videos.revision = self.library.liked_videos.revision.wrapping_add(1);
    }

    pub(crate) fn poll_liked_videos(&mut self) {
        let mut messages = Vec::new();
        let mut disconnected = false;
        if let Some(receiver) = &self.library.liked_videos.rx {
            loop {
                match receiver.try_recv() {
                    Ok(msg) => messages.push(msg),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut changed = false;
        let mut metadata_changed = false;
        for msg in messages {
            match msg {
                LikedVideosFetchMessage::Progress {
                    request_id,
                    batch,
                    total_results,
                } => {
                    if self.library.liked_videos.active_request_id != Some(request_id) {
                        continue;
                    }
                    if total_results.is_some()
                        && self.library.liked_videos.total_results != total_results
                    {
                        self.library.liked_videos.total_results = total_results;
                        changed = true;
                    }
                    for item in batch {
                        let key = history::youtube_video_id(&item.video.url)
                            .unwrap_or_else(|| item.video.url.clone());
                        if self.library.liked_videos.seen_keys.insert(key) {
                            self.video_metadata
                                .insert(item.video.url.clone(), item.metadata.clone());
                            metadata_changed = true;
                            self.library.liked_videos.items.push(item);
                            changed = true;
                        }
                    }
                }
                LikedVideosFetchMessage::Finished {
                    request_id,
                    token,
                    total_results,
                    result,
                } => {
                    if self.library.liked_videos.active_request_id != Some(request_id) {
                        continue;
                    }
                    self.library.liked_videos.rx = None;
                    self.library.liked_videos.cancel = None;
                    self.library.liked_videos.active_request_id = None;
                    if let Some(session) = token
                        && (self.auth.session.is_some() || self.auth.authenticated)
                    {
                        self.auth.authenticated = true;
                        let _ = self.set_oauth_session(Some(session));
                    }
                    if total_results.is_some() {
                        self.library.liked_videos.total_results = total_results;
                    }
                    match result {
                        Ok(()) => {
                            self.library.liked_videos.loaded = true;
                            self.library.liked_videos.error = None;
                        }
                        Err(err) => {
                            self.library.liked_videos.error = Some(err);
                        }
                    }
                    changed = true;
                }
            }
        }
        if disconnected && self.library.liked_videos.rx.is_some() {
            self.library.liked_videos.rx = None;
            self.library.liked_videos.cancel = None;
            self.library.liked_videos.active_request_id = None;
            if self.library.liked_videos.error.is_none() && !self.library.liked_videos.loaded {
                self.library.liked_videos.error =
                    Some("A consulta dos vídeos com Gostei foi interrompida.".into());
            }
            changed = true;
        }
        if metadata_changed {
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        }
        if changed {
            self.library.liked_videos.revision = self.library.liked_videos.revision.wrapping_add(1);
        }
    }

    pub(crate) fn start_browser_feed(
        &mut self,
        target: String,
        interactive: bool,
        ctx: &egui::Context,
    ) {
        if target == ":google-liked" && !interactive && self.auth.authenticated {
            self.start_liked_videos_fetch(false, ctx);
            return;
        }
        if target == ":ytrec" && self.feed.home.rx.is_some() {
            return;
        }
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feed.home.batch_cancel.take() {
            cancel.cancel();
        }
        self.feed.home.batch_rx = None;
        self.feed.home.batch_active_request_id = None;
        self.feed.home.subscriptions_active_request_id = None;
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        if let Some(cancel) = self.feed.search.cancel.take() {
            cancel.cancel();
        }
        self.feed.search.rx = None;
        self.feed.search.active_request_id = None;
        self.ui.search_results_mode = false;
        self.close_channel_view();
        self.feed.home.playlist_urls.clear();
        self.feed.home.playlist_total_results = None;
        self.feed.home.limit_reached = false;
        self.feed.home.pending_items.clear();
        if target == ":ytrec" {
            self.feed.home.subscription_channels_error = None;
        }
        self.apply_search_results(ctx, Vec::new());
        let title = if target == ":ytrec" {
            "Inscrições: envios recentes"
        } else if target == ":google-playlists" {
            "Playlists da conta Google"
        } else if target == ":google-liked" {
            "Vídeos com Gostei"
        } else if target.starts_with("fasttube-google-playlist:") {
            "Vídeos da playlist"
        } else {
            "Playlist do YouTube"
        };
        self.feed.home.title = Some(title.into());
        self.message = if target == ":ytrec" {
            "Consultando suas inscrições..."
        } else if target == ":google-playlists" || target.starts_with("fasttube-google-playlist:") {
            "Consultando suas playlists Google..."
        } else if target == ":google-liked" {
            "Consultando vídeos com Gostei..."
        } else {
            "Consultando a playlist..."
        }
        .into();
        let cancel = process::Cancellation::default();
        self.feed.home.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.feed.home.rx = Some(receiver);
        let ctx = ctx.clone();
        let saved_session = self.auth.session.clone();
        std::thread::spawn(move || {
            let is_oauth_target = target == ":ytrec"
                || target == ":google-playlists"
                || target == ":google-liked"
                || target.starts_with("fasttube-google-playlist:");
            let result = if is_oauth_target {
                let (token_opt, ready_session) = match saved_session {
                    Some(mut session) => match session.refresh_if_needed(&cancel) {
                        Ok(()) => (Some(session.clone()), Ok(session)),
                        Err(err) => (Some(session), Err(err)),
                    },
                    None if interactive => match oauth::authorize(&cancel) {
                        Ok(session) => (Some(session.clone()), Ok(session)),
                        Err(err) => (None, Err(err)),
                    },
                    None => (
                        None,
                        Err("Entre na conta para consultar suas inscrições.".into()),
                    ),
                };
                match ready_session {
                    Ok(session) => {
                        if let Some(id) = target.strip_prefix("fasttube-google-playlist:") {
                            let progress_sender = sender.clone();
                            let progress_ctx = ctx.clone();
                            let outcome = oauth::playlist_videos_feed_progressive(
                                &session.access_token,
                                id,
                                &cancel,
                                move |progress| {
                                    let _ =
                                        progress_sender.send(HomeFetchMessage::PlaylistProgress {
                                            batch: progress.batch,
                                            total_results: progress.total_results,
                                        });
                                    progress_ctx.request_repaint();
                                },
                            );
                            let (total_results, res) = match outcome {
                                Ok(out) => (out.total_results, Ok(())),
                                Err(err) => (None, Err(err)),
                            };
                            let _ = sender.send(HomeFetchMessage::PlaylistFinished {
                                token: token_opt,
                                total_results,
                                result: res,
                            });
                            ctx.request_repaint();
                            return;
                        }
                        let feed = if target == ":google-playlists" {
                            oauth::playlists_feed(&session.access_token, &cancel)
                        } else if target == ":google-liked" {
                            oauth::liked_videos_feed(&session.access_token, &cancel)
                        } else {
                            let progress_sender = sender.clone();
                            let progress_ctx = ctx.clone();
                            oauth::subscriptions_feed(
                                &session.access_token,
                                &cancel,
                                move |progress| {
                                    let _ =
                                        progress_sender.send(HomeFetchMessage::Progress(progress));
                                    progress_ctx.request_repaint();
                                },
                            )
                        };
                        (token_opt, feed)
                    }
                    Err(error) => {
                        if target.starts_with("fasttube-google-playlist:") {
                            let _ = sender.send(HomeFetchMessage::PlaylistFinished {
                                token: token_opt,
                                total_results: None,
                                result: Err(error),
                            });
                            ctx.request_repaint();
                            return;
                        }
                        (token_opt, Err(error))
                    }
                }
            } else {
                let feed = session::youtube_cookies(false, &cancel)
                    .and_then(|cookies| search::session_feed(&cookies, &target, &cancel));
                (saved_session, feed)
            };
            let _ = sender.send(HomeFetchMessage::Finished(result.0, result.1));
            ctx.request_repaint();
        });
    }

    pub(crate) fn start_reauth(&mut self, ctx: &egui::Context) {
        if self.auth.reauth_rx.is_some() {
            return;
        }
        let cancel = process::Cancellation::default();
        self.auth.reauth_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.reauth_rx = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(oauth::authorize(&cancel));
            ctx.request_repaint();
        });
    }

    pub(crate) fn cancel_reauth(&mut self) {
        if let Some(cancel) = self.auth.reauth_cancel.take() {
            cancel.cancel();
        }
        self.auth.reauth_rx = None;
    }

    pub(crate) fn start_video_rating(
        &mut self,
        video_id: String,
        rating: Option<oauth::VideoRating>,
        ctx: &egui::Context,
    ) {
        if self.auth.rating_rx.is_some() {
            return;
        }
        let Some(session) = self.auth.session.as_ref() else {
            self.message = "Entre com Google para avaliar este vídeo.".into();
            self.start_reauth(ctx);
            return;
        };
        let mut session = session.clone();
        let cancel = process::Cancellation::default();
        self.auth.rating_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.auth.rating_rx = Some(receiver);
        let api_rating = rating.unwrap_or(oauth::VideoRating::None);
        self.message = "Enviando avaliação...".into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = session
                .refresh_if_needed(&cancel)
                .and_then(|()| {
                    oauth::rate_video(&session.access_token, &video_id, api_rating, &cancel)
                })
                .map(|()| (video_id, rating));
            let _ = sender.send((Some(session), result));
            ctx.request_repaint();
        });
    }

    pub(crate) fn set_oauth_session(
        &mut self,
        session: Option<oauth::TokenSession>,
    ) -> Option<String> {
        if session.is_none() {
            self.auth.ratings.clear();
            self.auth.subscriptions.clear();
            self.auth.my_channel = None;
            self.auth.my_channel_title = None;
            self.auth.my_channel_avatar_url = None;
            self.auth.my_channel_subscribers = None;
            self.auth.my_channel_open_when_ready = false;
            self.auth.my_channel_fetch_attempted = false;
            self.auth.remote_playlists.clear();
            self.auth.youtube_web_session = None;
            self.auth.youtube_web_session_checked = false;
            self.library.liked_videos.reset();
        }
        self.auth.session = session;
        #[cfg(test)]
        return None;
        #[cfg(not(test))]
        match self.auth.session.as_ref() {
            Some(session) => oauth::save_session(session).err(),
            None => oauth::delete_session().err(),
        }
    }

    #[cfg(test)]
    pub(crate) fn cancel_login(&mut self) {
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        self.feed.home.rx = None;
        self.feed.home.subscriptions_active_request_id = None;
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        self.feed.home.pending_items.clear();
        self.feed.home.title = None;
        self.feed.home.limit_reached = false;
        self.message = "Login cancelado. Você pode tentar novamente.".into();
    }

    #[cfg(test)]
    pub(crate) fn cancel_home_feed(&mut self, ctx: &egui::Context) {
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        self.feed.home.rx = None;
        self.feed.home.subscriptions_active_request_id = None;
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        if !self.feed.home.pending_items.is_empty() {
            let partial = std::mem::take(&mut self.feed.home.pending_items);
            self.apply_search_results(ctx, partial);
        }
        self.message =
            "Carregamento cancelado. Os resultados já recebidos permanecem disponíveis.".into();
    }

    pub(crate) fn poll_async_tasks(&mut self, ctx: &egui::Context) {
        self.poll_reauth(ctx);
        self.poll_rating();
        self.poll_subscriptions(ctx);
        self.poll_my_channel(ctx);
        self.poll_remote_playlists(ctx);
        self.poll_playlist_action(ctx);
        self.poll_report(ctx);
        self.poll_subscription_channels();
        self.poll_home_batch(ctx);
        self.poll_home(ctx);
        self.poll_liked_videos();
        self.poll_quality();
        self.poll_live_watch(ctx);
        self.maybe_auto_activate_subtitle(ctx);
        self.maybe_start_deferred_quality_fallback(ctx);
        self.poll_search(ctx);
        self.poll_search_suggestions(ctx);
        self.poll_channel(ctx);
        self.poll_channel_more_videos();
        self.poll_channel_sorted_videos();
        self.poll_channel_tab_feeds();
        self.poll_channel_search();
        self.poll_channel_playlists(ctx);
        self.poll_channel_playlist_videos();
        self.poll_channel_about();
        self.poll_sponsorblock();
        self.poll_local_followed_feed(ctx);
        self.poll_downloads(ctx);
        self.poll_remote_playlist(ctx);
        self.poll_community_extras();
        self.poll_ytdlp_updater();
        self.poll_app_updater(ctx);
        self.poll_cache_picker(ctx);
        self.poll_download_dir_picker(ctx);
        self.poll_backup_picker(ctx);
        self.poll_oauth_picker(ctx);
    }

    pub(crate) fn poll_reauth(&mut self, ctx: &egui::Context) {
        let reauth_result = self.auth.reauth_rx.as_ref().map(Receiver::try_recv);
        let mut reauth_finished = None;
        match reauth_result {
            Some(Ok(result)) => reauth_finished = Some(result),
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auth.reauth_rx = None;
                self.auth.reauth_cancel = None;
                self.message = "Autoriza\u{00e7}\u{00e3}o interrompida.".into();
            }
        }
        if let Some(result) = reauth_finished {
            self.auth.reauth_rx = None;
            self.auth.reauth_cancel = None;
            match result {
                Ok(session) => {
                    self.auth.authenticated = true;
                    let persistence_error = self.set_oauth_session(Some(session));
                    self.auth.my_channel_fetch_attempted = false;
                    if self.auth.my_channel_avatar_url.is_none() {
                        self.start_fetch_my_channel(false, ctx);
                    }
                    self.start_subscription_channels_fetch(ctx);
                    self.watch.extras.reset_comments();
                    self.message = persistence_error
                        .map(|error| format!("Autorização concluída, mas não foi possível salvar a sessão: {error}"))
                        .unwrap_or_else(|| "Comentários autorizados com sucesso.".into());
                    if self.watch.extras.tab == watch_extras::Tab::Comments
                        && self.watch.extras.video_url.is_some()
                    {
                        self.watch
                            .extras
                            .start_comments(self.auth.session.as_ref(), false, ctx);
                    }
                }
                Err(error) => {
                    self.message = error.clone();
                    if self.watch.extras.tab == watch_extras::Tab::Comments {
                        self.watch.extras.comments_error = Some(error);
                    }
                }
            }
        }
    }

    pub(crate) fn poll_rating(&mut self) {
        let rating_result = self.auth.rating_rx.as_ref().map(Receiver::try_recv);
        match rating_result {
            Some(Ok((updated_session, result))) => {
                if let Some(session) = updated_session
                    && self.auth.session.is_some()
                {
                    let _ = self.set_oauth_session(Some(session));
                }
                match result {
                    Ok((video_id, rating)) => {
                        if rating == Some(oauth::VideoRating::Like) {
                            if self.library.liked_videos.rx.is_none() {
                                self.library.liked_videos.loaded = false;
                            }
                        } else {
                            let before_len = self.library.liked_videos.items.len();
                            self.library.liked_videos.items.retain(|item| {
                                history::youtube_video_id(&item.video.url).as_deref()
                                    != Some(video_id.as_str())
                            });
                            let removed = before_len - self.library.liked_videos.items.len();
                            if removed > 0 {
                                self.library.liked_videos.seen_keys.remove(&video_id);
                                if let Some(total) = &mut self.library.liked_videos.total_results {
                                    *total = total.saturating_sub(removed as u64);
                                }
                                self.library.liked_videos.revision =
                                    self.library.liked_videos.revision.wrapping_add(1);
                            }
                        }
                        if let Some(rating) = rating {
                            self.auth.ratings.insert(video_id, rating);
                            self.message = match rating {
                                oauth::VideoRating::Like => "Você gostou deste vídeo.".into(),
                                oauth::VideoRating::Dislike => {
                                    "Você não gostou deste vídeo.".into()
                                }
                                oauth::VideoRating::None => "Avaliação removida.".into(),
                            };
                        } else {
                            self.auth.ratings.remove(&video_id);
                            self.message = "Avaliação removida.".into();
                        }
                    }
                    Err(error) => {
                        self.message = error;
                    }
                }
                self.auth.rating_rx = None;
                self.auth.rating_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.message = "A avaliação foi interrompida.".into();
                self.auth.rating_rx = None;
                self.auth.rating_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn poll_subscription_channels(&mut self) {
        let outcome = self.feed.home.channels_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok((updated_session, result))) => {
                self.feed.home.channels_rx = None;
                self.feed.home.channels_cancel = None;
                if let Some(session) = updated_session
                    && self.auth.session.is_some()
                {
                    let _ = self.set_oauth_session(Some(session));
                }
                match result {
                    Ok(channels) => {
                        self.feed.home.subscription_channels = channels;
                        self.feed.home.subscription_channels_loaded = true;
                        self.feed.home.subscription_channels_error = None;
                        self.populate_channel_avatars_from_subscriptions();
                    }
                    Err(error) => {
                        self.feed.home.subscription_channels_error = Some(error);
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.feed.home.channels_rx = None;
                self.feed.home.channels_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn poll_home_batch(&mut self, ctx: &egui::Context) {
        let outcome = self.feed.home.batch_rx.as_ref().map(Receiver::try_recv);
        match outcome {
            Some(Ok(msg)) => {
                if self.feed.home.batch_active_request_id != Some(msg.request_id) {
                    return;
                }
                self.feed.home.batch_rx = None;
                self.feed.home.batch_cancel = None;
                self.feed.home.batch_active_request_id = None;
                match msg.result {
                    Ok(feed) => {
                        let fetched_count = feed.items.len();
                        let mut existing_keys: std::collections::HashSet<String> = self
                            .feed
                            .home
                            .cached_items
                            .iter()
                            .map(|item| {
                                history::youtube_video_id(&item.video.url)
                                    .unwrap_or_else(|| item.video.url.clone())
                            })
                            .collect();
                        let mut existing_playlists: std::collections::HashSet<String> =
                            self.feed.home.cached_playlists.iter().cloned().collect();
                        for playlist in feed.playlists {
                            if existing_playlists.insert(playlist.clone()) {
                                self.feed.home.cached_playlists.push(playlist);
                            }
                        }
                        let mut added_count = 0usize;
                        for item in feed.items {
                            let key = history::youtube_video_id(&item.video.url)
                                .unwrap_or_else(|| item.video.url.clone());
                            if existing_keys.insert(key) {
                                self.feed.home.cached_items.push(item);
                                added_count += 1;
                            }
                        }
                        self.feed.home.cached_loaded = true;
                        self.feed.home.page = msg.page.saturating_add(1);
                        self.feed.home.has_more = fetched_count > 0 && added_count > 0;
                        let should_apply = (self.ui.browse_section == BrowseSection::Home
                            || self.ui.browse_section == BrowseSection::Shorts)
                            && !self.ui.search_results_mode
                            && !self.channels.view_open
                            && self.feed.home.title.is_none();
                        if should_apply {
                            self.feed.home.playlist_urls = self.feed.home.cached_playlists.clone();
                            let cached = self.feed.home.cached_items.clone();
                            self.apply_search_results(ctx, cached);
                        }
                    }
                    Err(error) => {
                        self.feed.home.has_more = false;
                        if self.feed.videos.is_empty()
                            && self.ui.browse_section == BrowseSection::Home
                            && !self.ui.search_results_mode
                        {
                            self.message = error;
                        }
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.feed.home.batch_rx = None;
                self.feed.home.batch_cancel = None;
                self.feed.home.batch_active_request_id = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn poll_home(&mut self, ctx: &egui::Context) {
        let mut home_messages = Vec::new();
        let mut home_disconnected = false;
        if let Some(receiver) = &self.feed.home.rx {
            loop {
                match receiver.try_recv() {
                    Ok(message) => home_messages.push(message),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        home_disconnected = true;
                        break;
                    }
                }
            }
        }
        for home_message in home_messages {
            match home_message {
                HomeFetchMessage::Progress(progress) => {
                    if self.feed.home.title.as_deref()
                        == Some("Inscri\u{00e7}\u{00f5}es: envios recentes")
                    {
                        self.feed.home.subscription_channels = progress.channels.clone();
                        self.feed.home.subscription_channels_loaded = true;
                        self.feed.home.subscription_channels_error = None;
                        self.populate_channel_avatars_from_subscriptions();
                    }
                    self.feed.home.limit_reached |= progress.reached_limit;
                    self.feed.home.pending_items = progress.items;
                    self.message = if self.feed.home.limit_reached {
                        format!(
                            "Carregados {} canais; limite de 100 canais atingido. Algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.",
                            progress.completed_channels
                        )
                    } else {
                        format!(
                            "Carregando envios: {} de {} canais.",
                            progress.completed_channels, progress.total_channels
                        )
                    };
                }
                HomeFetchMessage::Finished(token, Ok(feed)) => {
                    self.feed.home.pending_items.clear();
                    let was_authenticated = self.auth.authenticated;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    let is_subscription_feed = self.feed.home.title.as_deref()
                        == Some("Inscri\u{00e7}\u{00f5}es: envios recentes");
                    let is_playlists_feed =
                        self.feed.home.title.as_deref() == Some("Playlists da conta Google");
                    self.feed.home.rx = None;
                    self.feed.home.cancel = None;
                    if is_subscription_feed {
                        self.feed.home.subscription_channels = feed.channels.clone();
                        self.feed.home.subscription_channels_loaded = true;
                        self.feed.home.subscription_channels_error = None;
                        self.populate_channel_avatars_from_subscriptions();
                        self.feed.home.subscriptions_cached_items = feed.items.clone();
                        self.feed.home.subscriptions_cached_loaded = true;
                        self.feed.home.subscriptions_cached_at = Some(Instant::now());
                        self.persist_subscriptions_disk_cache();
                    }
                    if !was_authenticated && self.auth.authenticated {
                        if self.auth.my_channel_avatar_url.is_none()
                            && !self.auth.my_channel_fetch_attempted
                        {
                            self.start_fetch_my_channel(false, ctx);
                        }
                        if !self.feed.home.subscription_channels_loaded {
                            self.start_subscription_channels_fetch(ctx);
                        }
                    }
                    if is_playlists_feed {
                        self.auth.remote_playlists = feed
                            .items
                            .iter()
                            .filter_map(|item| {
                                item.video
                                    .url
                                    .strip_prefix("fasttube-google-playlist:")
                                    .map(|id| (id.to_owned(), item.video.title.clone()))
                            })
                            .collect();
                    }
                    self.feed.home.playlist_urls = feed.playlists;
                    self.apply_search_results(ctx, feed.items);
                    if self.feed.home.limit_reached {
                        self.message = "Limite de 100 canais por atualiza\u{e7}\u{e3}o atingido; algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.".into();
                    } else if self.feed.videos.is_empty() {
                        self.message = if self.feed.home.title.as_deref()
                            == Some("Playlists da conta Google")
                        {
                            "Nenhuma playlist encontrada nesta conta.".into()
                        } else if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
                            "Nenhum vídeo encontrado nesta playlist.".into()
                        } else if self.feed.home.title.as_deref() == Some("Vídeos com Gostei") {
                            "Nenhum vídeo com Gostei encontrado.".into()
                        } else {
                            "Nenhum envio encontrado nas inscri\u{e7}\u{f5}es consultadas.".into()
                        };
                    }
                    if let Some(error) = persistence_error {
                        self.message = format!("{} (sessao local: {error})", self.message);
                    }
                }
                HomeFetchMessage::Finished(token, Err(error)) => {
                    let is_subscription_feed = self.feed.home.title.as_deref()
                        == Some("Inscri\u{00e7}\u{00f5}es: envios recentes");
                    if is_subscription_feed {
                        self.feed.home.subscription_channels_error = Some(error.clone());
                    }
                    if !self.feed.home.pending_items.is_empty() {
                        let partial = std::mem::take(&mut self.feed.home.pending_items);
                        self.apply_search_results(ctx, partial);
                    }
                    self.feed.home.rx = None;
                    self.feed.home.cancel = None;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    self.message = persistence_error
                        .map(|save_error| format!("{error} ({save_error})"))
                        .unwrap_or(error);
                }
                HomeFetchMessage::SubscriptionProgress {
                    request_id,
                    progress,
                } => {
                    if self.feed.home.subscriptions_active_request_id != Some(request_id) {
                        continue;
                    }
                    if !progress.channels.is_empty() {
                        self.feed.home.subscription_channels = progress.channels.clone();
                        self.feed.home.subscription_channels_loaded = true;
                        self.feed.home.subscription_channels_error = None;
                        self.populate_channel_avatars_from_subscriptions();
                    }
                    self.feed.home.limit_reached |= progress.reached_limit;
                    let viewing_subscriptions = self.ui.browse_section
                        == BrowseSection::Subscriptions
                        && !self.ui.search_results_mode
                        && !self.channels.view_open;
                    if self.feed.home.subscriptions_progressive_streaming && viewing_subscriptions {
                        if !progress.items.is_empty() {
                            self.apply_search_results(ctx, progress.items.clone());
                        }
                        self.feed.home.pending_items = progress.items;
                    } else {
                        self.feed.home.pending_items = progress.items;
                    }
                    if viewing_subscriptions {
                        self.message = if self.feed.home.limit_reached {
                            format!(
                                "Carregados {} canais; limite de 100 canais atingido. Algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.",
                                progress.completed_channels
                            )
                        } else if self.feed.home.subscriptions_refreshing_in_background {
                            format!(
                                "Atualizando inscri\u{00e7}\u{f5}es em segundo plano ({} de {} canais)...",
                                progress.completed_channels, progress.total_channels
                            )
                        } else {
                            format!(
                                "Carregando envios: {} de {} canais.",
                                progress.completed_channels, progress.total_channels
                            )
                        };
                    }
                }
                HomeFetchMessage::SubscriptionFinished {
                    request_id,
                    token,
                    result,
                } => {
                    if self.feed.home.subscriptions_active_request_id != Some(request_id) {
                        continue;
                    }
                    self.feed.home.rx = None;
                    self.feed.home.cancel = None;
                    self.feed.home.subscriptions_active_request_id = None;
                    let was_progressive =
                        std::mem::take(&mut self.feed.home.subscriptions_progressive_streaming);
                    self.feed.home.subscriptions_refreshing_in_background = false;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    let viewing_subscriptions = self.ui.browse_section
                        == BrowseSection::Subscriptions
                        && !self.ui.search_results_mode
                        && !self.channels.view_open;
                    match result {
                        Ok(output) => {
                            self.feed.home.pending_items.clear();
                            self.feed.home.subscription_channels = output.feed.channels.clone();
                            self.feed.home.subscription_channels_loaded = true;
                            self.feed.home.subscription_channels_error = None;
                            self.populate_channel_avatars_from_subscriptions();
                            self.feed.home.subscriptions_cached_items = output.feed.items.clone();
                            self.feed.home.subscriptions_cached_loaded = true;
                            self.feed.home.subscriptions_cached_at = Some(Instant::now());
                            self.feed.home.subscriptions_rss_cache = output.rss_cache;
                            self.persist_subscriptions_disk_cache();
                            if viewing_subscriptions {
                                self.feed.home.playlist_urls = output.feed.playlists;
                                self.apply_search_results(ctx, output.feed.items);
                                if self.feed.home.limit_reached {
                                    self.message = "Limite de 100 canais por atualiza\u{e7}\u{e3}o atingido; algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.".into();
                                } else if self.feed.videos.is_empty() {
                                    self.message = "Nenhum envio encontrado nas inscri\u{e7}\u{f5}es consultadas.".into();
                                }
                                if let Some(error) = persistence_error {
                                    self.message =
                                        format!("{} (sessao local: {error})", self.message);
                                }
                            }
                        }
                        Err(error) => {
                            self.feed.home.subscription_channels_error = Some(error.clone());
                            let partial = std::mem::take(&mut self.feed.home.pending_items);
                            if viewing_subscriptions {
                                if !partial.is_empty()
                                    && (was_progressive || self.feed.videos.is_empty())
                                {
                                    self.apply_search_results(ctx, partial);
                                }
                                self.message = persistence_error
                                    .map(|save_error| format!("{error} ({save_error})"))
                                    .unwrap_or(error);
                            }
                        }
                    }
                }
                HomeFetchMessage::PlaylistProgress {
                    batch,
                    total_results,
                } => {
                    if self.feed.home.title.as_deref() != Some("Vídeos da playlist") {
                        continue;
                    }
                    if total_results.is_some() {
                        self.feed.home.playlist_total_results = total_results;
                    }
                    if !batch.is_empty() {
                        for item in batch {
                            self.video_metadata
                                .insert(item.video.url.clone(), item.metadata);
                            self.feed.videos.push(item.video);
                        }
                        self.feed.generation = self.feed.generation.wrapping_add(1);
                        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                        self.feed.cache_dirty = true;
                        self.message.clear();
                    }
                }
                HomeFetchMessage::PlaylistFinished {
                    token,
                    total_results,
                    result,
                } => {
                    self.feed.home.rx = None;
                    self.feed.home.cancel = None;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    if total_results.is_some() {
                        self.feed.home.playlist_total_results = total_results;
                    }
                    match result {
                        Ok(()) => {
                            if self.feed.videos.is_empty() {
                                self.message = "Nenhum vídeo encontrado nesta playlist.".into();
                            } else {
                                self.message.clear();
                            }
                            if let Some(error) = persistence_error {
                                self.message = format!("{} (sessao local: {error})", self.message);
                            }
                        }
                        Err(error) => {
                            self.message = persistence_error
                                .map(|save_error| format!("{error} ({save_error})"))
                                .unwrap_or(error);
                        }
                    }
                }
            }
        }
        if home_disconnected && self.feed.home.rx.is_some() {
            self.feed.home.rx = None;
            self.feed.home.cancel = None;
            self.feed.home.subscriptions_active_request_id = None;
            self.feed.home.subscriptions_progressive_streaming = false;
            self.feed.home.subscriptions_refreshing_in_background = false;
            if !self.feed.home.pending_items.is_empty() {
                let partial = std::mem::take(&mut self.feed.home.pending_items);
                self.apply_search_results(ctx, partial);
            }
            self.message = "A consulta foi interrompida.".into();
        }
    }

    pub(crate) fn poll_quality(&mut self) {
        if let Some(receiver) = &self.playback.quality_rx {
            match receiver.try_recv() {
                Ok(Ok(meta)) => {
                    self.playback.qualities = meta.heights;
                    self.playback.storyboard = meta.storyboard.clone();
                    self.playback.storyboard_video_url =
                        self.selected.as_ref().map(|v| v.url.clone());
                    self.watch.extras.apply_preloaded_subtitles(meta.subtitles);
                    self.watch.overlay_cache.mpv_subs_last_poll = None;
                    self.watch.overlay_cache.force_refresh = true;
                    self.playback.quality_rx = None;
                    if let Some(selected_url) = self.selected.as_ref().map(|v| v.url.clone()) {
                        if meta.is_live
                            || meta.title.is_some()
                            || meta.channel.is_some()
                            || meta.channel_id.is_some()
                            || meta.channel_url.is_some()
                            || meta.description.is_some()
                            || meta.concurrent_view_count.is_some()
                        {
                            self.apply_live_metadata_to_video(
                                &selected_url,
                                meta.is_live,
                                None,
                                meta.title.as_deref(),
                                meta.channel.as_deref(),
                                meta.channel_id.as_deref(),
                                meta.channel_url.as_deref(),
                                None,
                                None,
                                meta.description.as_deref(),
                                meta.concurrent_view_count,
                                None,
                            );
                        }
                        self.mark_followed_video_seen(meta.channel_id.as_deref(), &selected_url);
                    }
                }
                Ok(Err(error)) => {
                    self.playback.quality_error = Some(error);
                    self.playback.quality_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.playback.quality_error =
                        Some("A consulta de resoluções foi interrompida.".into());
                    self.playback.quality_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    pub(crate) fn poll_search(&mut self, ctx: &egui::Context) {
        loop {
            let outcome = self
                .feed
                .search
                .rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ = self.complete_search_request(msg.request_id, msg.result, ctx);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.feed.search.active_request_id.is_some() {
                        self.message = "A pesquisa foi interrompida.".into();
                    }
                    self.feed.search.active_request_id = None;
                    self.feed.search.rx = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    pub(crate) fn poll_channel(&mut self, ctx: &egui::Context) {
        loop {
            let outcome = self
                .channels
                .videos
                .rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ =
                        self.complete_channel_outcome(msg.request_id, &msg.target, msg.result, ctx);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.channels.videos.active_request_id.is_some() {
                        self.channels.error = Some("A consulta do canal foi interrompida.".into());
                    }
                    self.channels.videos.active_request_id = None;
                    self.channels.loading_target = None;
                    self.channels.videos.rx = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    pub(crate) fn poll_channel_more_videos(&mut self) {
        loop {
            let outcome = self
                .channels
                .videos
                .more_rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ =
                        self.complete_channel_more_videos(msg.request_id, &msg.target, msg.result);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.channels.videos.active_more_request_id.is_some() {
                        self.channels.videos.more_error =
                            Some("O carregamento de mais vídeos do canal foi interrompido.".into());
                    }
                    self.channels.videos.active_more_request_id = None;
                    self.channels.videos.more_rx = None;
                    self.channels.videos.more_cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    pub(crate) fn poll_channel_playlists(&mut self, ctx: &egui::Context) {
        loop {
            let outcome = self
                .channels
                .playlists
                .rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ = self.complete_channel_playlists(
                        msg.request_id,
                        &msg.target,
                        msg.result,
                        ctx,
                    );
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.channels.playlists.active_request_id.is_some() {
                        self.channels.playlists.error =
                            Some("A consulta de playlists públicas foi interrompida.".into());
                    }
                    self.channels.playlists.active_request_id = None;
                    self.channels.playlists.rx = None;
                    self.channels.playlists.cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    pub(crate) fn poll_channel_playlist_videos(&mut self) {
        loop {
            let outcome = self
                .channels
                .playlist_detail
                .rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ = self.complete_channel_playlist_videos(
                        msg.request_id,
                        &msg.target,
                        &msg.playlist_id,
                        msg.result,
                    );
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.channels.playlist_detail.active_request_id.is_some() {
                        self.channels.playlist_detail.error =
                            Some("A consulta dos vídeos da playlist foi interrompida.".into());
                    }
                    self.channels.playlist_detail.active_request_id = None;
                    self.channels.playlist_detail.rx = None;
                    self.channels.playlist_detail.cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    pub(crate) fn poll_channel_about(&mut self) {
        loop {
            let outcome = self
                .channels
                .about
                .rx
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ = self.complete_channel_about(msg.request_id, &msg.target, msg.result);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.channels.about.active_request_id.is_some() {
                        self.channels.about.error =
                            Some("A consulta de informações da aba Sobre foi interrompida.".into());
                    }
                    self.channels.about.active_request_id = None;
                    self.channels.about.rx = None;
                    self.channels.about.cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }
    pub(crate) fn start_sponsorblock_fetch(&mut self, video_url: &str, ctx: &egui::Context) {
        let video_id = history::youtube_video_id(video_url);
        self.watch.sponsorblock.reset_for_video(video_id.clone());
        if !self.settings.prefs.value.sponsorblock.enabled {
            return;
        }
        let Some(video_id) = video_id else {
            return;
        };
        let categories = self
            .settings
            .prefs
            .value
            .sponsorblock
            .categories
            .enabled_categories();
        if categories.is_empty() {
            self.watch.sponsorblock.loaded = true;
            return;
        }
        self.watch.sponsorblock.next_request_id = self
            .watch
            .sponsorblock
            .next_request_id
            .wrapping_add(1)
            .max(1);
        let request_id = self.watch.sponsorblock.next_request_id;
        self.watch.sponsorblock.active_request_id = Some(request_id);
        let cancel = process::Cancellation::default();
        self.watch.sponsorblock.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.watch.sponsorblock.rx = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = sponsorblock::fetch_segments_with_cancel(&video_id, &categories, &cancel);
            let _ = sender.send(sponsorblock::SponsorBlockFetchMessage {
                request_id,
                video_id,
                result,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn poll_sponsorblock(&mut self) {
        let mut disconnect = false;
        let mut received = None;
        if let Some(receiver) = &self.watch.sponsorblock.rx {
            match receiver.try_recv() {
                Ok(msg) => received = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => disconnect = true,
            }
        }
        if let Some(msg) = received {
            let matches_active = self.watch.sponsorblock.active_request_id == Some(msg.request_id)
                && self.watch.sponsorblock.video_id.as_deref() == Some(msg.video_id.as_str());
            self.watch.sponsorblock.rx = None;
            self.watch.sponsorblock.cancel = None;
            self.watch.sponsorblock.active_request_id = None;
            if matches_active {
                match msg.result {
                    Ok(segments) => {
                        self.watch.sponsorblock.segments = segments;
                        self.watch.sponsorblock.error = None;
                    }
                    Err(error) => {
                        self.watch.sponsorblock.segments.clear();
                        self.watch.sponsorblock.error = Some(error);
                    }
                }
                self.watch.sponsorblock.loaded = true;
            }
        } else if disconnect {
            self.watch.sponsorblock.rx = None;
            self.watch.sponsorblock.cancel = None;
            self.watch.sponsorblock.active_request_id = None;
        }
    }

    pub(crate) fn try_auto_skip_sponsor_segment(
        &mut self,
        pos_secs: f64,
        duration_secs: Option<f64>,
        is_live: bool,
        now: Instant,
    ) -> Option<f64> {
        if is_live
            || self.playback.seek_drag.is_some()
            || !self.settings.prefs.value.sponsorblock.enabled
            || self.settings.prefs.value.sponsorblock.skip_mode
                != sponsorblock::SponsorSkipMode::AutoSkip
        {
            return None;
        }
        let seg = self
            .watch
            .sponsorblock
            .segment_at(pos_secs, &self.settings.prefs.value.sponsorblock)?
            .clone();
        if self
            .watch
            .sponsorblock
            .auto_skipped_uuids
            .contains(&seg.uuid)
        {
            return None;
        }
        self.watch.sponsorblock.auto_skipped_uuids.insert(seg.uuid);
        if self.settings.prefs.value.sponsorblock.show_skip_toast {
            self.watch.sponsorblock.last_skip_notice =
                Some((format!("Pulado: {}", seg.category.label_pt()), now));
        }
        let target = duration_secs
            .map_or(seg.end_secs, |dur| seg.end_secs.min((dur - 0.1).max(0.0)))
            .max(pos_secs);
        Some(target)
    }

    pub(crate) fn record_manual_sponsor_skip(
        &mut self,
        pos_secs: f64,
        target_secs: f64,
        now: Instant,
    ) {
        let matched = self
            .watch
            .sponsorblock
            .segment_at(pos_secs, &self.settings.prefs.value.sponsorblock)
            .cloned()
            .or_else(|| {
                self.watch
                    .sponsorblock
                    .filtered_segments(&self.settings.prefs.value.sponsorblock)
                    .find(|seg| (seg.end_secs - target_secs).abs() < 0.25)
                    .cloned()
            });
        if let Some(seg) = matched {
            self.watch.sponsorblock.auto_skipped_uuids.insert(seg.uuid);
            if self.settings.prefs.value.sponsorblock.show_skip_toast {
                self.watch.sponsorblock.last_skip_notice =
                    Some((format!("Pulado: {}", seg.category.label_pt()), now));
            }
        }
    }

    pub(crate) fn open_video(
        &mut self,
        video: Video,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) -> bool {
        let keep_docked = std::mem::take(&mut self.playback.pending_open_keep_mini_player);
        let suppress_back_stack =
            std::mem::take(&mut self.playback.pending_open_suppress_back_stack);
        self.cancel_pending_queue_video();
        if !keep_docked {
            self.playback.in_app_mini_player = false;
            self.ui.settings_open = false;
            self.close_channel_view();
        }
        self.playback.autoplay_eof_handled = false;
        self.playback.auto_subtitle_attempted = false;
        self.playback.save_menu_open = false;
        self.playback.pending_open_video = None;
        let explicit_seek_ms = self.playback.pending_open_seek_ms.take();
        #[cfg(windows)]
        {
            let now = Instant::now();
            let previous_player_pos_ms = self.current_player_live_pos_ms();
            self.sample_player_before_transition(now);
            self.flush_playback_history_at(now);
            self.ensure_playback_settings_synchronized();
            let bookmark_seek_ms =
                self.library
                    .pending_bookmark_seek
                    .take()
                    .and_then(|(target_id, time_ms)| {
                        (history::youtube_video_id(&video.url).as_deref()
                            == Some(target_id.as_str()))
                        .then_some(time_ms)
                    });
            let resume_target_ms = explicit_seek_ms
                .or(bookmark_seek_ms)
                .or_else(|| self.planned_resume_ms(&video));

            let mut player_error: Option<String> = None;
            let player_created = self.player.is_none();
            if player_created {
                let profile = self.settings.buffer.profile;
                let hwdec_mode = self.settings.prefs.value.hwdec_mode;
                match player::Player::new(frame, profile.values(), hwdec_mode, ctx) {
                    Ok(player) => {
                        self.player = Some(player);
                        self.playback.active_buffer = Some(profile);
                        self.playback.active_hwdec = Some(hwdec_mode);
                    }
                    Err(error) => {
                        player_error = Some(error);
                    }
                }
            }
            self.playback.autoplay = self.settings.prefs.value.default_autoplay;
            self.playback.loop_video = self.settings.prefs.value.default_loop;
            self.playback.clear_ab_loop();
            let initial_speed = f64::from(self.settings.playback.value.speed_quarters) / 4.0;
            self.playback.speed = initial_speed;
            let is_live_initial = self.is_video_known_live(&video);
            if let Some(player) = &mut self.player {
                let _ = player.set(
                    "options/ytdl-format",
                    &self.settings.playback.value.format(),
                );
                let _ = player.set("speed", &format!("{initial_speed}"));
                let _ = player.set_audio_only(self.playback.audio_only);
                if player_created || !self.settings.prefs.value.remember_volume {
                    let volume = self.settings.prefs.value.default_volume_percent.min(100);
                    let _ = player.set("volume", &format!("{volume}"));
                    self.playback.mini_player_volume = f32::from(volume);
                } else if let Some(vol) = player.double_cstr(c"volume") {
                    self.playback.mini_player_volume = vol.clamp(0.0, 100.0) as f32;
                }
                let _ = player.apply_audio_filters(
                    self.settings.prefs.value.audio_normalization,
                    self.playback.audio_eq,
                );
                let _ = player.set_video_eq(
                    self.playback.brightness,
                    self.playback.contrast,
                    self.playback.saturation,
                );
                let _ = player.set("options/start", "none");
                let _ = player.set(
                    "loop-file",
                    if self.playback.loop_video && !is_live_initial {
                        "inf"
                    } else {
                        "no"
                    },
                );
                let _ = player.set("pause", "no");
                if let Err(error) = player.play(&video.url) {
                    let _ = player.set("options/start", "none");
                    player_error = Some(error);
                }
            }
            if let Some(error) = &player_error {
                self.message = error.clone();
            } else {
                self.message.clear();
            }
            self.watch.share_copied_at = None;
            self.playback.quality = None;
            self.playback.qualities.clear();
            self.playback.quality_error = None;
            self.playback.quality_fallback_url = None;
            self.playback.current_file_loaded = false;
            self.playback.cached_raw_path = None;
            self.playback.cached_raw_path_poll = None;
            self.playback.storyboard = None;
            self.playback.storyboard_video_url = None;
            self.playback.audio_tracks.clear();
            self.playback.audio_tracks_video_url = None;
            self.playback.audio_tracks_last_poll = None;
            self.thumbnails.clear_storyboards(ctx);
            self.playback.seek_drag = None;
            self.playback.player_menu = None;
            self.playback.player_context_menu = None;
            self.playback.last_subtitle_id = None;
            self.reset_decoding_diagnostics();
            self.watch.extras.sync_video(Some(&video.url));
            self.start_watch_live_for_selected(&video, ctx);
            self.start_sponsorblock_fetch(&video.url, ctx);
            self.start_community_extras_fetch(&video.url, ctx);
            if let Some(cancel) = self.playback.quality_cancel.take() {
                cancel.cancel();
            }
            self.playback.quality_rx = None;
            if let Some(cached) = search::cached_playback_metadata(&video.url) {
                self.playback.qualities = cached.heights;
                self.playback.storyboard = cached.storyboard;
                self.playback.storyboard_video_url = Some(video.url.clone());
                self.watch
                    .extras
                    .apply_preloaded_subtitles(cached.subtitles);
            } else if history::youtube_video_id(&video.url).is_none() {
                self.playback.quality_fallback_url = Some(video.url.clone());
            }
            self.selected_metadata = self.metadata_for(&video.url).cloned();
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            self.watch.metadata_cache = app_state::WatchMetadataUiCache::default();
            self.watch.overlay_cache = app_state::OverlayCache::default();
            self.prune_session_caches();
            self.playback.last_video_size_url = None;
            self.playback.last_video_size = None;
            let sub_target = self.channel_target_for_video(&video);
            let sub_channel_id = self
                .selected_metadata
                .as_ref()
                .and_then(|meta| meta.channel_id.clone());
            self.mark_followed_video_seen(sub_channel_id.as_deref(), &video.url);
            self.activate_video_playback(&video, resume_target_ms, previous_player_pos_ms, now);
            if explicit_seek_ms.is_some()
                && let Some(active) = &mut self.playback.active
                && !active.is_live
            {
                active.explicit_seek_pending = true;
            }
            if !suppress_back_stack && let Some(prev) = self.selected.clone() {
                self.record_video_back_stack(prev, &video.url);
            }
            self.playback.mini_player_paused = false;
            self.selected = Some(video);
            if self.auth.authenticated
                && let Some(target) = sub_target
            {
                self.start_check_subscription(target, sub_channel_id, ctx);
            }
            if !keep_docked {
                self.ui.navigation_open = false;
            }
            self.invalidate_feed_cache();
            self.sync_player_window_visibility();
            player_error.is_none()
        }
        #[cfg(not(windows))]
        {
            let _ = (video, frame, ctx, suppress_back_stack);
            self.message = "A reprodução está disponível somente no Windows.".into();
            false
        }
    }

    pub(crate) fn close_video(&mut self, ctx: &egui::Context) {
        self.cancel_pending_queue_video();
        self.playback.autoplay_eof_handled = false;
        self.playback.auto_subtitle_attempted = false;
        self.playback.save_menu_open = false;
        self.playback.pending_open_video = None;
        self.playback.pending_open_seek_ms = None;
        self.watch.extras.sync_video(None);
        self.watch.live.reset_for_video(None, None);
        self.watch.sponsorblock.reset_for_video(None);
        if let Some(cancel) = self.watch.community_cancel.take() {
            cancel.cancel();
        }
        self.watch.community_rx = None;
        self.watch.community_active_id = None;
        self.watch.show_original_title = false;
        if let Some(cancel) = self.playback.quality_cancel.take() {
            cancel.cancel();
        }
        let now = Instant::now();
        #[cfg(windows)]
        {
            self.remember_current_player_volume();
            self.sample_player_before_transition(now);
        }
        self.flush_playback_history_at(now);
        self.playback.active = None;
        self.playback.in_app_mini_player = false;
        self.playback.pending_open_keep_mini_player = false;
        self.playback.pending_open_suppress_back_stack = false;
        self.playback.mini_player_paused = false;
        self.playback.video_back_stack.clear();
        self.playback.video_forward_stack.clear();
        self.selected = None;
        self.watch.share_copied_at = None;
        self.feed.share_modal_video = None;
        self.ui.navigation_open = false;
        self.invalidate_feed_cache();
        self.selected_metadata = None;
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.watch.metadata_cache = app_state::WatchMetadataUiCache::default();
        self.watch.sidebar_cache = app_state::WatchSidebarCache::default();
        self.watch.overlay_cache = app_state::OverlayCache::default();
        self.playback.last_video_size_url = None;
        self.playback.last_video_size = None;
        self.playback.seek_drag = None;
        self.playback.player_menu = None;
        self.playback.player_context_menu = None;
        self.playback.stats_for_nerds_open = false;
        self.playback.stats_for_nerds = app_state::PlayerNerdsStats::default();
        self.playback.stats_for_nerds_last_poll = None;
        self.playback.overlay_last_pointer_pos = None;
        self.playback.last_subtitle_id = None;
        self.playback.quality = None;
        self.playback.qualities.clear();
        self.playback.quality_rx = None;
        self.playback.quality_error = None;
        self.playback.quality_fallback_url = None;
        self.playback.current_file_loaded = false;
        self.playback.cached_raw_path = None;
        self.playback.cached_raw_path_poll = None;
        self.playback.storyboard = None;
        self.playback.storyboard_video_url = None;
        self.playback.audio_tracks.clear();
        self.playback.audio_tracks_video_url = None;
        self.playback.audio_tracks_last_poll = None;
        self.thumbnails.clear_storyboards(ctx);
        self.prune_session_caches();
        self.playback.active_buffer = None;
        self.playback.active_hwdec = None;
        self.playback.clear_ab_loop();
        self.playback.audio_only = false;
        self.playback.speed = 1.0;
        self.reset_decoding_diagnostics();
        #[cfg(windows)]
        {
            if let Some(player) = &mut self.player {
                player.set_visible(false);
            }
            self.player = None;
        }
        if self.playback.mini_player {
            self.toggle_mini_player(ctx);
        }
        if self.playback.fullscreen {
            self.toggle_fullscreen(ctx);
        }
        self.sync_player_window_visibility();
    }

    pub(crate) fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        if !self.playback.fullscreen {
            let current_maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            let mut restore_maximized = current_maximized;
            if self.playback.mini_player {
                restore_maximized |=
                    std::mem::take(&mut self.playback.mini_player_restore_maximized);
                self.toggle_mini_player(ctx);
            }
            self.playback.in_app_mini_player = false;
            self.playback.fullscreen = true;
            self.playback.fullscreen_restore_maximized = restore_maximized;
            self.playback.fullscreen_sync_retries = 3;
            self.playback.player_context_menu = None;
            self.playback.overlay_last_activity = Instant::now();
            self.playback.overlay_last_pointer_pos = ctx.input(|i| i.pointer.hover_pos());
            if current_maximized {
                // Clear WS_MAXIMIZE before entering fullscreen; winit's WM_NCCALCSIZE clamps maximized windows to rcWork.
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            // Force egui-winit to clear MARKER_UNDECORATED_SHADOW while fullscreen to avoid a 1px WM_NCCALCSIZE offset.
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        } else {
            let restore_maximized = std::mem::take(&mut self.playback.fullscreen_restore_maximized);
            self.playback.fullscreen = false;
            self.playback.fullscreen_sync_retries = 0;
            self.playback.player_context_menu = None;
            self.playback.overlay_last_activity = Instant::now();
            self.playback.overlay_last_pointer_pos = ctx.input(|i| i.pointer.hover_pos());
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            // Restore undecorated drop shadow after leaving fullscreen.
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
            if restore_maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
        self.sync_player_window_visibility();
    }

    pub(crate) fn sync_fullscreen_viewport(&mut self, ctx: &egui::Context) {
        if !self.playback.fullscreen {
            return;
        }
        let (is_maximized, is_fullscreen) = ctx.input(|input| {
            let vp = input.viewport();
            (vp.maximized == Some(true), vp.fullscreen == Some(true))
        });
        if is_maximized {
            if self.playback.fullscreen_sync_retries > 0 {
                self.playback.fullscreen_sync_retries -= 1;
                self.playback.fullscreen_restore_maximized = true;
                if is_fullscreen {
                    // winit ignores set_fullscreen(Borderless(None)) when already fullscreen; reset first to reapply bounds.
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                ctx.request_repaint();
            }
        } else if is_fullscreen {
            self.playback.fullscreen_sync_retries = 3;
        }
    }

    pub(crate) fn toggle_theater_mode(&mut self) {
        self.playback.theater = !self.playback.theater;
        self.sync_player_window_visibility();
    }

    pub(crate) fn viewport_resize_direction(
        rect: egui::Rect,
        pos: egui::Pos2,
        edge_thickness: f32,
        corner_thickness: f32,
    ) -> Option<egui::viewport::ResizeDirection> {
        use egui::viewport::ResizeDirection;
        if !rect.contains(pos)
            || rect.width() <= edge_thickness * 2.0
            || rect.height() <= edge_thickness * 2.0
        {
            return None;
        }
        let left = (pos.x - rect.left()) <= edge_thickness;
        let right = (rect.right() - pos.x) <= edge_thickness;
        let top = (pos.y - rect.top()) <= edge_thickness;
        let bottom = (rect.bottom() - pos.y) <= edge_thickness;
        if !(left || right || top || bottom) {
            return None;
        }
        let near_left = (pos.x - rect.left()) <= corner_thickness;
        let near_right = (rect.right() - pos.x) <= corner_thickness;
        let near_top = (pos.y - rect.top()) <= corner_thickness;
        let near_bottom = (rect.bottom() - pos.y) <= corner_thickness;
        Some(if (top && near_left) || (left && near_top) {
            ResizeDirection::NorthWest
        } else if (top && near_right) || (right && near_top) {
            ResizeDirection::NorthEast
        } else if (bottom && near_left) || (left && near_bottom) {
            ResizeDirection::SouthWest
        } else if (bottom && near_right) || (right && near_bottom) {
            ResizeDirection::SouthEast
        } else if top {
            ResizeDirection::North
        } else if bottom {
            ResizeDirection::South
        } else if left {
            ResizeDirection::West
        } else {
            ResizeDirection::East
        })
    }

    pub(crate) fn handle_frameless_window_resize(&self, ctx: &egui::Context) -> bool {
        use egui::viewport::ResizeDirection;
        let (is_maximized, is_fullscreen, content_rect, hover_pos, primary_pressed) =
            ctx.input(|input| {
                let vp = input.viewport();
                (
                    vp.maximized.unwrap_or(false),
                    self.playback.fullscreen || vp.fullscreen.unwrap_or(false),
                    input.content_rect(),
                    input.pointer.hover_pos(),
                    input.pointer.primary_pressed(),
                )
            });
        if is_maximized || is_fullscreen || ctx.egui_is_using_pointer() {
            return false;
        }
        let Some(pos) = hover_pos else {
            return false;
        };
        let Some(direction) = Self::viewport_resize_direction(content_rect, pos, 4.0, 10.0) else {
            return false;
        };
        let cursor = match direction {
            ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
            ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
        };
        ctx.set_cursor_icon(cursor);
        if primary_pressed {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
        true
    }

    pub(crate) fn preprocess_raw_input(&mut self, raw_input: &mut egui::RawInput) {
        let chromeless =
            self.playback.fullscreen || (self.playback.mini_player && self.is_watch_page_active());
        let allow_activate = !chromeless;
        let header_guard_y = if chromeless { 0.0 } else { 56.0 };
        self.autoscroll
            .preprocess_raw_input(raw_input, allow_activate, header_guard_y);
        if self.autoscroll.is_active() {
            self.playback.player_context_menu = None;
        }
    }

    pub(crate) fn apply_middle_autoscroll(&mut self, ctx: &egui::Context) {
        if self.playback.fullscreen || self.playback.mini_player {
            self.autoscroll.cancel();
            return;
        }
        if let Some((vel, _)) = self.autoscroll.velocity_and_cursor()
            && vel != egui::Vec2::ZERO
        {
            let dt = ctx.input(|i| i.stable_dt.clamp(0.0, 0.05));
            ctx.input_mut(|i| i.smooth_scroll_delta += vel * dt);
            ctx.request_repaint();
        }
    }

    pub(crate) fn paint_middle_autoscroll_overlay(&self, ctx: &egui::Context) {
        let Some(anchor) = self.autoscroll.anchor() else {
            return;
        };
        let Some((_, cursor)) = self.autoscroll.velocity_and_cursor() else {
            return;
        };
        let dark = self.settings.theme.value == Theme::Dark;
        let bg = if dark {
            Color32::from_rgba_unmultiplied(32, 32, 36, 232)
        } else {
            Color32::from_rgba_unmultiplied(248, 248, 250, 240)
        };
        let border = if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 110)
        } else {
            Color32::from_rgba_unmultiplied(40, 40, 44, 140)
        };
        let fg = if dark {
            Color32::from_rgb(240, 240, 244)
        } else {
            Color32::from_rgb(36, 36, 40)
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("fasttube_autoscroll_anchor"),
        ));
        painter.circle(anchor, 13.5, bg, egui::Stroke::new(1.2, border));
        painter.circle_filled(anchor, 2.2, fg);
        let arrow_offsets = [
            (
                egui::vec2(0.0, -8.5),
                egui::vec2(-2.6, -5.2),
                egui::vec2(2.6, -5.2),
            ),
            (
                egui::vec2(0.0, 8.5),
                egui::vec2(-2.6, 5.2),
                egui::vec2(2.6, 5.2),
            ),
            (
                egui::vec2(-8.5, 0.0),
                egui::vec2(-5.2, -2.6),
                egui::vec2(-5.2, 2.6),
            ),
            (
                egui::vec2(8.5, 0.0),
                egui::vec2(5.2, -2.6),
                egui::vec2(5.2, 2.6),
            ),
        ];
        for (tip, a, b) in arrow_offsets {
            painter.add(egui::Shape::convex_polygon(
                vec![anchor + tip, anchor + a, anchor + b],
                fg,
                egui::Stroke::NONE,
            ));
        }
        ctx.set_cursor_icon(cursor);
    }

    pub(crate) fn header_menu_button_ui(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let nav_visible = self.ui.navigation_open && !self.playback.fullscreen;
        let nav_tip = if nav_visible {
            self.pick("Ocultar navega\u{00e7}\u{00e3}o", "Hide navigation")
        } else {
            self.pick("Mostrar navega\u{00e7}\u{00e3}o", "Show navigation")
        };
        let palette = self.palette();
        let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
        if response.hovered() {
            ui.painter()
                .circle_filled(rect.center(), 18.0, palette.header_button_bg);
        }
        let c = rect.center();
        let stroke = egui::Stroke::new(1.6, palette.icon_fg);
        for dy in [-5.0_f32, 0.0, 5.0] {
            ui.painter().line_segment(
                [
                    egui::pos2(c.x - 8.5, c.y + dy),
                    egui::pos2(c.x + 8.5, c.y + dy),
                ],
                stroke,
            );
        }
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(nav_tip);
        if response.clicked() {
            self.ui.navigation_open = !self.ui.navigation_open;
        }
        response
    }

    pub(crate) fn header_brand_ui(
        &mut self,
        ui: &mut egui::Ui,
        palette: ThemePalette,
    ) -> egui::Response {
        let region_code = self.settings.prefs.value.language.region_code();
        let title_galley = egui::WidgetText::from(
            RichText::new("FastTube")
                .strong()
                .size(19.5)
                .color(palette.header_button_fg),
        )
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::FontSelection::Default,
        );
        let badge_galley = egui::WidgetText::from(
            RichText::new(region_code)
                .size(9.5)
                .color(palette.neutral_status_fg),
        )
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::FontSelection::Default,
        );

        let logo_w = 30.0;
        let logo_h = 21.0;
        let logo_gap = 5.0;
        let badge_gap = 3.0;
        let total_w =
            logo_w + logo_gap + title_galley.size().x + badge_gap + badge_galley.size().x + 2.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(total_w, 36.0), egui::Sense::click());
        let logo_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + logo_w * 0.5, rect.center().y),
            egui::vec2(logo_w, logo_h),
        );
        paint_youtube_logo_badge_at(ui.painter(), logo_rect);

        let title_x = logo_rect.right() + logo_gap;
        let title_y = rect.center().y - title_galley.size().y * 0.5;
        let title_pos = egui::pos2(title_x, title_y);
        let title_right = title_x + title_galley.size().x;
        ui.painter()
            .galley(title_pos, title_galley, palette.header_button_fg);

        let badge_pos = egui::pos2(title_right + badge_gap, (title_y - 1.5).max(rect.top()));
        ui.painter()
            .galley(badge_pos, badge_galley, palette.neutral_status_fg);

        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(self.pick("P\u{00e1}gina inicial do FastTube", "FastTube Home"))
    }

    pub(crate) fn handle_header_home(&mut self, ctx: &egui::Context) {
        let already_on_default_home = !self.is_watch_page_active()
            && !self.channels.view_open
            && !self.ui.settings_open
            && self.ui.browse_section == BrowseSection::Home
            && !self.ui.search_results_mode
            && self.feed.filter == FeedFilter::All
            && self.feed.home.title.is_none();
        if already_on_default_home && !self.feed.videos.is_empty() {
            return;
        }
        self.ui.settings_open = false;
        self.ui.browse_section = BrowseSection::Home;
        self.ui.search_results_mode = false;
        self.feed.filter = FeedFilter::All;
        self.queue.remote_playlist_id = None;
        self.invalidate_feed_cache();
        self.close_channel_view();
        if self.selected.is_some() {
            if !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
        } else {
            self.sync_player_window_visibility();
        }
        self.feed.home.subscriptions_progressive_streaming = false;
        if !self.restore_cached_home_feed(ctx)
            && (self.feed.home.rx.is_none()
                || self.feed.home.subscriptions_active_request_id.is_some())
            && self.feed.home.batch_rx.is_none()
            && self.feed.search.rx.is_none()
        {
            self.start_fast_home_feed(false, ctx);
        }
    }

    pub(crate) fn header_search_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        search_width: f32,
    ) -> egui::Rect {
        let search_width = search_width.clamp(140.0, 640.0);
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let pill_bg = if dark_theme {
            Color32::from_rgb(18, 18, 18)
        } else {
            Color32::WHITE
        };
        let pill_stroke = if dark_theme {
            Color32::from_rgb(48, 48, 48)
        } else {
            Color32::from_rgb(204, 204, 204)
        };
        let button_bg = if dark_theme {
            Color32::from_rgb(34, 34, 34)
        } else {
            Color32::from_rgb(240, 240, 240)
        };
        let mut trigger_search = false;
        let mut search_changed = false;
        let mut search_has_focus = false;
        let mut search_gained_focus = false;
        let mut search_lost_focus = false;
        let pill_resp = egui::Frame::new()
            .fill(pill_bg)
            .stroke(egui::Stroke::new(1.0, pill_stroke))
            .corner_radius(20)
            .inner_margin(egui::Margin {
                left: 16,
                right: 0,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| {
                ui.set_width((search_width - 16.0).max(124.0));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let show_keyboard_btn = search_width >= 210.0;
                    let kb_slot = if show_keyboard_btn { 30.0 } else { 0.0 };
                    let clear_slot = if self.feed.query.is_empty() {
                        0.0
                    } else {
                        26.0
                    };
                    let input_w = (search_width - 88.0 - kb_slot - clear_slot).max(40.0);
                    let search_hint = self.pick("Pesquisar", "Search").to_owned();
                    let search = ui.add_sized(
                        [input_w, 38.0],
                        egui::TextEdit::singleline(&mut self.feed.query)
                            .id_salt("header_search_input")
                            .frame(egui::Frame::NONE)
                            .vertical_align(egui::Align::Center)
                            .hint_text(search_hint),
                    );
                    search_changed = search.changed();
                    search_has_focus = search.has_focus();
                    search_gained_focus = search.gained_focus();
                    search_lost_focus = search.lost_focus();
                    let enter =
                        search_lost_focus && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if !self.feed.query.is_empty()
                        && ui
                            .add_sized(
                                [24.0, 38.0],
                                egui::Button::new(RichText::new("\u{00d7}").size(16.0))
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Limpar pesquisa")
                            .clicked()
                    {
                        self.feed.query.clear();
                        self.feed.search_suggest.remote_suggestions.clear();
                        self.feed.search_suggest.pending_fetch_query = None;
                        self.feed.search_suggest.highlighted_index = None;
                        if let Some(cancel) = self.feed.search_suggest.cancel.take() {
                            cancel.cancel();
                        }
                        self.feed.search_suggest.rx = None;
                        self.feed.search_suggest.active_request_id = None;
                    }
                    if show_keyboard_btn {
                        let (kb_rect, kb_resp) =
                            ui.allocate_exact_size(egui::vec2(28.0, 38.0), egui::Sense::click());
                        let kb_color = if kb_resp.hovered() {
                            palette.header_button_fg
                        } else {
                            palette.neutral_status_fg
                        };
                        paint_yt_icon(
                            ui.painter(),
                            kb_rect.center(),
                            YtSidebarIcon::Keyboard,
                            kb_color,
                            false,
                        );
                        if kb_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Atalhos do teclado (F1)")
                            .clicked()
                        {
                            self.ui.shortcuts_open = !self.ui.shortcuts_open;
                        }
                    }
                    let (div_rect, _) =
                        ui.allocate_exact_size(egui::vec2(1.0, 38.0), egui::Sense::hover());
                    ui.painter().line_segment(
                        [
                            egui::pos2(div_rect.center().x, div_rect.top()),
                            egui::pos2(div_rect.center().x, div_rect.bottom()),
                        ],
                        egui::Stroke::new(1.0, pill_stroke),
                    );
                    let (btn_rect, btn_resp) =
                        ui.allocate_exact_size(egui::vec2(60.0, 38.0), egui::Sense::click());
                    let btn_corner = egui::CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: 20,
                        se: 20,
                    };
                    ui.painter().rect_filled(btn_rect, btn_corner, button_bg);
                    let icon_rect =
                        egui::Rect::from_center_size(btn_rect.center(), egui::vec2(18.0, 18.0));
                    Icon::Search
                        .image(palette.icon_fg, 18.0)
                        .paint_at(ui, icon_rect);
                    let search_clicked = btn_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Pesquisar (Enter)")
                        .clicked();
                    trigger_search = search_clicked || enter;
                });
            });

        let just_opened = search_gained_focus || search_changed;
        if just_opened {
            self.feed.search_suggest.open = true;
        }
        if search_changed {
            self.feed.search_suggest.highlighted_index = None;
            self.feed.search_suggest.last_input_query = self.feed.query.clone();
            self.feed.search_suggest.last_input_at = Some(Instant::now());
            let trimmed = self.feed.query.trim().to_owned();
            if trimmed.len() < 2
                || trimmed.starts_with("http://")
                || trimmed.starts_with("https://")
                || trimmed.starts_with(':')
            {
                self.feed.search_suggest.pending_fetch_query = None;
                self.feed.search_suggest.remote_suggestions.clear();
                if let Some(cancel) = self.feed.search_suggest.cancel.take() {
                    cancel.cancel();
                }
                self.feed.search_suggest.rx = None;
                self.feed.search_suggest.active_request_id = None;
            } else if !trimmed
                .eq_ignore_ascii_case(self.feed.search_suggest.last_fetched_query.trim())
            {
                self.feed.search_suggest.pending_fetch_query = Some(trimmed);
                ctx.request_repaint_after(Duration::from_millis(220));
            }
        }

        let suggestions = search::merge_search_suggestions(
            &self.feed.query,
            &self.feed.recent_searches,
            &self.feed.search_suggest.remote_suggestions,
            8,
        );
        if let Some(idx) = self.feed.search_suggest.highlighted_index
            && idx >= suggestions.len()
        {
            self.feed.search_suggest.highlighted_index = None;
        }

        if self.feed.search_suggest.open
            && !suggestions.is_empty()
            && (search_has_focus || search_lost_focus)
        {
            let (up_pressed, down_pressed, esc_pressed, enter_pressed) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowUp),
                    input.key_pressed(egui::Key::ArrowDown),
                    input.key_pressed(egui::Key::Escape),
                    input.key_pressed(egui::Key::Enter),
                )
            });
            if down_pressed {
                self.feed.search_suggest.highlighted_index =
                    Some(match self.feed.search_suggest.highlighted_index {
                        None => 0,
                        Some(idx) => (idx + 1) % suggestions.len(),
                    });
            } else if up_pressed {
                self.feed.search_suggest.highlighted_index =
                    Some(match self.feed.search_suggest.highlighted_index {
                        None | Some(0) => suggestions.len() - 1,
                        Some(idx) => idx - 1,
                    });
            } else if esc_pressed {
                self.feed.search_suggest.open = false;
                self.feed.search_suggest.highlighted_index = None;
            } else if enter_pressed
                && let Some(idx) = self.feed.search_suggest.highlighted_index
                && let Some(item) = suggestions.get(idx)
            {
                self.feed.query = item.text.clone();
                trigger_search = true;
            }
        }

        if !trigger_search && self.feed.search_suggest.open && !suggestions.is_empty() {
            let pill_rect = pill_resp.response.rect;
            let dropdown_bg = if dark_theme {
                Color32::from_rgb(33, 33, 33)
            } else {
                Color32::WHITE
            };
            let dropdown_hover = if dark_theme {
                Color32::from_rgb(52, 52, 52)
            } else {
                Color32::from_rgb(238, 238, 238)
            };
            let mut clicked_query: Option<String> = None;
            let mut removed_recent: Option<String> = None;
            let mut dropdown_interacted = false;

            let area_resp = egui::Area::new(egui::Id::new("header_search_suggestions_popup"))
                .fixed_pos(egui::pos2(pill_rect.left(), pill_rect.bottom() + 4.0))
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(dropdown_bg)
                        .stroke(egui::Stroke::new(1.0, pill_stroke))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(6, 6))
                        .show(ui, |ui| {
                            let inner_w = (pill_rect.width() - 12.0).max(200.0);
                            ui.set_width(inner_w);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                for (idx, item) in suggestions.iter().enumerate() {
                                    let is_highlighted =
                                        self.feed.search_suggest.highlighted_index == Some(idx);
                                    let row_w = inner_w;
                                    let delete_w = if item.is_recent { 26.0 } else { 0.0 };
                                    let main_w = (row_w - delete_w - 4.0).max(120.0);
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let prefix = if item.is_recent { "🕒 " } else { "🔍 " };
                                        let row_btn = egui::Button::new(
                                            RichText::new(format!("{prefix}{}", item.text))
                                                .size(13.5)
                                                .color(palette.channel_fg),
                                        )
                                        .fill(if is_highlighted {
                                            dropdown_hover
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .corner_radius(8)
                                        .min_size(egui::vec2(main_w, 30.0));
                                        let row_resp = ui
                                            .add(row_btn)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if row_resp.hovered() {
                                            self.feed.search_suggest.highlighted_index = Some(idx);
                                        }
                                        if row_resp.clicked() {
                                            clicked_query = Some(item.text.clone());
                                            dropdown_interacted = true;
                                        }
                                        if item.is_recent {
                                            let del_btn = egui::Button::new(
                                                RichText::new("\u{00d7}")
                                                    .size(14.0)
                                                    .color(palette.neutral_status_fg),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .corner_radius(6)
                                            .min_size(egui::vec2(24.0, 28.0));
                                            if ui
                                                .add(del_btn)
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text("Remover das pesquisas recentes")
                                                .clicked()
                                            {
                                                removed_recent = Some(item.text.clone());
                                                dropdown_interacted = true;
                                            }
                                        }
                                    });
                                }
                            });
                        });
                });

            if let Some(recent) = removed_recent {
                self.remove_recent_search(&recent);
            }
            if let Some(selected_query) = clicked_query {
                self.feed.query = selected_query;
                trigger_search = true;
            } else if !just_opened && !dropdown_interacted {
                let clicked_outside = ctx.input(|input| {
                    input.pointer.primary_pressed().then(|| {
                        input.pointer.interact_pos().is_some_and(|pos| {
                            !pill_rect.contains(pos) && !area_resp.response.rect.contains(pos)
                        })
                    })
                });
                if clicked_outside == Some(true) {
                    self.feed.search_suggest.open = false;
                    self.feed.search_suggest.highlighted_index = None;
                }
            }
        }

        if trigger_search {
            self.feed.search_suggest.open = false;
            self.feed.search_suggest.highlighted_index = None;
            let query = self.feed.query.clone();
            if let Some(direct) = command_palette::parse_direct_youtube_target(&query) {
                self.record_recent_search(&query);
                self.open_direct_youtube_target(direct, ctx);
            } else {
                self.ui.search_results_mode = true;
                self.ui.browse_section = BrowseSection::Home;
                self.feed.filter = FeedFilter::All;
                self.queue.remote_playlist_id = None;
                self.feed.search_chip = search::SearchChip::All;
                self.feed.search_filters_open = false;
                self.feed.search_save_modal_video = None;
                self.feed.search_report_modal_video = None;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.record_recent_search(&query);
                self.start_search(ctx);
            }
        }
        pill_resp.response.rect
    }

    pub(crate) fn header_recommendation_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> egui::Rect {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::click());
        let bg = if response.hovered() {
            palette.card_alt_bg
        } else {
            palette.header_button_bg
        };
        ui.painter().circle_filled(rect.center(), 20.0, bg);
        paint_yt_icon(
            ui.painter(),
            rect.center(),
            YtSidebarIcon::Mic,
            palette.header_button_fg,
            false,
        );
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Ativar recomendados para voc\u{00ea}")
            .clicked()
        {
            self.ui.browse_section = BrowseSection::Home;
            self.ui.search_results_mode = false;
            self.feed.filter = FeedFilter::Recommended;
            self.close_channel_view();
            if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
            let restored = self.restore_cached_home_feed(ctx);
            self.invalidate_feed_cache();
            self.ensure_feed_cache();
            if (!restored || self.feed.cache.ranked.len() < 6)
                && self.feed.search.rx.is_none()
                && self.feed.home.rx.is_none()
                && self.feed.home.batch_rx.is_none()
            {
                self.start_fast_home_feed(false, ctx);
            }
        }
        rect
    }

    pub(crate) fn navigate_from_shortcut(&mut self, ctx: &egui::Context, section: BrowseSection) {
        match section {
            BrowseSection::Subscriptions => {
                self.open_subscriptions_section(false, ctx);
                return;
            }
            BrowseSection::Shorts => {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.queue.remote_playlist_id = None;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.ui.browse_section = BrowseSection::Shorts;
                self.feed.filter = FeedFilter::Shorts;
                if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
                    let _ = self.restore_cached_home_feed(ctx);
                }
                self.invalidate_feed_cache();
                self.ensure_feed_cache();
                return;
            }
            _ => {}
        }
        self.ui.settings_open = false;
        self.ui.search_results_mode = false;
        self.queue.remote_playlist_id = None;
        self.close_channel_view();
        if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
            self.close_video(ctx);
        }
        self.ui.browse_section = section;
        if section == BrowseSection::Home {
            self.feed.filter = FeedFilter::All;
            let _ = self.restore_cached_home_feed(ctx);
            self.invalidate_feed_cache();
        } else if matches!(self.feed.filter, FeedFilter::Shorts) {
            self.feed.filter = FeedFilter::All;
            self.invalidate_feed_cache();
        }
    }

    pub(crate) fn shortcuts_window_ui(&mut self, ctx: &egui::Context) {
        if !self.ui.shortcuts_open {
            return;
        }
        let mut open = self.ui.shortcuts_open;
        let palette = self.palette();
        let kb = self.settings.prefs.value.keybindings.clone();
        let pt = self.ui_pt();
        let title = if pt {
            "Atalhos e acessibilidade"
        } else {
            "Shortcuts & accessibility"
        };
        let play_key = if pt {
            format!("Espaço / {}", kb.play_pause)
        } else {
            format!("Space / {}", kb.play_pause)
        };
        let seek_keys = format!("{} / {}", kb.seek_back, kb.seek_forward);
        let ab_keys = format!("{} / {} / {}", kb.ab_loop_a, kb.ab_loop_b, kb.ab_loop_clear);
        let seek_label = if pt {
            format!("voltar ou avançar {} segundos", self.seek_step_secs())
        } else {
            format!("seek back or forward {} seconds", self.seek_step_secs())
        };
        let screen = ctx.content_rect();

        let paint_shortcut_row = |ui: &mut egui::Ui, key: &str, action: &str| {
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(palette.header_button_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(7, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(key)
                                .size(11.5)
                                .monospace()
                                .strong()
                                .color(palette.channel_fg),
                        );
                    });
                ui.add_space(4.0);
                ui.label(RichText::new(action).size(12.5).color(palette.snippet_fg));
            });
        };

        let modal_closed = app_modal_window(
            ctx,
            "shortcuts_modal_window",
            title,
            500.0,
            &palette,
            |ui| {
                ui.label(
                RichText::new(if pt {
                    "Use Tab e Shift+Tab para percorrer controles. Enter ou Espaço ativa o item focado."
                } else {
                    "Use Tab and Shift+Tab to move focus. Enter or Space activates the focused item."
                })
                .size(12.0)
                .color(palette.neutral_status_fg),
            );
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                .id_salt("shortcuts_modal_scroll")
                .max_height((screen.height() - 180.0).clamp(220.0, 480.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(if pt { "Reprodução" } else { "Playback" })
                            .size(13.5)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    ui.add_space(4.0);
                    for (key, action) in [
                        (play_key.as_str(), if pt { "reproduzir/pausar" } else { "play/pause" }),
                        (seek_keys.as_str(), seek_label.as_str()),
                        ("J / L", if pt { "voltar ou avançar 10 segundos" } else { "seek back or forward 10 seconds" }),
                        (", / .", if pt { "recuar ou avançar 1 quadro (quando pausado)" } else { "step back or forward 1 frame (while paused)" }),
                        ("< / >", if pt { "diminuir ou aumentar velocidade (0.25x – 3x)" } else { "decrease or increase speed (0.25x – 3x)" }),
                        (ab_keys.as_str(), if pt { "definir ponto A, ponto B ou limpar loop A-B" } else { "set point A, point B, or clear A-B loop" }),
                        (kb.screenshot.as_str(), if pt { "capturar quadro do vídeo (PNG)" } else { "capture video frame (PNG)" }),
                        (kb.mute.as_str(), if pt { "silenciar" } else { "mute" }),
                        (kb.audio_only.as_str(), if pt { "alternar somente áudio" } else { "toggle audio only" }),
                        ("C", if pt { "ativar ou desativar legendas" } else { "toggle captions" }),
                        (kb.theater.as_str(), if pt { "modo teatro" } else { "theater mode" }),
                        (kb.fullscreen.as_str(), if pt { "tela cheia" } else { "fullscreen" }),
                        (kb.mini_player.as_str(), if pt { "alternar miniplayer" } else { "toggle mini player" }),
                        ("N / Shift+N", if pt { "próximo vídeo" } else { "next video" }),
                        ("0 – 9", if pt { "ir para 0% – 90% do vídeo" } else { "jump to 0% – 90% of video" }),
                        ("↑ / ↓", if pt { "Short anterior / próximo (no modo Shorts)" } else { "previous / next Short (in Shorts mode)" }),
                        ("Esc", if pt { "fechar tela cheia, ajustes ou painel" } else { "close fullscreen, settings, or panel" }),
                    ] {
                        paint_shortcut_row(ui, key, action);
                        ui.add_space(3.0);
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(if pt { "Navegação" } else { "Navigation" })
                            .size(13.5)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    ui.add_space(4.0);
                    let palette_keys = format!("{} / Ctrl+P", kb.command_palette);
                    for (key, action) in [
                        (palette_keys.as_str(), if pt { "paleta de comandos" } else { "command palette" }),
                        ("Alt+← / Alt+→", if pt { "voltar / avançar histórico de telas" } else { "navigate back / forward" }),
                        ("Alt+1", if pt { "Início" } else { "Home" }),
                        ("Alt+2", "Shorts"),
                        ("Alt+3", if pt { "Inscrições" } else { "Subscriptions" }),
                        ("Alt+4", if pt { "Você" } else { "You" }),
                        ("Alt+5", if pt { "Histórico" } else { "History" }),
                        ("Alt+6", "Downloads"),
                    ] {
                        paint_shortcut_row(ui, key, action);
                        ui.add_space(3.0);
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(if pt { "Legendas" } else { "Captions" })
                            .size(13.5)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    ui.add_space(3.0);
                    ui.label(
                        RichText::new(if pt {
                            "Em Transcrição, o trecho atual acompanha o vídeo, a fonte pode ser ampliada e o modo de alto contraste fica disponível na própria aba."
                        } else {
                            "In Transcript, the active segment follows playback, font size can be increased, and high contrast mode is available in the tab."
                        })
                        .size(12.0)
                        .color(palette.snippet_fg),
                    );
                });
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_pill_button(ui, if pt { "Fechar" } else { "Close" }, false, &palette)
                        .clicked()
                    {
                        open = false;
                    }
                });
            },
        );
        if modal_closed {
            open = false;
        }
        self.ui.shortcuts_open = open;
    }

    pub(crate) fn handle_help_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (toggle_help, escape_pressed) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::F1),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if toggle_help {
            self.ui.shortcuts_open = !self.ui.shortcuts_open;
        }
        if escape_pressed && self.ui.shortcuts_open {
            self.ui.shortcuts_open = false;
        }
    }

    pub(crate) fn handle_player_shortcuts(
        &mut self,
        ctx: &egui::Context,
        allow_escape_exit_fullscreen: bool,
    ) {
        if !self.is_watch_page_active() || self.ui.settings_open || ctx.egui_wants_keyboard_input()
        {
            return;
        }
        let short_mode = self.is_watch_short_mode();
        let default_seek_step = f64::from(self.seek_step_secs());
        let kb = self.settings.prefs.value.keybindings.clone();
        let (
            toggle,
            back_step,
            forward_step,
            mute,
            captions,
            theater,
            fullscreen,
            escape_pressed,
            mini_player,
            audio_only,
            next_shortcut,
            short_prev,
            short_next,
            pct_seek,
            frame_back,
            frame_forward,
            speed_down,
            speed_up,
            set_loop_a,
            set_loop_b,
            clear_loop_ab,
            screenshot,
        ) = ctx.input(|input| {
            let no_alt_ctrl = !input.modifiers.alt && !input.modifiers.ctrl;
            let no_mods = no_alt_ctrl && !input.modifiers.shift;
            let mut text_comma = false;
            let mut text_period = false;
            let mut text_lt = false;
            let mut text_gt = false;
            let mut text_open_bracket = false;
            let mut text_close_bracket = false;
            let mut text_backslash = false;
            if no_alt_ctrl {
                for ev in &input.events {
                    if let egui::Event::Text(text) = ev {
                        match text.as_str() {
                            "," if no_mods => text_comma = true,
                            "." if no_mods => text_period = true,
                            "<" => text_lt = true,
                            ">" => text_gt = true,
                            "[" => text_open_bracket = true,
                            "]" => text_close_bracket = true,
                            "\\" => text_backslash = true,
                            _ => {}
                        }
                    }
                }
            }
            let pct = if no_alt_ctrl {
                [
                    (egui::Key::Num0, 0.0),
                    (egui::Key::Num1, 0.1),
                    (egui::Key::Num2, 0.2),
                    (egui::Key::Num3, 0.3),
                    (egui::Key::Num4, 0.4),
                    (egui::Key::Num5, 0.5),
                    (egui::Key::Num6, 0.6),
                    (egui::Key::Num7, 0.7),
                    (egui::Key::Num8, 0.8),
                    (egui::Key::Num9, 0.9),
                ]
                .into_iter()
                .find_map(|(k, p)| input.key_pressed(k).then_some(p))
            } else {
                None
            };
            let back = if no_alt_ctrl && !short_mode && input.key_pressed(egui::Key::J) {
                Some(10.0)
            } else if settings::Keybindings::matches_input(input, &kb.seek_back) {
                Some(default_seek_step)
            } else {
                None
            };
            let forward = if no_alt_ctrl && input.key_pressed(egui::Key::L) {
                Some(10.0)
            } else if settings::Keybindings::matches_input(input, &kb.seek_forward) {
                Some(default_seek_step)
            } else {
                None
            };
            (
                (no_alt_ctrl && input.key_pressed(egui::Key::Space))
                    || (!short_mode && settings::Keybindings::matches_input(input, &kb.play_pause)),
                back,
                forward,
                settings::Keybindings::matches_input(input, &kb.mute),
                no_alt_ctrl && input.key_pressed(egui::Key::C),
                settings::Keybindings::matches_input(input, &kb.theater),
                settings::Keybindings::matches_input(input, &kb.fullscreen),
                input.key_pressed(egui::Key::Escape),
                settings::Keybindings::matches_input(input, &kb.mini_player),
                settings::Keybindings::matches_input(input, &kb.audio_only),
                no_alt_ctrl && input.key_pressed(egui::Key::N),
                short_mode
                    && no_alt_ctrl
                    && (input.key_pressed(egui::Key::ArrowUp) || input.key_pressed(egui::Key::K)),
                short_mode
                    && no_alt_ctrl
                    && (input.key_pressed(egui::Key::ArrowDown) || input.key_pressed(egui::Key::J)),
                pct,
                no_mods && (input.key_pressed(egui::Key::Comma) || text_comma),
                no_mods && (input.key_pressed(egui::Key::Period) || text_period),
                (no_alt_ctrl && input.modifiers.shift && input.key_pressed(egui::Key::Comma))
                    || text_lt,
                (no_alt_ctrl && input.modifiers.shift && input.key_pressed(egui::Key::Period))
                    || text_gt,
                settings::Keybindings::matches_input(input, &kb.ab_loop_a)
                    || (kb.ab_loop_a == "[" && text_open_bracket),
                settings::Keybindings::matches_input(input, &kb.ab_loop_b)
                    || (kb.ab_loop_b == "]" && text_close_bracket),
                settings::Keybindings::matches_input(input, &kb.ab_loop_clear)
                    || (kb.ab_loop_clear == "\\" && text_backslash),
                settings::Keybindings::matches_input(input, &kb.screenshot),
            )
        });

        let any_activity = toggle
            || back_step.is_some()
            || forward_step.is_some()
            || mute
            || captions
            || theater
            || fullscreen
            || mini_player
            || audio_only
            || next_shortcut
            || short_prev
            || short_next
            || pct_seek.is_some()
            || frame_back
            || frame_forward
            || speed_down
            || speed_up
            || set_loop_a
            || set_loop_b
            || clear_loop_ab
            || screenshot;
        if any_activity {
            self.playback.overlay_last_activity = Instant::now();
        }

        if toggle || back_step.is_some() || forward_step.is_some() || mute || pct_seek.is_some() {
            let is_live = self.active_video_is_live();
            let live_dvr = self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.live_dvr);
            let mut key_seek_pos: Option<f64> = None;
            let mut key_seek_dur: Option<f64> = None;
            let mut key_flush_pause = false;
            let mut trigger_go_live = false;
            let fallback_pos = self
                .playback
                .active
                .as_ref()
                .map_or(0.0, |active| active.position_ms as f64 / 1000.0);
            let fallback_dur = self
                .playback
                .active
                .as_ref()
                .and_then(|a| a.duration_ms)
                .filter(|&d| d > 0)
                .map(|d| d as f64 / 1000.0);

            #[cfg(windows)]
            if let Some(player) = &self.player {
                let was_ended = toggle && player.flag_cstr(c"eof-reached") == Some(true);
                let was_paused = toggle && player.flag_cstr(c"pause") == Some(true);
                let cur_pos = if back_step.is_some() || forward_step.is_some() {
                    player
                        .double_cstr(c"time-pos")
                        .filter(|&pos| pos >= 0.0)
                        .unwrap_or(fallback_pos)
                } else {
                    0.0
                };
                let cur_dur = if back_step.is_some() || forward_step.is_some() || pct_seek.is_some()
                {
                    player
                        .double_cstr(c"duration")
                        .filter(|&d| d > 0.0)
                        .or(fallback_dur)
                } else {
                    None
                };
                key_seek_dur = cur_dur;
                let pct_target = pct_seek.and_then(|pct| cur_dur.map(|dur| (pct, dur * pct)));
                let result = if toggle {
                    if was_ended && is_live {
                        trigger_go_live = true;
                        Ok(())
                    } else if was_ended {
                        player
                            .seek(0.0, "absolute+exact")
                            .and_then(|()| player.set("pause", "no"))
                    } else {
                        player.set("pause", if was_paused { "no" } else { "yes" })
                    }
                } else if let Some(step) = back_step {
                    if is_live && !live_dvr {
                        Ok(())
                    } else {
                        player.seek(-step, "relative")
                    }
                } else if let Some(step) = forward_step {
                    if is_live && !live_dvr {
                        Ok(())
                    } else if is_live {
                        let target = cur_pos + step;
                        if cur_dur.is_some_and(|dur| dur <= 1.5 || target >= (dur - 1.5).max(0.0)) {
                            trigger_go_live = true;
                            Ok(())
                        } else {
                            let clamped =
                                cur_dur.map_or(target, |dur| target.min((dur - 1.5).max(0.0)));
                            key_seek_pos = Some(clamped);
                            player.seek(clamped, "absolute+exact")
                        }
                    } else {
                        player.seek(step, "relative")
                    }
                } else if let Some((pct, target)) = pct_target {
                    if is_live && !live_dvr {
                        Ok(())
                    } else if is_live {
                        if pct >= 0.985
                            || cur_dur
                                .is_some_and(|dur| dur <= 1.5 || target >= (dur - 1.5).max(0.0))
                        {
                            trigger_go_live = true;
                            Ok(())
                        } else {
                            let clamped =
                                cur_dur.map_or(target, |dur| target.min((dur - 1.5).max(0.0)));
                            key_seek_pos = Some(clamped);
                            player.seek(clamped, "absolute+exact")
                        }
                    } else {
                        player.seek(target, "absolute+exact")
                    }
                } else if mute {
                    player.set(
                        "mute",
                        if player.flag_cstr(c"mute") == Some(true) {
                            "no"
                        } else {
                            "yes"
                        },
                    )
                } else {
                    Ok(())
                };
                match result {
                    Ok(()) => {
                        if toggle && !trigger_go_live {
                            if was_ended {
                                key_seek_pos = Some(0.0);
                            } else if !was_paused {
                                key_flush_pause = true;
                            }
                        } else if let Some(step) = back_step
                            && (!is_live || live_dvr)
                        {
                            key_seek_pos = Some((cur_pos - step).max(0.0));
                        } else if let Some(step) = forward_step
                            && !is_live
                        {
                            key_seek_pos =
                                Some(cur_dur.map_or(cur_pos + step, |d| (cur_pos + step).min(d)));
                        } else if let Some((_, target)) = pct_target
                            && !is_live
                        {
                            key_seek_pos = Some(target);
                        }
                    }
                    Err(error) => self.message = error,
                }
            } else if !is_live || live_dvr {
                key_seek_dur = fallback_dur;
                if let Some(step) = back_step {
                    key_seek_pos = Some((fallback_pos - step).max(0.0));
                } else if let Some(step) = forward_step {
                    if is_live
                        && fallback_dur.is_some_and(|dur| {
                            dur <= 1.5 || fallback_pos + step >= (dur - 1.5).max(0.0)
                        })
                    {
                        trigger_go_live = true;
                    } else {
                        let target = fallback_pos + step;
                        key_seek_pos = Some(if is_live {
                            fallback_dur.map_or(target, |dur| target.min((dur - 1.5).max(0.0)))
                        } else {
                            fallback_dur.map_or(target, |dur| target.min(dur))
                        });
                    }
                } else if let Some(pct) = pct_seek
                    && let Some(dur) = fallback_dur
                {
                    let target = dur * pct;
                    if is_live && (pct >= 0.985 || dur <= 1.5 || target >= (dur - 1.5).max(0.0)) {
                        trigger_go_live = true;
                    } else {
                        key_seek_pos = Some(if is_live {
                            target.min((dur - 1.5).max(0.0))
                        } else {
                            target
                        });
                    }
                }
            }

            if trigger_go_live {
                if let Some(video) = self.selected.clone() {
                    self.go_live_edge(&video);
                }
            } else if let Some(pos) = key_seek_pos {
                self.record_manual_seek(pos, key_seek_dur, Instant::now());
            } else if key_flush_pause {
                self.flush_playback_history_at(Instant::now());
            }
        }

        #[cfg(windows)]
        if (frame_back || frame_forward)
            && !self.active_video_is_live()
            && let Some(player) = &self.player
            && player.flag_cstr(c"pause") == Some(true)
        {
            let res = if frame_back {
                player.frame_back_step()
            } else {
                player.frame_step()
            };
            if let Err(error) = res {
                self.message = error;
            }
        }
        #[cfg(not(windows))]
        let _ = (frame_back, frame_forward);

        if speed_up != speed_down {
            #[cfg(windows)]
            let current_speed = self
                .player
                .as_ref()
                .and_then(|p| p.double_cstr(c"speed"))
                .filter(|s| s.is_finite() && *s > 0.0)
                .unwrap_or(self.playback.speed);
            #[cfg(not(windows))]
            let current_speed = self.playback.speed;
            let next_speed = overlay::step_speed_preset(current_speed, speed_up);
            self.set_player_speed(next_speed);
            self.message = format!("Velocidade: {:.2}x", next_speed);
        }

        if set_loop_a {
            let pos = self.current_playback_position_for_ab(None);
            self.set_ab_loop_point_a(pos);
        }
        if set_loop_b {
            let pos = self.current_playback_position_for_ab(None);
            self.set_ab_loop_point_b(pos);
        }
        if clear_loop_ab {
            self.clear_ab_loop_points();
        }
        if screenshot && let Some(video) = self.selected.clone() {
            let pos = self.current_playback_position_for_ab(None);
            self.capture_player_screenshot(&video, pos);
        }

        if captions {
            self.toggle_player_captions(ctx);
        }
        if theater && !self.playback.fullscreen {
            self.toggle_theater_mode();
        }
        if fullscreen
            || (escape_pressed && allow_escape_exit_fullscreen && self.playback.fullscreen)
        {
            self.toggle_fullscreen(ctx);
        }
        if mini_player {
            self.toggle_mini_player(ctx);
        }
        if audio_only {
            self.toggle_audio_only_mode();
        }
        if next_shortcut {
            if !self.queue.data.is_empty() {
                self.request_queue_advance();
            } else {
                let next = if short_mode {
                    self.next_or_prev_short(true)
                } else {
                    self.next_recommended_video()
                };
                if let Some(video) = next {
                    self.playback.pending_open_video = Some(video);
                }
            }
        } else if short_prev {
            if let Some(video) = self.next_or_prev_short(false) {
                self.playback.pending_open_video = Some(video);
            }
        } else if short_next && let Some(video) = self.next_or_prev_short(true) {
            self.playback.pending_open_video = Some(video);
        }
    }

    pub(crate) fn header_window_controls_ui(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> egui::Rect {
        let is_maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let pt = self.ui_pt();
        let close_tip = if pt { "Fechar" } else { "Close" };
        let max_tip = if is_maximized {
            if pt { "Restaurar" } else { "Restore" }
        } else if pt {
            "Maximizar"
        } else {
            "Maximize"
        };
        let min_tip = if pt { "Minimizar" } else { "Minimize" };

        let close_resp =
            paint_window_control_button(ui, WindowControlKind::Close, close_tip, &palette);
        if close_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.add_space(2.0);
        let max_kind = if is_maximized {
            WindowControlKind::Restore
        } else {
            WindowControlKind::Maximize
        };
        let max_resp = paint_window_control_button(ui, max_kind, max_tip, &palette);
        if max_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
        }
        ui.add_space(2.0);
        let min_resp =
            paint_window_control_button(ui, WindowControlKind::Minimize, min_tip, &palette);
        if min_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        close_resp.rect.union(max_resp.rect).union(min_resp.rect)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn header_actions_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        show_back_button: bool,
        compact_create: bool,
        show_create: bool,
        viewport_width: f32,
    ) -> egui::Rect {
        let mut open_my_channel = false;
        let mut open_library = false;
        let mut toggle_settings = false;
        let mut toggle_shortcuts = false;
        let mut toggle_theme = false;
        let mut toggle_palette = false;
        let mut sign_out = false;
        let pt = self.ui_pt();
        let ultra_narrow = viewport_width < 520.0;
        let tight_gap = if ultra_narrow { 4.0 } else { 6.0 };
        let mut widgets_rect = egui::Rect::NOTHING;

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(6.0);
            widgets_rect |= self.header_window_controls_ui(ui, ctx, palette);

            let div_pad = if ultra_narrow { 4.0 } else { 8.0 };
            ui.add_space(div_pad);
            let (div_rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
            ui.painter().line_segment(
                [
                    egui::pos2(div_rect.center().x, div_rect.top()),
                    egui::pos2(div_rect.center().x, div_rect.bottom()),
                ],
                egui::Stroke::new(1.0, palette.card_alt_stroke),
            );
            widgets_rect |= div_rect;
            ui.add_space(div_pad);

            if self.auth.authenticated {
                let avatar_url = self.auth.my_channel_avatar_url.clone();
                let label = self
                    .auth
                    .my_channel_title
                    .clone()
                    .unwrap_or_else(|| if pt { "Voc\u{00ea}" } else { "You" }.to_owned());
                let avatar_resp = self.paint_avatar_or_initials(
                    ui,
                    ctx,
                    avatar_url.as_deref(),
                    &label,
                    32.0,
                    true,
                );
                widgets_rect |= avatar_resp.rect;
                egui::Popup::menu(&avatar_resp).show(|ui| {
                    ui.set_min_width(210.0);
                    ui.label(RichText::new(&label).strong());
                    ui.separator();
                    if ui
                        .button(if pt { "Seu canal" } else { "Your channel" })
                        .clicked()
                    {
                        open_my_channel = true;
                        ui.close();
                    }
                    if ui.button(if pt { "Voc\u{00ea}" } else { "You" }).clicked() {
                        open_library = true;
                        ui.close();
                    }
                    let appearance_label = match (self.settings.theme.value, pt) {
                        (Theme::Dark, true) => "Apar\u{00ea}ncia: Escuro",
                        (Theme::Light, true) => "Apar\u{00ea}ncia: Claro",
                        (Theme::Dark, false) => "Appearance: Dark",
                        (Theme::Light, false) => "Appearance: Light",
                    };
                    if ui.button(appearance_label).clicked() {
                        toggle_theme = true;
                        ui.close();
                    }
                    if ui
                        .button(if pt {
                            "Configura\u{00e7}\u{00f5}es"
                        } else {
                            "Settings"
                        })
                        .clicked()
                    {
                        toggle_settings = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(if pt { "Sair da conta" } else { "Sign out" })
                        .clicked()
                    {
                        sign_out = true;
                        ui.close();
                    }
                });
            } else {
                let accent = Color32::from_rgb(62, 166, 255);
                let button = egui::Button::new(
                    RichText::new(if pt { "Fazer login" } else { "Sign in" })
                        .size(if ultra_narrow { 12.0 } else { 13.0 })
                        .strong()
                        .color(accent),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(62, 166, 255, 110),
                ))
                .corner_radius(18);
                let login_w = if viewport_width < 560.0 { 80.0 } else { 98.0 };
                let login_resp = ui
                    .add_sized([login_w, 34.0], button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Entrar com Google para carregar inscri\u{00e7}\u{00f5}es e playlists"
                    } else {
                        "Sign in with Google to load subscriptions and playlists"
                    });
                widgets_rect |= login_resp.rect;
                if login_resp.clicked() {
                    self.start_reauth(ctx);
                }
            }

            ui.add_space(tight_gap);
            let has_alert = self.settings.buffer.error.is_some()
                || self.settings.playback.error.is_some()
                || self.settings.theme.error.is_some()
                || self.settings.prefs.error.is_some()
                || self.has_pending_buffer_restart()
                || self.has_pending_hwdec_restart();
            let bell_size = if ultra_narrow { 34.0 } else { 38.0 };
            let (bell_rect, bell_resp) =
                ui.allocate_exact_size(egui::vec2(bell_size, bell_size), egui::Sense::click());
            widgets_rect |= bell_rect;
            if bell_resp.hovered() || self.ui.settings_open {
                ui.painter().circle_filled(
                    bell_rect.center(),
                    bell_size * 0.5,
                    palette.header_button_bg,
                );
            }
            let bell_color = if has_alert {
                palette.warning_fg
            } else {
                palette.header_button_fg
            };
            paint_yt_icon(
                ui.painter(),
                bell_rect.center(),
                YtSidebarIcon::Bell,
                bell_color,
                self.ui.settings_open,
            );
            if has_alert {
                ui.painter().circle_filled(
                    egui::pos2(bell_rect.center().x + 6.0, bell_rect.center().y - 6.0),
                    3.0,
                    Color32::from_rgb(255, 0, 51),
                );
            }
            let settings_tip = format!(
                "{} (buffer: {} \u{2022} {}: {})",
                if pt {
                    "Notifica\u{00e7}\u{00f5}es e configura\u{00e7}\u{00f5}es"
                } else {
                    "Notifications & settings"
                },
                buffer_profile_summary(self.settings.buffer.profile),
                if pt { "tema" } else { "theme" },
                theme_name(self.settings.theme.value)
            );
            let bell_resp = bell_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(settings_tip);
            egui::Popup::menu(&bell_resp).show(|ui| {
                ui.set_min_width(220.0);
                if has_alert {
                    ui.colored_label(
                        palette.warning_fg,
                        if pt {
                            "H\u{00e1} avisos pendentes nas configura\u{00e7}\u{00f5}es."
                        } else {
                            "There are pending warnings in settings."
                        },
                    );
                    ui.separator();
                }
                let next_theme_label = match (self.settings.theme.value, pt) {
                    (Theme::Dark, true) => "Alternar para tema claro",
                    (Theme::Light, true) => "Alternar para tema escuro",
                    (Theme::Dark, false) => "Switch to light theme",
                    (Theme::Light, false) => "Switch to dark theme",
                };
                if ui.button(next_theme_label).clicked() {
                    toggle_theme = true;
                    ui.close();
                }
                if ui
                    .button(if pt {
                        "Configura\u{00e7}\u{00f5}es"
                    } else {
                        "Settings"
                    })
                    .clicked()
                {
                    toggle_settings = true;
                    ui.close();
                }
                if ui
                    .button(if pt {
                        "Paleta de comandos (Ctrl+K)"
                    } else {
                        "Command palette (Ctrl+K)"
                    })
                    .clicked()
                {
                    toggle_palette = true;
                    ui.close();
                }
                if ui
                    .button(if pt {
                        "Atalhos do teclado (F1)"
                    } else {
                        "Keyboard shortcuts (F1)"
                    })
                    .clicked()
                {
                    toggle_shortcuts = true;
                    ui.close();
                }
            });

            if show_create {
                ui.add_space(tight_gap);
                let create_label = if compact_create {
                    "+"
                } else if pt {
                    "+ Criar"
                } else {
                    "+ Create"
                };
                let create_size = if compact_create {
                    egui::vec2(36.0, 36.0)
                } else {
                    egui::vec2(86.0, 36.0)
                };
                let create_button = egui::Button::new(
                    RichText::new(create_label)
                        .size(if compact_create { 17.0 } else { 13.5 })
                        .strong()
                        .color(palette.header_button_fg),
                )
                .fill(palette.header_button_bg)
                .corner_radius(18);
                let create_resp = ui
                    .add_sized(create_size, create_button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Criar playlist ou abrir link"
                    } else {
                        "Create playlist or open link"
                    });
                widgets_rect |= create_resp.rect;
                egui::Popup::menu(&create_resp).show(|ui| {
                    ui.set_min_width(200.0);
                    if ui
                        .button(if pt { "Nova playlist" } else { "New playlist" })
                        .clicked()
                    {
                        open_library = true;
                        ui.close();
                    }
                    if ui
                        .button(if pt {
                            "Abrir link do YouTube (Ctrl+K)"
                        } else {
                            "Open YouTube link (Ctrl+K)"
                        })
                        .clicked()
                    {
                        toggle_palette = true;
                        ui.close();
                    }
                    if ui
                        .button(if pt {
                            "Configura\u{00e7}\u{00f5}es"
                        } else {
                            "Settings"
                        })
                        .clicked()
                    {
                        toggle_settings = true;
                        ui.close();
                    }
                    if ui
                        .button(if pt {
                            "Atalhos do teclado"
                        } else {
                            "Keyboard shortcuts"
                        })
                        .clicked()
                    {
                        toggle_shortcuts = true;
                        ui.close();
                    }
                });
            }

            if show_back_button {
                ui.add_space(tight_gap);
                let back_size = if ultra_narrow { 34.0 } else { 36.0 };
                let back_button = egui::Button::new(RichText::new("\u{2190}").size(17.0))
                    .fill(palette.header_button_bg)
                    .corner_radius(18);
                let back_resp = ui
                    .add_sized([back_size, back_size], back_button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Fechar reprodu\u{00e7}\u{00e3}o e voltar"
                    } else {
                        "Close video and go back"
                    });
                widgets_rect |= back_resp.rect;
                if back_resp.clicked() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
            }
        });

        if open_my_channel {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
            if let Some(target) = self.auth.my_channel.clone() {
                self.open_channel(target, ctx);
            } else {
                self.start_fetch_my_channel(true, ctx);
            }
        }
        if open_library {
            self.navigate_from_shortcut(ctx, BrowseSection::Library);
        }
        if toggle_settings {
            if !self.ui.settings_open && self.selected.is_some() {
                let _ = self.maybe_enter_auto_mini_player(ctx);
            }
            self.ui.settings_open = !self.ui.settings_open;
            self.sync_player_window_visibility();
        }
        if toggle_shortcuts {
            self.ui.shortcuts_open = !self.ui.shortcuts_open;
        }
        if toggle_palette {
            self.ui.command_palette.toggle();
        }
        if toggle_theme {
            let next = if self.settings.theme.value == Theme::Dark {
                Theme::Light
            } else {
                Theme::Dark
            };
            self.settings.apply_theme(next);
            self.sync_theme_visuals(ctx);
        }
        if sign_out {
            self.sign_out_and_reload_home(ctx);
        }
        widgets_rect
    }

    pub(crate) fn header_nav_history_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> egui::Rect {
        let can_back = self.ui.can_go_back();
        let can_forward = self.ui.can_go_forward();
        let back_fg = if can_back {
            palette.header_button_fg
        } else {
            palette.neutral_status_fg.gamma_multiply(0.45)
        };
        let fwd_fg = if can_forward {
            palette.header_button_fg
        } else {
            palette.neutral_status_fg.gamma_multiply(0.45)
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let back_btn = egui::Button::new(RichText::new("\u{2190}").size(15.0).color(back_fg))
                .fill(palette.header_button_bg)
                .corner_radius(15);
            let back_resp = ui
                .add_enabled(can_back, back_btn.min_size(egui::vec2(30.0, 30.0)))
                .on_hover_cursor(if can_back {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text("Voltar (Alt+\u{2190})");
            if back_resp.clicked() {
                self.navigate_back(ctx);
            }
            let fwd_btn = egui::Button::new(RichText::new("\u{2192}").size(15.0).color(fwd_fg))
                .fill(palette.header_button_bg)
                .corner_radius(15);
            let fwd_resp = ui
                .add_enabled(can_forward, fwd_btn.min_size(egui::vec2(30.0, 30.0)))
                .on_hover_cursor(if can_forward {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text("Avan\u{00e7}ar (Alt+\u{2192})");
            if fwd_resp.clicked() {
                self.navigate_forward(ctx);
            }
            back_resp.rect.union(fwd_resp.rect)
        })
        .inner
    }

    pub(crate) fn clipboard_quick_open_pill_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let Some(target) = self.ui.clipboard_youtube_target.clone() else {
            return;
        };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(14)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                self.pick("Link copiado", "Copied link"),
                                target.display_label_i18n(self.ui_pt())
                            ))
                            .size(12.5)
                            .color(palette.header_button_fg),
                        );
                        let open_btn = egui::Button::new(
                            RichText::new(self.pick("Abrir", "Open"))
                                .size(12.0)
                                .strong()
                                .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .corner_radius(10);
                        if ui
                            .add(open_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(target.canonical_url())
                            .clicked()
                        {
                            self.open_direct_youtube_target(target.clone(), ctx);
                            self.ui.dismiss_clipboard_target();
                        }
                        let dismiss_btn = egui::Button::new(
                            RichText::new("\u{2715}")
                                .size(12.0)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false);
                        if ui
                            .add(dismiss_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Dispensar sugest\u{00e3}o")
                            .clicked()
                        {
                            self.ui.dismiss_clipboard_target();
                        }
                    });
                });
        });
    }

    pub(crate) fn app_update_banner_pill_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        if self.settings.app_update_banner_dismissed {
            return;
        }
        let Some(check) = self.active_app_update_available().cloned() else {
            return;
        };
        let pt = self.ui_pt();
        let busy = self.settings.app_update_rx.is_some();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(
                    1.0,
                    palette.info_accent_fg.gamma_multiply(0.45),
                ))
                .corner_radius(14)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(
                            RichText::new(if pt {
                                format!(
                                    "Nova versão do FastTube disponível: v{} (atual: v{})",
                                    check.latest_version, check.current_version
                                )
                            } else {
                                format!(
                                    "New FastTube update available: v{} (current: v{})",
                                    check.latest_version, check.current_version
                                )
                            })
                            .size(12.5)
                            .color(palette.header_button_fg),
                        );
                        let update_label = if busy {
                            if pt { "Baixando..." } else { "Updating..." }
                        } else if pt {
                            "Atualizar agora"
                        } else {
                            "Update now"
                        };
                        let update_btn = egui::Button::new(
                            RichText::new(update_label)
                                .size(12.0)
                                .strong()
                                .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .corner_radius(10);
                        if ui
                            .add_enabled(!busy, update_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&check.asset_name)
                            .clicked()
                        {
                            self.start_app_update_download(ctx);
                        }
                        let gh_btn = egui::Button::new(
                            RichText::new("GitHub")
                                .size(12.0)
                                .color(palette.header_button_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(10);
                        if ui
                            .add(gh_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&check.release_url)
                            .clicked()
                        {
                            open_external_url(&check.release_url);
                        }
                        let skip_btn = egui::Button::new(
                            RichText::new(if pt { "Ignorar versão" } else { "Skip" })
                                .size(11.5)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false);
                        if ui
                            .add(skip_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            let mut prefs = self.settings.prefs.value.clone();
                            prefs.skipped_app_update_version = Some(check.latest_version.clone());
                            self.apply_app_preferences(prefs);
                        }
                        let dismiss_btn = egui::Button::new(
                            RichText::new("\u{2715}")
                                .size(12.0)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false);
                        if ui
                            .add(dismiss_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(if pt { "Fechar aviso" } else { "Dismiss notice" })
                            .clicked()
                        {
                            self.settings.app_update_banner_dismissed = true;
                        }
                    });
                });
        });
    }

    pub(crate) fn sign_out_and_reload_home(&mut self, ctx: &egui::Context) {
        self.close_video(ctx);
        let persistence_error = self.set_oauth_session(None);
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        self.feed.home.rx = None;
        self.auth.authenticated = false;
        self.feed.home.title = None;
        self.feed.home.playlist_urls.clear();
        self.feed.home.playlist_total_results = None;
        self.library.active_remote_playlist_title = None;
        self.library.liked_videos.reset();
        self.feed.home.pending_items.clear();
        self.feed.home.cached_items.clear();
        self.feed.home.cached_playlists.clear();
        self.feed.home.cached_loaded = false;
        self.feed.home.page = 0;
        self.feed.home.has_more = true;
        self.feed.home.channel_avatars.clear();
        self.feed.home.subscription_channels.clear();
        self.feed.home.subscription_channels_loaded = false;
        self.feed.home.subscription_channels_error = None;
        self.feed.home.subscriptions_cached_items.clear();
        self.feed.home.subscriptions_cached_loaded = false;
        self.feed.home.subscriptions_cached_at = None;
        self.feed.home.subscriptions_disk_cache_loaded = false;
        self.feed.home.subscriptions_rss_cache.clear();
        self.feed.home.subscriptions_active_request_id = None;
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        if let Some(path) = self.effective_subscriptions_cache_path() {
            let _ = oauth::delete_subscriptions_cache_at_path(&path);
        }
        self.apply_search_results(ctx, Vec::new());
        self.ui.browse_section = BrowseSection::Home;
        self.ui.search_results_mode = false;
        self.feed.filter = FeedFilter::All;
        let logout_error = persistence_error.unwrap_or_default();
        self.start_fast_home_feed(false, ctx);
        if !logout_error.is_empty() {
            self.message = logout_error;
        }
    }

    pub(crate) fn header_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        self.poll_clipboard_youtube_target();
        let palette = self.palette();
        let viewport_width = ui.available_width();
        let layout_mode = compute_header_layout_mode(viewport_width);
        let show_back_button =
            layout_mode == HeaderLayoutMode::TwoRows && self.is_watch_page_active();
        ui.spacing_mut().item_spacing.y = 0.0;
        let drag_origin = ui.cursor().min;
        let header_drag_height = if layout_mode == HeaderLayoutMode::SingleRow {
            56.0
        } else {
            52.0
        };
        let drag_rect =
            egui::Rect::from_min_size(drag_origin, egui::vec2(viewport_width, header_drag_height));
        let titlebar_resp = ui.interact(
            drag_rect,
            egui::Id::new("fasttube_titlebar_drag"),
            egui::Sense::click_and_drag(),
        );
        let mut interactive_rects: Vec<egui::Rect> = Vec::with_capacity(6);
        ui.add_space(8.0);

        match layout_mode {
            HeaderLayoutMode::SingleRow => {
                ui.allocate_ui_with_layout(
                    egui::vec2(viewport_width, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(18.0);
                        interactive_rects.push(self.header_menu_button_ui(ui).rect);
                        ui.add_space(6.0);
                        let brand_resp = self.header_brand_ui(ui, palette);
                        interactive_rects.push(brand_resp.rect);
                        if brand_resp.clicked() {
                            self.handle_header_home(ctx);
                        }
                        ui.add_space(6.0);
                        interactive_rects.push(self.header_nav_history_ui(ui, ctx, palette));

                        let compact_create = viewport_width < 980.0;
                        let window_controls_w = 136.0;
                        let auth_w = if self.auth.authenticated { 38.0 } else { 104.0 };
                        let bell_w = 44.0;
                        let create_w = if compact_create { 42.0 } else { 92.0 };
                        let reserve_right = window_controls_w + auth_w + bell_w + create_w + 8.0;
                        let (leading_space, search_width) =
                            compute_header_search_layout(ui.available_width(), reserve_right);
                        ui.add_space(leading_space);
                        interactive_rects.push(self.header_search_ui(
                            ui,
                            ctx,
                            palette,
                            search_width,
                        ));
                        if ui.available_width() >= reserve_right + 52.0 {
                            ui.add_space(10.0);
                            interactive_rects.push(self.header_recommendation_ui(ui, ctx, palette));
                        }
                        interactive_rects.push(self.header_actions_ui(
                            ui,
                            ctx,
                            palette,
                            false,
                            compact_create,
                            true,
                            viewport_width,
                        ));
                    },
                );
            }
            HeaderLayoutMode::TwoRows => {
                ui.allocate_ui_with_layout(
                    egui::vec2(viewport_width, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let wide_two_row = viewport_width >= 540.0;
                        ui.add_space(if wide_two_row { 18.0 } else { 10.0 });
                        interactive_rects.push(self.header_menu_button_ui(ui).rect);
                        ui.add_space(if wide_two_row { 6.0 } else { 4.0 });
                        let brand_resp = self.header_brand_ui(ui, palette);
                        interactive_rects.push(brand_resp.rect);
                        if brand_resp.clicked() {
                            self.handle_header_home(ctx);
                        }
                        if viewport_width >= 580.0 {
                            ui.add_space(6.0);
                            interactive_rects.push(self.header_nav_history_ui(ui, ctx, palette));
                        }
                        interactive_rects.push(self.header_actions_ui(
                            ui,
                            ctx,
                            palette,
                            show_back_button,
                            true,
                            viewport_width >= 520.0,
                            viewport_width,
                        ));
                    },
                );
                ui.add_space(4.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(viewport_width, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(6.0);
                        let available = (ui.available_width() - 56.0).max(220.0);
                        let search_width = available.min(640.0);
                        let leftover = (ui.available_width() - search_width - 52.0).max(0.0);
                        ui.add_space(leftover * 0.5);
                        let _ = self.header_search_ui(ui, ctx, palette, search_width);
                        ui.add_space(8.0);
                        let _ = self.header_recommendation_ui(ui, ctx, palette);
                    },
                );
            }
        }

        let (is_maximized, is_fullscreen, content_rect, pointer_pos, press_origin) =
            ctx.input(|input| {
                let vp = input.viewport();
                (
                    vp.maximized.unwrap_or(false),
                    self.playback.fullscreen || vp.fullscreen.unwrap_or(false),
                    input.content_rect(),
                    input
                        .pointer
                        .interact_pos()
                        .or_else(|| input.pointer.hover_pos()),
                    input.pointer.press_origin(),
                )
            });
        let point_hits_interactive = |pos: egui::Pos2| {
            interactive_rects
                .iter()
                .any(|rect| rect.is_positive() && rect.contains(pos))
        };
        let over_interactive = pointer_pos.is_some_and(point_hits_interactive)
            || press_origin.is_some_and(point_hits_interactive);
        let in_top_resize_edge = !is_maximized
            && !is_fullscreen
            && (pointer_pos.is_some_and(|pos| pos.y <= content_rect.top() + 4.0)
                || press_origin.is_some_and(|pos| pos.y <= content_rect.top() + 4.0));
        if !over_interactive && !in_top_resize_edge {
            if titlebar_resp.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
            } else if titlebar_resp.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }

        self.clipboard_quick_open_pill_ui(ui, ctx, palette);
        self.app_update_banner_pill_ui(ui, ctx, palette);
        ui.add_space(8.0);
    }

    pub(crate) fn save_library(&mut self) {
        self.library.revision = self.library.revision.wrapping_add(1);
        let result = self
            .library
            .path
            .as_deref()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "local library path unavailable")
            })
            .and_then(|path| library::save(path, &self.library.data));
        self.library.error = result
            .err()
            .map(|error| format!("Não foi possível salvar a biblioteca: {error}"));
    }

    pub(crate) fn user_data_paths(&self) -> io::Result<Vec<(&'static str, PathBuf)>> {
        let required = |path: &Option<PathBuf>, name| {
            path.clone().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("caminho de dados indisponível: {name}"),
                )
            })
        };
        let playback_path = required(&self.settings.playback.path, "preferências de reprodução")?;
        let prefs_path = self
            .settings
            .prefs
            .path
            .clone()
            .unwrap_or_else(|| playback_path.with_file_name("app-preferences.json"));
        let followed_path = self
            .followed_channels
            .path
            .clone()
            .unwrap_or_else(|| playback_path.with_file_name("followed-channels.json"));
        Ok(vec![
            ("library", required(&self.library.path, "biblioteca")?),
            ("queue", required(&self.queue.path, "fila")?),
            ("history", required(&self.history.path, "histórico")?),
            ("followed", followed_path),
            ("playback", playback_path),
            ("theme", required(&self.settings.theme.path, "tema")?),
            (
                "feed",
                required(&self.feed_preferences.path, "filtros do feed")?,
            ),
            ("buffer", required(&self.settings.buffer.path, "buffer")?),
            ("prefs", prefs_path),
        ])
    }

    pub(crate) fn export_user_data(&mut self) {
        let path_text = self.backup.path_text.trim();
        if path_text.is_empty() {
            self.backup.status = Some("Informe o caminho do arquivo de backup.".into());
            return;
        }
        let path = PathBuf::from(path_text);
        let history_json = serde_json::json!({
            "version": 1,
            "entries": self.history.data.entries().iter().map(|entry| serde_json::json!({
                "video_id": entry.video_id,
                "title": entry.title,
                "channel": entry.channel,
                "position_ms": entry.position_ms,
                "duration_ms": entry.duration_ms,
                "watched_at_ms": entry.watched_at_ms,
            })).collect::<Vec<_>>()
        });
        let backup = backup::Backup {
            library: serde_json::to_value(&self.library.data).unwrap_or_default(),
            queue: self.queue.data.to_backup_value(),
            history: history_json,
            preferences: serde_json::json!({
                "speed_quarters": self.settings.playback.value.speed_quarters,
                "max_height": self.settings.playback.value.max_height,
                "theme": if self.settings.theme.value == Theme::Dark { "dark" } else { "light" },
                "buffer_max_mib": self.settings.buffer.profile.values().max_mib,
                "buffer_back_mib": self.settings.buffer.profile.values().back_mib,
                "buffer_cache_seconds": self.settings.buffer.profile.values().cache_seconds,
                "feed_hide_watched": self.feed_preferences.values.hide_watched,
                "feed_hide_shorts": self.feed_preferences.values.hide_shorts,
                "feed_hide_lives": self.feed_preferences.values.hide_lives,
                "feed_newest_first": self.feed_preferences.values.newest_first,
                "feed_channel": self.feed_preferences.values.channel,
                "app_preferences": self.settings.prefs.value,
                "followed_channels": self.followed_channels.data,
            }),
        };
        self.backup.status = Some(match backup::save(&path, &backup) {
            Ok(()) => "Backup exportado com fila versionada e preferências atuais.".into(),
            Err(error) => format!("Falha ao exportar backup: {error}"),
        });
    }

    pub(crate) fn read_imported_user_data(&self) -> io::Result<ImportedUserData> {
        let path_text = self.backup.path_text.trim();
        if path_text.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "informe o caminho do arquivo de backup",
            ));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(Path::new(path_text))?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let data = backup::decode(&bytes)?;
        for field in [
            "speed_quarters",
            "max_height",
            "theme",
            "buffer_max_mib",
            "buffer_back_mib",
            "buffer_cache_seconds",
        ] {
            if data.preferences.get(field).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "preferências incompletas no backup",
                ));
            }
        }
        let library: library::Library = serde_json::from_value(data.library)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "biblioteca inválida"))?;
        library
            .validate()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "biblioteca inválida"))?;
        let queue = Queue::from_backup_value(data.queue, DEFAULT_QUEUE_LIMIT)?;
        let values = data
            .history
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "histórico ausente"))?;
        let mut history = History::default();
        if values.len() > history::MAX_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "histórico excede o limite",
            ));
        }
        for value in values.iter().rev() {
            let entry = HistoryEntry {
                video_id: value["video_id"].as_str().unwrap_or_default().into(),
                title: value["title"].as_str().unwrap_or_default().into(),
                channel: value["channel"].as_str().unwrap_or_default().into(),
                position_ms: value["position_ms"].as_u64().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "posição inválida")
                })?,
                duration_ms: value["duration_ms"].as_u64(),
                watched_at_ms: value["watched_at_ms"].as_u64(),
            };
            history.record(entry).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "entrada de histórico inválida")
            })?;
        }
        let speed = data.preferences["speed_quarters"]
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(4);
        let height = data.preferences["max_height"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok());
        let playback_preferences = PlaybackPreferences {
            speed_quarters: speed,
            max_height: height,
        };
        let buffer_profile = BufferProfile::Custom(BufferSettings {
            max_mib: data.preferences["buffer_max_mib"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(64),
            back_mib: data.preferences["buffer_back_mib"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(16),
            cache_seconds: data.preferences["buffer_cache_seconds"]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(30),
        });
        buffer_profile
            .validate()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "buffer inválido"))?;
        let theme = match data.preferences["theme"].as_str() {
            Some("light") => Theme::Light,
            Some("dark") => Theme::Dark,
            _ => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "tema inválido"));
            }
        };
        let feed = feed_prefs::FeedPreferences {
            hide_watched: data.preferences["feed_hide_watched"]
                .as_bool()
                .unwrap_or(false),
            hide_shorts: data.preferences["feed_hide_shorts"]
                .as_bool()
                .unwrap_or(false),
            hide_lives: data.preferences["feed_hide_lives"]
                .as_bool()
                .unwrap_or(false),
            newest_first: data.preferences["feed_newest_first"]
                .as_bool()
                .unwrap_or(false),
            channel: data.preferences["feed_channel"].as_str().map(str::to_owned),
        };
        if feed
            .channel
            .as_ref()
            .is_some_and(|channel| channel.len() > 4096)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "canal de filtro inválido",
            ));
        }
        if !(1..=8).contains(&speed)
            || !matches!(height, None | Some(360 | 480 | 720 | 1080 | 1440 | 2160))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "preferências inválidas",
            ));
        }
        let app_preferences = match data.preferences.get("app_preferences") {
            Some(value) if !value.is_null() => {
                let parsed: settings::AppPreferences = serde_json::from_value(value.clone())
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "preferências do app inválidas")
                    })?;
                let normalized = parsed.normalized();
                normalized
                    .validate()
                    .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
                normalized
            }
            _ => settings::AppPreferences::default(),
        };
        let followed_channels = match data.preferences.get("followed_channels") {
            Some(value) if !value.is_null() => {
                let parsed: followed_channels::FollowedChannels =
                    serde_json::from_value(value.clone()).map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "canais seguidos inválidos no backup",
                        )
                    })?;
                parsed
                    .validate()
                    .map_err(|msg| io::Error::new(io::ErrorKind::InvalidData, msg))?;
                parsed
            }
            _ => followed_channels::FollowedChannels::default(),
        };
        Ok(ImportedUserData {
            library,
            queue,
            history,
            followed_channels,
            playback_preferences,
            theme,
            buffer_profile,
            feed,
            app_preferences,
        })
    }

    pub(crate) fn analyze_import(&mut self) {
        let imported = match self.read_imported_user_data() {
            Ok(imported) => imported,
            Err(error) => {
                self.backup.pending_import = None;
                self.backup.preview = None;
                self.backup.confirm_import = false;
                self.backup.status = Some(format!(
                    "Importação cancelada; dados atuais preservados: {error}"
                ));
                return;
            }
        };
        let (_, library_merge) = self.library.data.merged(&imported.library);
        let (_, history_merge) = self.history.data.merged(&imported.history);
        let (_, followed_merge) = self
            .followed_channels
            .data
            .merged(&imported.followed_channels);
        let queue_space = DEFAULT_QUEUE_LIMIT.saturating_sub(self.queue.data.len());
        let queue_added = imported.queue.len().min(queue_space);
        let queue_skipped = imported.queue.len().saturating_sub(queue_added);
        self.backup.preview = Some(BackupPreview {
            path: PathBuf::from(self.backup.path_text.trim()),
            library_items: imported.library.item_count(),
            favorites: imported.library.favorites.len(),
            watch_later: imported.library.watch_later.len(),
            collections: imported.library.collections.len(),
            bookmarks: imported.library.bookmarks.len(),
            queue_items: imported.queue.len(),
            history_entries: imported.history.entries().len(),
            followed_channels: imported.followed_channels.len(),
            merge_added: library_merge.added
                + history_merge.added
                + queue_added
                + followed_merge.added,
            merge_conflicts: library_merge.conflicts
                + history_merge.conflicts
                + followed_merge.conflicts,
            merge_skipped: library_merge.skipped_for_limit
                + queue_skipped
                + followed_merge.skipped_for_limit,
            history_progress_updates: history_merge.progress_updated,
        });
        self.backup.pending_import = Some(imported);
        self.backup.import_mode = BackupImportMode::Merge;
        self.backup.confirm_import = true;
        self.backup.status =
            Some("Backup validado. Confira a prévia e escolha como aplicar os dados.".into());
    }

    pub(crate) fn merged_imported_user_data(&self, imported: ImportedUserData) -> ImportedUserData {
        let (library, _) = self.library.data.merged(&imported.library);
        let (history, _) = self.history.data.merged(&imported.history);
        let (followed_channels, _) = self
            .followed_channels
            .data
            .merged(&imported.followed_channels);
        let mut queue = Queue::new(DEFAULT_QUEUE_LIMIT);
        for video in self
            .queue
            .data
            .videos()
            .iter()
            .chain(imported.queue.videos())
            .cloned()
        {
            if queue.add(video).is_err() {
                break;
            }
        }
        ImportedUserData {
            library,
            queue,
            history,
            followed_channels,
            ..imported
        }
    }

    pub(crate) fn import_user_data(&mut self) {
        let Some(imported) = self.backup.pending_import.clone() else {
            self.analyze_import();
            return;
        };
        let target = match self.backup.import_mode {
            BackupImportMode::Replace => imported,
            BackupImportMode::Merge => self.merged_imported_user_data(imported),
        };
        let paths = match self.user_data_paths() {
            Ok(paths) => paths,
            Err(error) => {
                self.backup.status = Some(format!(
                    "Importação cancelada; dados atuais preservados: {error}"
                ));
                return;
            }
        };
        let Some(root) = paths.first().and_then(|(_, path)| path.parent()) else {
            self.backup.status =
                Some("Importação cancelada; a pasta de dados local não foi localizada.".into());
            return;
        };
        let refs: Vec<_> = paths
            .iter()
            .map(|(name, path)| (*name, path.as_path()))
            .collect();
        let mut restore_point = match backup::ImportRestorePoint::create(root, &refs) {
            Ok(point) => point,
            Err(error) => {
                self.backup.status = Some(format!(
                    "Importação cancelada; não foi possível criar o ponto de restauração: {error}"
                ));
                return;
            }
        };
        let persist = (|| -> io::Result<()> {
            let path = |name| {
                paths
                    .iter()
                    .find(|(candidate, _)| *candidate == name)
                    .map(|(_, path)| path.as_path())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name))
            };
            library::save(path("library")?, &target.library)?;
            target.queue.save(path("queue")?)?;
            history::save(path("history")?, &target.history)?;
            followed_channels::save(path("followed")?, &target.followed_channels)?;
            settings::save_playback(path("playback")?, target.playback_preferences)?;
            settings::save_theme(path("theme")?, target.theme)?;
            feed_prefs::save(path("feed")?, &target.feed)?;
            settings::save(path("buffer")?, target.buffer_profile)?;
            settings::save_app_preferences(path("prefs")?, &target.app_preferences)?;
            Ok(())
        })();
        if let Err(error) = persist {
            let rollback = restore_point.restore();
            self.backup.status = Some(match rollback {
                Ok(true) => format!(
                    "Importação falhou ({error}); o ponto de restauração desfez as alterações parciais."
                ),
                Ok(false) => {
                    format!("Importação falhou ({error}); nenhum arquivo precisou ser restaurado.")
                }
                Err(restore_error) => format!(
                    "Importação falhou ({error}); a restauração automática também falhou: {restore_error}. O ponto será tentado novamente ao abrir o app."
                ),
            });
            return;
        }
        if let Err(error) = restore_point.commit() {
            self.backup.status = Some(format!(
                "Os arquivos foram gravados, mas não foi possível remover o ponto de restauração: {error}. A importação será desfeita na próxima abertura."
            ));
            return;
        }
        self.library.data = target.library;
        self.library.revision = self.library.revision.wrapping_add(1);
        self.queue.data = target.queue;
        self.history.data = target.history;
        self.followed_channels.data = target.followed_channels;
        self.followed_channels.error = None;
        self.settings.playback.value = target.playback_preferences;
        self.settings.theme.value = target.theme;
        self.settings.applied_theme = None;
        self.settings.buffer.profile = target.buffer_profile;
        self.settings.prefs.value = target.app_preferences;
        self.settings.prefs.error = None;
        self.settings.cache_dir_input_synced = false;
        self.sync_yt_dlp_cache_dir();
        self.sync_disk_image_cache();
        if self.selected.is_none() {
            self.playback.autoplay = self.settings.prefs.value.default_autoplay;
            self.playback.loop_video = self.settings.prefs.value.default_loop;
            self.reset_decoding_diagnostics();
        }
        self.feed_preferences.values.hide_watched = target.feed.hide_watched;
        self.feed_preferences.values.hide_shorts = target.feed.hide_shorts;
        self.feed_preferences.values.hide_lives = target.feed.hide_lives;
        self.feed_preferences.values.newest_first = target.feed.newest_first;
        self.feed_preferences.values.channel = target.feed.channel;
        self.cancel_pending_queue_video();
        self.invalidate_feed_cache();
        self.backup.pending_import = None;
        self.backup.preview = None;
        self.backup.confirm_import = false;
        self.backup.status = Some(match self.backup.import_mode {
            BackupImportMode::Replace => {
                "Backup aplicado por substituição; os dados anteriores ficaram no ponto de restauração até a conclusão.".into()
            }
            BackupImportMode::Merge => {
                "Backup mesclado: itens ausentes foram adicionados, a biblioteca atual prevaleceu em conflitos e o histórico manteve o maior progresso.".into()
            }
        });
    }
}

impl eframe::App for FastTube {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.watch.extras.sync_video(None);
        self.watch.live.reset_for_video(None, None);
        if let Some(cancel) = self.feed.search.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feed.search_suggest.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.channels.videos.cancel.take() {
            cancel.cancel();
        }
        self.cancel_channel_subrequests();
        if let Some(cancel) = self.playback.quality_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feed.home.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feed.home.batch_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.feed.home.channels_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.library.liked_videos.cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.reauth_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.rating_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.sub_check_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.sub_toggle_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.my_channel_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.remote_playlists_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.playlist_action_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.auth.report_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.followed_channels.feed_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.queue.remote_playlist_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.watch.community_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.settings.ytdlp_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.settings.app_update_cancel.take() {
            cancel.cancel();
        }
        self.downloads.manager.cancel_all();
        self.followed_channels.feed_rx = None;
        self.queue.remote_playlist_rx = None;
        self.watch.community_rx = None;
        self.settings.ytdlp_rx = None;
        self.settings.app_update_rx = None;
        self.auth.reauth_rx = None;
        self.auth.rating_rx = None;
        self.auth.sub_check_rx = None;
        self.auth.sub_toggle_rx = None;
        self.auth.my_channel_rx = None;
        self.auth.remote_playlists_rx = None;
        self.auth.playlist_action_rx = None;
        self.auth.report_rx = None;
        let now = Instant::now();
        #[cfg(windows)]
        {
            self.remember_current_player_volume();
            self.sample_player_before_transition(now);
        }
        self.flush_playback_history_at(now);
        self.desktop.release_keep_awake();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.preprocess_raw_input(raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_window_tray_lifecycle(&ctx);
        self.maybe_auto_check_app_updates(&ctx);
        self.process_desktop_integrations(&ctx);
        self.watch
            .extras
            .sync_video(self.selected.as_ref().map(|video| video.url.as_str()));
        if let Some(status) = self.watch.extras.poll_with_session(
            &mut self.auth.youtube_web_session,
            &mut self.auth.youtube_web_session_checked,
        ) {
            self.message = status;
        }
        self.apply_ready_subtitle();
        if let Some(err) = self.watch.extras.subtitles_error.as_deref()
            && self.message != err
        {
            self.message = err.to_owned();
        }
        self.poll_async_tasks(&ctx);
        self.sync_fullscreen_viewport(&ctx);
        self.sync_mini_player_viewport(&ctx);
        let initial_viewport_width = ui.available_width();
        if !self.ui.initial_sidebar_synced {
            self.ui.initial_sidebar_synced = true;
            if !self.is_watch_page_active()
                && initial_viewport_width >= layout::PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH
            {
                self.ui.navigation_open = true;
            }
        }
        if self.auth.authenticated && !self.auth.restore_attempted {
            self.auth.restore_attempted = true;
            if !self.feed.home.subscription_channels_loaded {
                self.start_subscription_channels_fetch(&ctx);
            }
            if self.auth.my_channel_avatar_url.is_none() && !self.auth.my_channel_fetch_attempted {
                self.start_fetch_my_channel(false, &ctx);
            }
            if !self.feed.home.cached_loaded {
                self.start_fast_home_feed(false, &ctx);
            }
        } else if !self.auth.authenticated && !self.feed.home.initial_public_started {
            self.feed.home.initial_public_started = true;
            if !self.feed.home.cached_loaded {
                self.start_fast_home_feed(false, &ctx);
            }
        }
        self.in_app_ui_pass = true;
        self.sync_theme_visuals(&ctx);
        if !self.autoscroll.is_active() {
            let _ = self.handle_frameless_window_resize(&ctx);
        }
        self.scrolling.apply(&ctx);
        self.apply_middle_autoscroll(&ctx);
        self.thumbnails.maintain(&ctx);
        self.avatars.maintain(&ctx);
        #[cfg(windows)]
        if self.selected.is_some() {
            let eof = self
                .player
                .as_ref()
                .is_some_and(|player| player.flag_cstr(c"eof-reached") == Some(true));
            if !eof {
                self.queue.eof_handled = false;
                self.playback.autoplay_eof_handled = false;
            }
            let ended = eof && self.queue_auto_advance_ready(true);
            if eof
                && self.queue.repeat_mode == queue::RepeatMode::One
                && !self.playback.loop_video
                && !self.active_video_is_live()
            {
                if !self.queue.eof_handled {
                    self.queue.eof_handled = true;
                    if let Some(player) = &self.player {
                        let _ = player
                            .seek(0.0, "absolute+exact")
                            .and_then(|()| player.set("pause", "no"));
                    }
                    self.record_manual_seek(0.0, None, Instant::now());
                }
            } else if queue::should_advance(
                ended,
                &mut self.queue.eof_handled,
                !self.queue.data.is_empty(),
            ) {
                let readd = (self.queue.repeat_mode == queue::RepeatMode::All)
                    .then(|| self.selected.clone())
                    .flatten();
                let keep_docked = self.playback.in_app_mini_player;
                if keep_docked {
                    self.playback.pending_open_keep_mini_player = true;
                    self.playback.pending_open_suppress_back_stack = false;
                }
                if self.advance_queue_with_frame(frame, &ctx).is_some() {
                    if let Some(prev) = readd {
                        let _ = self.queue.data.add(prev);
                        self.persist_queue();
                    }
                    if keep_docked {
                        self.playback.in_app_mini_player = true;
                        self.sync_player_window_visibility();
                    }
                } else if keep_docked {
                    self.playback.pending_open_keep_mini_player = false;
                }
            } else if self.queue.data.is_empty()
                && !self.playback.loop_video
                && !self.active_video_is_live()
            {
                let has_live = self
                    .playback
                    .active
                    .as_ref()
                    .is_some_and(|active| active.has_live_sample);
                if self.queue.repeat_mode == queue::RepeatMode::All {
                    if eof && !self.playback.autoplay_eof_handled {
                        self.playback.autoplay_eof_handled = true;
                        if let Some(player) = &self.player {
                            let _ = player
                                .seek(0.0, "absolute+exact")
                                .and_then(|()| player.set("pause", "no"));
                        }
                        self.record_manual_seek(0.0, None, Instant::now());
                    }
                } else if self.playback.autoplay
                    && has_live
                    && eof
                    && !self.playback.autoplay_eof_handled
                {
                    self.playback.autoplay_eof_handled = true;
                    let next_video = if self.is_watch_short_mode() {
                        self.next_or_prev_short(true)
                    } else {
                        self.next_recommended_video()
                    };
                    if let Some(video) = next_video {
                        self.playback.pending_open_keep_mini_player =
                            self.playback.in_app_mini_player;
                        self.playback.pending_open_suppress_back_stack = false;
                        self.playback.pending_open_video = Some(video);
                    }
                }
            }
        }
        let escape_pressed = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let toggled_palette_by_shortcut = self.handle_command_palette_global_shortcut(&ctx);
        let closed_command_palette =
            escape_pressed && !toggled_palette_by_shortcut && self.ui.command_palette.open;
        if closed_command_palette {
            self.ui.command_palette.close_palette();
        }
        let closed_search_modal = if escape_pressed && !closed_command_palette {
            if self.channels.about_modal_open {
                self.channels.about_modal_open = false;
                true
            } else if self.feed.share_modal_video.is_some() {
                self.feed.share_modal_video = None;
                true
            } else if self.feed.search_report_modal_video.is_some() {
                self.feed.search_report_modal_video = None;
                self.feed.search_report_reason = None;
                true
            } else if self.feed.search_save_modal_video.is_some() {
                self.feed.search_save_modal_video = None;
                true
            } else if self.feed.search_filters_open {
                self.feed.search_filters_open = false;
                true
            } else {
                false
            }
        } else {
            false
        };
        let closed_settings = escape_pressed
            && !closed_command_palette
            && !closed_search_modal
            && self.ui.settings_open;
        if closed_settings {
            self.ui.settings_open = false;
            if self.selected.is_none() {
                self.reset_decoding_diagnostics();
            }
            self.sync_player_window_visibility();
        }
        let closed_context_menu = escape_pressed
            && !closed_command_palette
            && !closed_search_modal
            && !closed_settings
            && self.playback.player_context_menu.take().is_some();
        let closed_player_menu = escape_pressed
            && !closed_command_palette
            && !closed_search_modal
            && !closed_settings
            && !closed_context_menu
            && self.playback.player_menu.is_some();
        if closed_player_menu {
            self.playback.player_menu = None;
        }
        let had_shortcuts_open = self.ui.shortcuts_open;
        if !closed_command_palette {
            self.handle_help_shortcuts(&ctx);
        }
        let closed_shortcuts = escape_pressed && had_shortcuts_open && !self.ui.shortcuts_open;
        let blocked_by_palette = self.ui.command_palette.open || closed_command_palette;
        if !blocked_by_palette && !ctx.egui_wants_keyboard_input() {
            let (nav_back, nav_forward, home, shorts, subscriptions, library, history, downloads) =
                ctx.input(|input| {
                    let alt = input.modifiers.alt && !input.modifiers.ctrl;
                    (
                        (alt && input.key_pressed(egui::Key::ArrowLeft))
                            || input.pointer.button_pressed(egui::PointerButton::Extra1),
                        (alt && input.key_pressed(egui::Key::ArrowRight))
                            || input.pointer.button_pressed(egui::PointerButton::Extra2),
                        alt && input.key_pressed(egui::Key::Num1),
                        alt && input.key_pressed(egui::Key::Num2),
                        alt && input.key_pressed(egui::Key::Num3),
                        alt && input.key_pressed(egui::Key::Num4),
                        alt && input.key_pressed(egui::Key::Num5),
                        alt && input.key_pressed(egui::Key::Num6),
                    )
                });
            if nav_back {
                self.navigate_back(&ctx);
            } else if nav_forward {
                self.navigate_forward(&ctx);
            } else if home {
                self.navigate_from_shortcut(&ctx, BrowseSection::Home);
            } else if shorts {
                self.navigate_from_shortcut(&ctx, BrowseSection::Shorts);
            } else if subscriptions {
                self.navigate_from_shortcut(&ctx, BrowseSection::Subscriptions);
            } else if library {
                self.navigate_from_shortcut(&ctx, BrowseSection::Library);
            } else if history {
                self.navigate_from_shortcut(&ctx, BrowseSection::History);
            } else if downloads {
                self.navigate_from_shortcut(&ctx, BrowseSection::Downloads);
            }
        } else if !blocked_by_palette {
            let (mouse_back, mouse_forward) = ctx.input(|input| {
                (
                    input.pointer.button_pressed(egui::PointerButton::Extra1),
                    input.pointer.button_pressed(egui::PointerButton::Extra2),
                )
            });
            if mouse_back {
                self.navigate_back(&ctx);
            } else if mouse_forward {
                self.navigate_forward(&ctx);
            }
        }
        self.handle_player_shortcuts(
            &ctx,
            !blocked_by_palette
                && !closed_search_modal
                && !closed_settings
                && !closed_context_menu
                && !closed_player_menu
                && !closed_shortcuts,
        );
        let viewport_width = ui.available_width();
        let chromeless_video =
            self.playback.fullscreen || (self.playback.mini_player && self.is_watch_page_active());
        if escape_pressed
            && !closed_search_modal
            && !closed_settings
            && !closed_context_menu
            && !closed_player_menu
            && !closed_shortcuts
            && !self.playback.fullscreen
            && should_show_navigation_drawer(
                viewport_width,
                self.ui.navigation_open,
                chromeless_video,
                self.is_watch_page_active(),
            )
        {
            self.ui.navigation_open = false;
        }
        if !chromeless_video {
            let flush_panel_frame = egui::Frame::new()
                .fill(ctx.global_style().visuals.panel_fill)
                .inner_margin(egui::Margin::ZERO);
            egui::Panel::top("header")
                .default_size(56.0)
                .show_separator_line(false)
                .frame(flush_panel_frame)
                .show(ui, |ui| {
                    self.header_ui(ui, &ctx);
                });
            let has_selected_video = self.is_watch_page_active();
            if should_show_persistent_navigation_panel(
                viewport_width,
                self.ui.navigation_open,
                chromeless_video,
                has_selected_video,
                self.playback.theater,
            ) {
                let nav_width = layout::compute_youtube_sidebar_width(viewport_width);
                egui::Panel::left("navigation")
                    .resizable(false)
                    .show_separator_line(false)
                    .exact_size(nav_width)
                    .frame(flush_panel_frame)
                    .show(ui, |ui| {
                        self.navigation_ui(ui, &ctx);
                    });
            } else if should_show_compact_navigation_for_page(
                viewport_width,
                self.ui.navigation_open,
                chromeless_video,
                has_selected_video,
                self.playback.theater,
            ) {
                egui::Panel::left("compact_navigation")
                    .resizable(false)
                    .show_separator_line(false)
                    .exact_size(COMPACT_NAVIGATION_WIDTH)
                    .frame(flush_panel_frame)
                    .show(ui, |ui| {
                        self.compact_navigation_ui(ui, &ctx);
                    });
            }
        }
        let mut central_panel = egui::CentralPanel::default();
        if chromeless_video {
            central_panel = central_panel.frame(egui::Frame::NONE.fill(Color32::BLACK));
        }
        central_panel.show(ui, |ui| {
            if chromeless_video {
                let avail_w = ui.available_width();
                let avail_h = ui.available_height();
                self.watch_primary_ui(ui, &ctx, avail_w, avail_h, WatchLayoutMode::SideBySide);
                if self.queue.pending_advance {
                    self.queue.pending_advance = false;
                    self.queue.pending_play_entry_id = None;
                    let _ = self.advance_queue_with_frame(frame, &ctx);
                } else if let Some(entry_id) = self.queue.pending_play_entry_id.take() {
                    let _ = self.play_queue_entry_with_frame(entry_id, frame, &ctx);
                } else if let Some(video) = self.playback.pending_open_video.take() {
                    let _ = self.open_video(video, frame, &ctx);
                }
                if is_actionable_inline_message(&self.message) && !self.playback.mini_player {
                    ui.add_space(6.0);
                    let palette = self.palette();
                    if inline_error_banner_ui(ui, &self.message, &palette) {
                        self.message.clear();
                    }
                }
                return;
            }

            if self.ui.settings_open {
                self.sync_player_window_visibility();
                #[cfg(windows)]
                if let Some(player) = &mut self.player {
                    let (events, poll_error) = player.poll_events();
                    for event in events {
                        self.handle_queue_player_event(event);
                    }
                    if let Some(error) = poll_error {
                        self.message = error;
                        self.report_pending_queue_load_error();
                    }
                }
                self.settings_page_ui(ui, &ctx);
                if self.settings.pending_reload_video {
                    self.settings.pending_reload_video = false;
                    if let Some(video) = self.selected.clone() {
                        let now = Instant::now();
                        #[cfg(windows)]
                        {
                            self.remember_current_player_volume();
                            self.sample_player_before_transition(now);
                        }
                        let saved_pos_ms = self.current_player_live_pos_ms();
                        let was_live = self.active_video_is_live();
                        self.flush_playback_history_at(now);
                        #[cfg(windows)]
                        {
                            if let Some(player) = &mut self.player {
                                player.set_visible(false);
                            }
                            self.player = None;
                        }
                        self.playback.active_buffer = None;
                        self.playback.active_hwdec = None;
                        if self.open_video(video, frame, &ctx)
                            && !was_live
                            && let Some(pos_ms) = saved_pos_ms
                            && pos_ms > 1_000
                            && let Some(active) = &mut self.playback.active
                        {
                            active.resume_target_ms = Some(pos_ms);
                            active.resume_seek_pending = true;
                            active.start_option_cleared = false;
                            active.position_ms = pos_ms;
                        }
                    }
                }
                if self.queue.pending_advance {
                    self.queue.pending_advance = false;
                    self.queue.pending_play_entry_id = None;
                    let _ = self.advance_queue_with_frame(frame, &ctx);
                } else if let Some(entry_id) = self.queue.pending_play_entry_id.take() {
                    let _ = self.play_queue_entry_with_frame(entry_id, frame, &ctx);
                } else if let Some(video) = self.playback.pending_open_video.take() {
                    let _ = self.open_video(video, frame, &ctx);
                }
                return;
            }

            let chosen = if self.is_watch_page_active() {
                self.watch_page_ui(ui, &ctx)
            } else {
                self.browse_page_ui(ui, &ctx)
            };

            if let Some(video) = chosen {
                self.playback.pending_open_keep_mini_player = false;
                self.playback.pending_open_suppress_back_stack = false;
                if self.library.pending_bookmark_seek.is_none()
                    && self
                        .selected
                        .as_ref()
                        .is_some_and(|current| Self::same_video_target(&current.url, &video.url))
                {
                    self.exit_in_app_mini_player(&ctx);
                } else {
                    let _ = self.open_video(video, frame, &ctx);
                }
            }
            if self.queue.pending_advance {
                self.queue.pending_advance = false;
                self.queue.pending_play_entry_id = None;
                let keep_docked = self.playback.in_app_mini_player;
                if keep_docked {
                    self.playback.pending_open_keep_mini_player = true;
                    self.playback.pending_open_suppress_back_stack = false;
                }
                if self.advance_queue_with_frame(frame, &ctx).is_some() && keep_docked {
                    self.playback.in_app_mini_player = true;
                    self.sync_player_window_visibility();
                } else if keep_docked {
                    self.playback.pending_open_keep_mini_player = false;
                }
            } else if let Some(entry_id) = self.queue.pending_play_entry_id.take() {
                let _ = self.play_queue_entry_with_frame(entry_id, frame, &ctx);
            } else if let Some(video) = self.playback.pending_open_video.take() {
                let _ = self.open_video(video, frame, &ctx);
            }
        });
        if !chromeless_video && self.is_in_app_mini_player_active() {
            self.in_app_mini_player_ui(&ctx);
            if self.queue.pending_advance {
                self.queue.pending_advance = false;
                self.queue.pending_play_entry_id = None;
                let keep_docked = self.playback.in_app_mini_player;
                if keep_docked {
                    self.playback.pending_open_keep_mini_player = true;
                    self.playback.pending_open_suppress_back_stack = false;
                }
                if self.advance_queue_with_frame(frame, &ctx).is_some() && keep_docked {
                    self.playback.in_app_mini_player = true;
                    self.sync_player_window_visibility();
                } else if keep_docked {
                    self.playback.pending_open_keep_mini_player = false;
                }
            } else if let Some(video) = self.playback.pending_open_video.take() {
                if self.playback.in_app_mini_player {
                    self.playback.pending_open_keep_mini_player = true;
                }
                let _ = self.open_video(video, frame, &ctx);
            }
        }
        if should_show_navigation_drawer(
            viewport_width,
            self.ui.navigation_open,
            chromeless_video,
            self.is_watch_page_active(),
        ) {
            self.watch_navigation_drawer_ui(&ctx);
        }
        self.in_app_ui_pass = false;
        self.shortcuts_window_ui(&ctx);
        self.search_filters_modal_ui(&ctx);
        self.search_save_modal_ui(&ctx);
        self.search_report_modal_ui(&ctx);
        self.share_modal_ui(&ctx);
        self.channel_about_modal_ui(&ctx);
        self.command_palette_ui(&ctx);
        self.sync_navigation_history();
        let palette = self.palette();
        self.toasts.show(&ctx, &palette);
        let (is_maximized, is_fullscreen, content_rect) = ctx.input(|input| {
            let vp = input.viewport();
            (
                vp.maximized.unwrap_or(false),
                self.playback.fullscreen || vp.fullscreen.unwrap_or(false),
                input.content_rect(),
            )
        });
        if !is_maximized && !is_fullscreen {
            let border_color = if self.settings.theme.value == Theme::Dark {
                Color32::from_rgb(48, 48, 52)
            } else {
                Color32::from_rgb(200, 200, 206)
            };
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("frameless_window_border"),
            ))
            .rect_stroke(
                content_rect,
                0.0,
                egui::Stroke::new(1.0, border_color),
                egui::StrokeKind::Inside,
            );
        }
        self.paint_middle_autoscroll_overlay(&ctx);
        self.schedule_idle_aware_repaint(&ctx);
    }
}

impl FastTube {
    pub(crate) fn same_video_target(a: &str, b: &str) -> bool {
        match (history::youtube_video_id(a), history::youtube_video_id(b)) {
            (Some(id_a), Some(id_b)) => id_a == id_b,
            _ => a == b,
        }
    }

    pub(crate) fn find_known_video_by_id(&self, video_id: &str, fallback_title: &str) -> Video {
        let canonical_url = history::youtube_watch_url(video_id)
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={video_id}"));
        let matches_id = |video: &Video| {
            history::youtube_video_id(&video.url).as_deref() == Some(video_id)
                || video.url == canonical_url
        };
        if let Some(video) = self.selected.as_ref().filter(|v| matches_id(v)) {
            return video.clone();
        }
        if let Some(video) = self.feed.videos.iter().find(|v| matches_id(v)) {
            return video.clone();
        }
        if let Some(video) = self.queue.data.videos().iter().find(|v| matches_id(v)) {
            return video.clone();
        }
        if let Some(record) = self
            .library
            .data
            .watch_later
            .iter()
            .find(|r| matches_id(&r.video))
        {
            return record.video.clone();
        }
        if let Some(record) = self
            .library
            .data
            .favorites
            .iter()
            .find(|r| matches_id(&r.video))
        {
            return record.video.clone();
        }
        for collection in &self.library.data.collections {
            if let Some(record) = collection.videos.iter().find(|r| matches_id(&r.video)) {
                return record.video.clone();
            }
        }
        if let Some(entry) = self.history.data.get(video_id) {
            return Video {
                title: entry.title.clone(),
                channel: entry.channel.clone(),
                url: canonical_url,
                thumbnail: String::new(),
                duration: entry
                    .duration_ms
                    .map(command_palette::format_timestamp_ms)
                    .unwrap_or_default(),
            };
        }
        let title = if fallback_title.trim().is_empty() {
            video_id.to_owned()
        } else {
            fallback_title.to_owned()
        };
        Video {
            title,
            channel: String::new(),
            url: canonical_url,
            thumbnail: String::new(),
            duration: String::new(),
        }
    }

    pub(crate) fn open_video_at_timestamp(
        &mut self,
        video: Video,
        seek_ms: Option<u64>,
        ctx: &egui::Context,
    ) {
        self.ui.settings_open = false;
        self.close_channel_view();
        self.playback.pending_open_keep_mini_player = false;
        self.playback.pending_open_suppress_back_stack = false;
        if self
            .selected
            .as_ref()
            .is_some_and(|current| Self::same_video_target(&current.url, &video.url))
        {
            self.playback.pending_open_video = None;
            self.playback.pending_open_seek_ms = None;
            self.playback.in_app_mini_player = false;
            if let Some(ms) = seek_ms {
                self.seek_to_seconds(ms as f64 / 1000.0);
            }
            self.sync_player_window_visibility();
            ctx.request_repaint();
            return;
        }
        self.playback.in_app_mini_player = false;
        self.playback.pending_open_seek_ms = seek_ms;
        self.playback.pending_open_video = Some(video);
        ctx.request_repaint();
    }

    pub(crate) fn open_direct_youtube_target(
        &mut self,
        target: command_palette::DirectYouTubeTarget,
        ctx: &egui::Context,
    ) {
        match target {
            command_palette::DirectYouTubeTarget::Video {
                url,
                video_id,
                start_ms,
            } => {
                let mut video = self.find_known_video_by_id(&video_id, &url);
                if video.url.is_empty() {
                    video.url = url;
                }
                self.open_video_at_timestamp(video, start_ms, ctx);
            }
            command_palette::DirectYouTubeTarget::Channel(channel_target) => {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.queue.remote_playlist_id = None;
                self.ui.browse_section = BrowseSection::Home;
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.open_channel(channel_target, ctx);
            }
            command_palette::DirectYouTubeTarget::Playlist { url, list_id } => {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Home;
                self.feed.filter = FeedFilter::All;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.start_remote_playlist_fetch(&list_id, &url, ctx);
            }
        }
    }

    pub(crate) fn set_clipboard_preview_text(&mut self, raw: Option<&str>) {
        self.ui.clipboard_last_checked_at = Some(Instant::now());
        let normalized = raw
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        self.ui.clipboard_last_raw = normalized.clone();
        if let Some(current) = normalized {
            if self.ui.clipboard_dismissed_raw.as_deref() == Some(current.as_str()) {
                self.ui.clipboard_youtube_target = None;
            } else {
                self.ui.clipboard_dismissed_raw = None;
                self.ui.clipboard_youtube_target =
                    command_palette::parse_direct_youtube_url(&current);
            }
        } else {
            self.ui.clipboard_dismissed_raw = None;
            self.ui.clipboard_youtube_target = None;
        }
    }

    pub(crate) fn poll_clipboard_youtube_target(&mut self) {
        #[cfg(not(test))]
        {
            if self.desktop.hidden_to_tray {
                return;
            }
            if self
                .ui
                .clipboard_last_checked_at
                .is_some_and(|at| at.elapsed() < Duration::from_millis(750))
            {
                return;
            }
            let raw = read_text_from_os_clipboard();
            let trimmed = raw.as_deref().map(str::trim).filter(|s| !s.is_empty());
            if trimmed == self.ui.clipboard_last_raw.as_deref() {
                self.ui.clipboard_last_checked_at = Some(Instant::now());
                return;
            }
            self.set_clipboard_preview_text(raw.as_deref());
        }
    }

    pub(crate) fn current_navigation_route(&self) -> app_state::NavigationRoute {
        if self.ui.settings_open {
            app_state::NavigationRoute::Settings(self.settings.active_tab)
        } else if self.is_watch_page_active()
            && let Some(video) = &self.selected
        {
            app_state::NavigationRoute::Watch(video.clone())
        } else if self.channels.view_open
            && let Some(target) = self.current_channel_target()
        {
            app_state::NavigationRoute::Channel(target)
        } else {
            let search_query = (self.ui.search_results_mode && !self.feed.query.trim().is_empty())
                .then(|| self.feed.query.clone());
            app_state::NavigationRoute::Browse {
                section: self.ui.browse_section,
                search_query,
            }
        }
    }

    pub(crate) fn apply_navigation_route(
        &mut self,
        route: app_state::NavigationRoute,
        ctx: &egui::Context,
    ) {
        self.ui.nav_suppress_record = true;
        self.ui.nav_last_route = Some(route.clone());
        match route {
            app_state::NavigationRoute::Settings(tab) => {
                self.settings.active_tab = tab;
                self.ui.settings_open = true;
                self.sync_player_window_visibility();
            }
            app_state::NavigationRoute::Watch(video) => {
                self.ui.settings_open = false;
                self.close_channel_view();
                self.playback.pending_open_keep_mini_player = false;
                self.playback.pending_open_suppress_back_stack = false;
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|current| Self::same_video_target(&current.url, &video.url))
                {
                    self.playback.pending_open_video = None;
                    self.exit_in_app_mini_player(ctx);
                } else {
                    self.playback.pending_open_seek_ms = None;
                    self.playback.pending_open_video = Some(video);
                    ctx.request_repaint();
                }
            }
            app_state::NavigationRoute::Channel(target) => {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.queue.remote_playlist_id = None;
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                if self.channels.last_target.as_ref() == Some(&target)
                    && self.channels.page.is_some()
                {
                    self.channels.view_open = true;
                    self.sync_player_window_visibility();
                } else {
                    self.open_channel(target, ctx);
                }
            }
            app_state::NavigationRoute::Browse {
                section,
                search_query,
            } => {
                self.ui.settings_open = false;
                self.queue.remote_playlist_id = None;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                if let Some(query) = search_query {
                    self.ui.search_results_mode = true;
                    self.ui.browse_section = BrowseSection::Home;
                    self.feed.filter = FeedFilter::All;
                    if self.feed.query != query || self.feed.videos.is_empty() {
                        self.feed.query = query;
                        self.start_search(ctx);
                    }
                } else {
                    self.ui.search_results_mode = false;
                    match section {
                        BrowseSection::Home => self.handle_header_home(ctx),
                        BrowseSection::Subscriptions => self.open_subscriptions_section(false, ctx),
                        BrowseSection::Shorts => {
                            self.ui.browse_section = BrowseSection::Shorts;
                            self.feed.filter = FeedFilter::Shorts;
                            if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
                                let _ = self.restore_cached_home_feed(ctx);
                            }
                            self.invalidate_feed_cache();
                            self.ensure_feed_cache();
                        }
                        other => {
                            self.ui.browse_section = other;
                            if matches!(self.feed.filter, FeedFilter::Shorts) {
                                self.feed.filter = FeedFilter::All;
                            }
                            self.invalidate_feed_cache();
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn sync_navigation_history(&mut self) {
        if self.playback.pending_open_video.is_some() {
            return;
        }
        let current = self.current_navigation_route();
        if self.ui.nav_suppress_record {
            self.ui.nav_suppress_record = false;
            self.ui.nav_last_route = Some(current);
            return;
        }
        match &self.ui.nav_last_route {
            None => {
                self.ui.nav_last_route = Some(current);
            }
            Some(previous) if *previous == current => {}
            Some(previous) => {
                self.ui.nav_back_stack.push(previous.clone());
                if self.ui.nav_back_stack.len() > 64 {
                    self.ui.nav_back_stack.remove(0);
                }
                self.ui.nav_forward_stack.clear();
                self.ui.nav_last_route = Some(current);
            }
        }
    }

    pub(crate) fn navigate_back(&mut self, ctx: &egui::Context) -> bool {
        let Some(target) = self.ui.nav_back_stack.pop() else {
            return false;
        };
        let current = self
            .ui
            .nav_last_route
            .clone()
            .unwrap_or_else(|| self.current_navigation_route());
        if current != target {
            self.ui.nav_forward_stack.push(current);
            if self.ui.nav_forward_stack.len() > 64 {
                self.ui.nav_forward_stack.remove(0);
            }
        }
        self.apply_navigation_route(target, ctx);
        true
    }

    pub(crate) fn navigate_forward(&mut self, ctx: &egui::Context) -> bool {
        let Some(target) = self.ui.nav_forward_stack.pop() else {
            return false;
        };
        let current = self
            .ui
            .nav_last_route
            .clone()
            .unwrap_or_else(|| self.current_navigation_route());
        if current != target {
            self.ui.nav_back_stack.push(current);
            if self.ui.nav_back_stack.len() > 64 {
                self.ui.nav_back_stack.remove(0);
            }
        }
        self.apply_navigation_route(target, ctx);
        true
    }

    pub(crate) fn build_command_palette_items(&self) -> Vec<command_palette::PaletteItem> {
        use command_palette::{PaletteAction, PaletteCategory, PaletteItem};

        let pt = self.ui_pt();
        let mut items = Vec::new();

        if let Some(target) = &self.ui.clipboard_youtube_target {
            items.push(PaletteItem {
                id: "action:clipboard-open".to_owned(),
                category: PaletteCategory::Action,
                title: format!(
                    "{}: {}",
                    self.pick("Abrir link da área de transferência", "Open clipboard link",),
                    target.display_label_i18n(pt)
                ),
                subtitle: target.canonical_url(),
                keywords: format!(
                    "clipboard area de transferencia colar url link {}",
                    target.canonical_url()
                ),
                shortcut: None,
                action: PaletteAction::OpenDirectTarget(target.clone()),
            });
        }

        items.push(PaletteItem {
            id: "action:toggle-theme".to_owned(),
            category: PaletteCategory::Action,
            title: if self.settings.theme.value == Theme::Dark {
                self.pick("Alternar para tema claro", "Switch to light theme")
                    .to_owned()
            } else {
                self.pick("Alternar para tema escuro", "Switch to dark theme")
                    .to_owned()
            },
            subtitle: self
                .pick(
                    "Mudar a aparência geral da interface",
                    "Change the overall UI appearance",
                )
                .to_owned(),
            keywords: "tema claro escuro dark light theme aparencia".to_owned(),
            shortcut: None,
            action: PaletteAction::ToggleTheme,
        });

        items.push(PaletteItem {
            id: "action:toggle-theater".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Alternar modo teatro", "Toggle theater mode")
                .to_owned(),
            subtitle: self
                .pick(
                    "Expandir ou restaurar o player na página de reprodução",
                    "Expand or restore the player on the watch page",
                )
                .to_owned(),
            keywords: "teatro theater layout player".to_owned(),
            shortcut: Some("T"),
            action: PaletteAction::ToggleTheater,
        });

        items.push(PaletteItem {
            id: "action:toggle-miniplayer".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Alternar miniplayer", "Toggle mini-player")
                .to_owned(),
            subtitle: self
                .pick(
                    "Janela compacta sempre visível",
                    "Always-on-top compact window",
                )
                .to_owned(),
            keywords: "miniplayer pip flutuante janela floating".to_owned(),
            shortcut: Some("I"),
            action: PaletteAction::ToggleMiniPlayer,
        });

        items.push(PaletteItem {
            id: "action:cycle-repeat".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Alternar modo de repetição", "Cycle repeat mode")
                .to_owned(),
            subtitle: if self.ui_pt() {
                self.queue.repeat_mode.label_pt().to_owned()
            } else {
                self.queue.repeat_mode.label_en().to_owned()
            },
            keywords: "repetir repeat loop fila modo".to_owned(),
            shortcut: None,
            action: PaletteAction::CycleRepeat,
        });

        if !self.queue.data.is_empty() {
            if self.queue.data.len() > 1 {
                items.push(PaletteItem {
                    id: "action:shuffle-queue".to_owned(),
                    category: PaletteCategory::Action,
                    title: self
                        .pick("Embaralhar fila de reprodução", "Shuffle playback queue")
                        .to_owned(),
                    subtitle: format!(
                        "{} {}",
                        self.queue.data.len(),
                        self.pick("vídeo(s) na fila", "video(s) in queue")
                    ),
                    keywords: "embaralhar shuffle misturar fila queue".to_owned(),
                    shortcut: None,
                    action: PaletteAction::ShuffleQueue,
                });
            }
            items.push(PaletteItem {
                id: "action:clear-queue".to_owned(),
                category: PaletteCategory::Action,
                title: self
                    .pick("Limpar fila de reprodução", "Clear playback queue")
                    .to_owned(),
                subtitle: format!(
                    "{} {}",
                    self.queue.data.len(),
                    self.pick("vídeo(s) na fila", "video(s) in queue")
                ),
                keywords: "limpar fila apagar remover queue".to_owned(),
                shortcut: None,
                action: PaletteAction::ClearQueue,
            });
        }

        items.push(PaletteItem {
            id: "action:shortcuts".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Atalhos e acessibilidade", "Shortcuts and accessibility")
                .to_owned(),
            subtitle: self
                .pick(
                    "Exibir todos os atalhos de teclado",
                    "Show all keyboard shortcuts",
                )
                .to_owned(),
            keywords: "atalhos ajuda teclado f1 acessibilidade shortcuts".to_owned(),
            shortcut: Some("F1"),
            action: PaletteAction::OpenShortcuts,
        });

        for (section, title, subtitle, keywords, shortcut) in [
            (
                BrowseSection::Home,
                self.pick("Início", "Home"),
                self.pick(
                    "Feed principal e recomendados",
                    "Main feed and recommendations",
                ),
                "inicio home feed principal",
                Some("Alt+1"),
            ),
            (
                BrowseSection::Shorts,
                "Shorts",
                self.pick("Vídeos curtos verticais", "Vertical short videos"),
                "shorts curtos verticais",
                Some("Alt+2"),
            ),
            (
                BrowseSection::Subscriptions,
                self.pick("Inscrições", "Subscriptions"),
                self.pick(
                    "Envios recentes dos canais seguidos",
                    "Recent uploads from followed channels",
                ),
                "inscricoes canais seguidos envios subscriptions",
                Some("Alt+3"),
            ),
            (
                BrowseSection::Library,
                self.pick("Você", "You"),
                self.pick(
                    "Histórico, playlists e Assistir mais tarde",
                    "History, playlists and Watch later",
                ),
                "voce biblioteca salvos assistir mais tarde playlists colecoes library you",
                Some("Alt+4"),
            ),
            (
                BrowseSection::History,
                self.pick("Histórico", "History"),
                self.pick(
                    "Vídeos reproduzidos recentemente",
                    "Recently watched videos",
                ),
                "historico recentes assistidos history",
                Some("Alt+5"),
            ),
            (
                BrowseSection::Downloads,
                "Downloads",
                self.pick(
                    "Gerenciador de downloads offline via yt-dlp",
                    "Offline download manager via yt-dlp",
                ),
                "downloads offline baixar mp4 mp3 ytdlp",
                Some("Alt+6"),
            ),
        ] {
            items.push(PaletteItem {
                id: format!("section:{section:?}"),
                category: PaletteCategory::Section,
                title: title.to_owned(),
                subtitle: subtitle.to_owned(),
                keywords: keywords.to_owned(),
                shortcut,
                action: PaletteAction::Browse(section),
            });
        }

        for tab in app_state::SettingsTab::ALL {
            items.push(PaletteItem {
                id: format!("settings:{tab:?}"),
                category: PaletteCategory::Settings,
                title: format!(
                    "{}: {}",
                    self.pick("Configurações", "Settings"),
                    tab.label(pt)
                ),
                subtitle: tab.subtitle(pt).to_owned(),
                keywords: format!(
                    "configuracoes settings ajustes preferencias {} {}",
                    tab.label_pt(),
                    tab.label_en()
                ),
                shortcut: None,
                action: PaletteAction::SettingsTab(tab),
            });
        }

        for (idx, mark) in self.library.data.bookmarks.iter().enumerate() {
            let ts = command_palette::format_timestamp_ms(mark.time_ms);
            let title = if mark.note.trim().is_empty() {
                format!("{ts} \u{2014} {}", mark.title)
            } else {
                format!("{} ({ts})", mark.note)
            };
            items.push(PaletteItem {
                id: format!("bookmark:{idx}:{}", mark.video_id),
                category: PaletteCategory::Bookmark,
                title,
                subtitle: format!("{} \u{2022} {ts}", mark.title),
                keywords: format!(
                    "marcador bookmark {} {} {}",
                    mark.title, mark.note, mark.video_id
                ),
                shortcut: None,
                action: PaletteAction::OpenBookmark {
                    bookmark: mark.clone(),
                },
            });
        }

        for (idx, video) in self.queue.data.videos().iter().enumerate() {
            items.push(PaletteItem {
                id: format!("queue:{idx}:{}", video.url),
                category: PaletteCategory::Queue,
                title: format!("{}. {}", idx + 1, video.title),
                subtitle: video.channel.clone(),
                keywords: format!("fila queue {} {}", video.title, video.channel),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video: video.clone(),
                    seek_ms: None,
                },
            });
        }

        for (idx, record) in self.library.data.watch_later.iter().enumerate() {
            items.push(PaletteItem {
                id: format!("library:wl:{idx}:{}", record.video.url),
                category: PaletteCategory::Library,
                title: record.video.title.clone(),
                subtitle: format!(
                    "{} \u{2022} {}",
                    self.pick("Assistir mais tarde", "Watch later"),
                    record.video.channel
                ),
                keywords: format!(
                    "biblioteca library assistir mais tarde watch later {} {}",
                    record.video.title, record.video.channel
                ),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video: record.video.clone(),
                    seek_ms: None,
                },
            });
        }

        for (idx, record) in self.library.data.favorites.iter().enumerate() {
            items.push(PaletteItem {
                id: format!("library:fav:{idx}:{}", record.video.url),
                category: PaletteCategory::Library,
                title: record.video.title.clone(),
                subtitle: format!(
                    "{} \u{2022} {}",
                    self.pick("Favoritos", "Favorites"),
                    record.video.channel
                ),
                keywords: format!(
                    "biblioteca library favoritos favorites {} {}",
                    record.video.title, record.video.channel
                ),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video: record.video.clone(),
                    seek_ms: None,
                },
            });
        }

        for (c_idx, collection) in self.library.data.collections.iter().enumerate() {
            for (v_idx, record) in collection.videos.iter().enumerate() {
                items.push(PaletteItem {
                    id: format!("library:col:{c_idx}:{v_idx}:{}", record.video.url),
                    category: PaletteCategory::Library,
                    title: record.video.title.clone(),
                    subtitle: format!(
                        "{} {} \u{2022} {}",
                        self.pick("Coleção", "Collection"),
                        collection.name,
                        record.video.channel
                    ),
                    keywords: format!(
                        "biblioteca library colecao collection {} {} {}",
                        collection.name, record.video.title, record.video.channel
                    ),
                    shortcut: None,
                    action: PaletteAction::OpenVideo {
                        video: record.video.clone(),
                        seek_ms: None,
                    },
                });
            }
        }

        for (idx, entry) in self.history.data.entries().iter().take(60).enumerate() {
            let video = self.find_known_video_by_id(&entry.video_id, &entry.title);
            let subtitle = if let Some(resume_ms) = entry.resume_position_ms() {
                format!(
                    "{} \u{2022} {} {}",
                    entry.channel,
                    self.pick("retomar em", "resume at"),
                    command_palette::format_timestamp_ms(resume_ms)
                )
            } else {
                entry.channel.clone()
            };
            items.push(PaletteItem {
                id: format!("history:{idx}:{}", entry.video_id),
                category: PaletteCategory::History,
                title: entry.title.clone(),
                subtitle,
                keywords: format!(
                    "historico history {} {} {}",
                    entry.title, entry.channel, entry.video_id
                ),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video,
                    seek_ms: None,
                },
            });
        }

        if self.auth.authenticated {
            for channel in &self.feed.home.subscription_channels {
                items.push(PaletteItem {
                    id: format!("sub:{}", channel.channel_id),
                    category: PaletteCategory::Subscription,
                    title: channel.title.clone(),
                    subtitle: self
                        .pick("Abrir página do canal inscrito", "Open subscribed channel")
                        .to_owned(),
                    keywords: format!("canal inscricao {} {}", channel.title, channel.channel_id),
                    shortcut: None,
                    action: PaletteAction::OpenChannel(ChannelTarget::ChannelId(
                        channel.channel_id.clone(),
                    )),
                });
            }
        } else {
            for channel in &self.followed_channels.data.channels {
                let target = if channel.channel_id.starts_with("UC") {
                    ChannelTarget::ChannelId(channel.channel_id.clone())
                } else if let Some(handle) = &channel.handle {
                    ChannelTarget::Handle(handle.clone())
                } else {
                    ChannelTarget::ChannelId(channel.channel_id.clone())
                };
                items.push(PaletteItem {
                    id: format!("followed:{}", channel.channel_id),
                    category: PaletteCategory::Subscription,
                    title: channel.title.clone(),
                    subtitle: self
                        .pick(
                            "Abrir canal seguido localmente",
                            "Open locally followed channel",
                        )
                        .to_owned(),
                    keywords: format!(
                        "canal seguido inscricao local {} {} {}",
                        channel.title,
                        channel.channel_id,
                        channel.handle.as_deref().unwrap_or("")
                    ),
                    shortcut: None,
                    action: PaletteAction::OpenChannel(target),
                });
            }
        }

        items
    }

    pub(crate) fn execute_palette_action(
        &mut self,
        action: command_palette::PaletteAction,
        ctx: &egui::Context,
    ) {
        self.ui.command_palette.close_palette();
        match action {
            command_palette::PaletteAction::Browse(section) => match section {
                BrowseSection::Home => self.handle_header_home(ctx),
                BrowseSection::Subscriptions => self.open_subscriptions_section(false, ctx),
                BrowseSection::Shorts => {
                    self.ui.settings_open = false;
                    self.ui.search_results_mode = false;
                    self.queue.remote_playlist_id = None;
                    self.ui.browse_section = BrowseSection::Shorts;
                    self.feed.filter = FeedFilter::Shorts;
                    self.close_channel_view();
                    if self.selected.is_some() {
                        if !self.maybe_enter_auto_mini_player(ctx) {
                            self.close_video(ctx);
                        }
                    } else {
                        self.sync_player_window_visibility();
                    }
                    if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
                        let _ = self.restore_cached_home_feed(ctx);
                    }
                    self.invalidate_feed_cache();
                    self.ensure_feed_cache();
                }
                other => {
                    if matches!(self.feed.filter, FeedFilter::Shorts) {
                        self.feed.filter = FeedFilter::All;
                    }
                    self.navigate_from_shortcut(ctx, other);
                }
            },
            command_palette::PaletteAction::SettingsTab(tab) => {
                self.settings.active_tab = tab;
                self.ui.settings_open = true;
                self.sync_player_window_visibility();
            }
            command_palette::PaletteAction::OpenVideo { video, seek_ms } => {
                self.open_video_at_timestamp(video, seek_ms, ctx);
            }
            command_palette::PaletteAction::OpenBookmark { bookmark } => {
                if let Some(video) = self.open_library_bookmark(&bookmark) {
                    self.ui.settings_open = false;
                    self.close_channel_view();
                    self.playback.pending_open_seek_ms = Some(bookmark.time_ms);
                    self.playback.pending_open_video = Some(video);
                }
            }
            command_palette::PaletteAction::OpenChannel(target) => {
                self.open_direct_youtube_target(
                    command_palette::DirectYouTubeTarget::Channel(target),
                    ctx,
                );
            }
            command_palette::PaletteAction::OpenDirectTarget(target) => {
                self.open_direct_youtube_target(target, ctx);
            }
            command_palette::PaletteAction::ToggleTheme => {
                let next = if self.settings.theme.value == Theme::Dark {
                    Theme::Light
                } else {
                    Theme::Dark
                };
                self.settings.apply_theme(next);
                self.sync_theme_visuals(ctx);
            }
            command_palette::PaletteAction::ToggleTheater => {
                self.toggle_theater_mode();
            }
            command_palette::PaletteAction::ToggleMiniPlayer => {
                self.toggle_mini_player(ctx);
            }
            command_palette::PaletteAction::CycleRepeat => {
                self.cycle_repeat_mode();
            }
            command_palette::PaletteAction::ShuffleQueue => {
                self.shuffle_queue();
            }
            command_palette::PaletteAction::ClearQueue => {
                self.clear_queue();
            }
            command_palette::PaletteAction::OpenShortcuts => {
                self.ui.shortcuts_open = true;
            }
        }
        ctx.request_repaint();
    }

    pub(crate) fn handle_command_palette_global_shortcut(&mut self, ctx: &egui::Context) -> bool {
        let custom = settings::Keybindings::parse_egui_key(
            &self.settings.prefs.value.keybindings.command_palette,
        );
        let triggered = ctx.input_mut(|input| {
            let custom_hit = custom.is_some_and(|(ctrl, alt, shift, key)| {
                let mods_cmd = egui::Modifiers {
                    alt,
                    ctrl,
                    shift,
                    mac_cmd: false,
                    command: ctrl,
                };
                let mods_ctrl = egui::Modifiers {
                    alt,
                    ctrl,
                    shift,
                    mac_cmd: false,
                    command: false,
                };
                input.consume_key(mods_cmd, key) || (ctrl && input.consume_key(mods_ctrl, key))
            });
            custom_hit
                || input.consume_key(egui::Modifiers::COMMAND, egui::Key::P)
                || input.consume_key(egui::Modifiers::CTRL, egui::Key::P)
        });
        if triggered {
            self.ui.command_palette.toggle();
            ctx.request_repaint();
        }
        triggered
    }

    pub(crate) fn command_palette_ui(&mut self, ctx: &egui::Context) {
        let open_t = ctx.animate_bool_with_time(
            egui::Id::new("command_palette_anim"),
            self.ui.command_palette.open,
            0.12,
        );
        if !self.ui.command_palette.open {
            return;
        }
        let pt = self.ui_pt();
        let palette = self.palette();
        let mut items = self.build_command_palette_items();
        let has_direct_query =
            command_palette::parse_direct_youtube_target(&self.ui.command_palette.query)
                .map(|direct| {
                    let canonical = direct.canonical_url();
                    let display = direct.display_label_i18n(pt);
                    items.insert(
                        0,
                        command_palette::PaletteItem {
                            id: "dynamic:direct-url".to_owned(),
                            category: command_palette::PaletteCategory::Action,
                            title: format!("{} {display}", self.pick("Abrir", "Open")),
                            subtitle: canonical,
                            keywords: self.ui.command_palette.query.clone(),
                            shortcut: Some("Enter"),
                            action: command_palette::PaletteAction::OpenDirectTarget(direct),
                        },
                    );
                })
                .is_some();

        let viewport_rect = ctx.input(|input| input.content_rect());
        let modal_width = (viewport_rect.width() - 32.0).clamp(300.0, 580.0);

        let mut close_requested = false;
        let mut activated_action: Option<command_palette::PaletteAction> = None;

        let scrim_alpha = (125.0 * open_t.max(0.75)).round() as u8;
        egui::Area::new(egui::Id::new("command_palette_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(viewport_rect.min)
            .show(ctx, |ui| {
                let (_, response) =
                    ui.allocate_exact_size(viewport_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    viewport_rect,
                    0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, scrim_alpha),
                );
                if response.clicked() {
                    close_requested = true;
                }
            });

        let outer_width = modal_width + 24.0;
        let modal_pos = egui::pos2(
            (viewport_rect.center().x - outer_width * 0.5).max(viewport_rect.left() + 8.0),
            viewport_rect.top() + 68.0,
        );
        egui::Area::new(egui::Id::new("command_palette_modal"))
            .order(egui::Order::Foreground)
            .fixed_pos(modal_pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(14)
                    .inner_margin(egui::Margin::same(12))
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(0, 0, 0, 150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(modal_width);
                        let input_resp = ui.add_sized(
                            [modal_width, 34.0],
                            egui::TextEdit::singleline(&mut self.ui.command_palette.query)
                                .hint_text(if pt {
                                    "Buscar comando, seção, vídeo, marcador ou link do YouTube..."
                                } else {
                                    "Search command, section, video, bookmark, or YouTube link..."
                                }),
                        );
                        if self.ui.command_palette.focus_requested {
                            input_resp.request_focus();
                            self.ui.command_palette.focus_requested = false;
                        }
                        if input_resp.changed() {
                            self.ui.command_palette.selected_index = 0;
                        }

                        let mut ranked = command_palette::filter_and_rank(
                            &self.ui.command_palette.query,
                            &items,
                            12,
                        );
                        if has_direct_query
                            && !ranked.iter().any(|entry| entry.index == 0)
                        {
                            ranked.insert(
                                0,
                                command_palette::RankedPaletteEntry {
                                    index: 0,
                                    score: 5_000,
                                },
                            );
                            ranked.truncate(12);
                        } else if has_direct_query
                            && let Some(pos) = ranked.iter().position(|entry| entry.index == 0)
                            && pos > 0
                        {
                            let entry = ranked.remove(pos);
                            ranked.insert(0, entry);
                        }

                        let (up_pressed, down_pressed, enter_pressed) = ctx.input(|input| {
                            (
                                input.key_pressed(egui::Key::ArrowUp),
                                input.key_pressed(egui::Key::ArrowDown),
                                input.key_pressed(egui::Key::Enter),
                            )
                        });
                        if up_pressed {
                            self.ui.command_palette.move_selection(-1, ranked.len());
                        }
                        if down_pressed {
                            self.ui.command_palette.move_selection(1, ranked.len());
                        }
                        if !ranked.is_empty()
                            && self.ui.command_palette.selected_index >= ranked.len()
                        {
                            self.ui.command_palette.selected_index = 0;
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);

                        if ranked.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new(self.pick(
                                        "Nenhum resultado correspondente.",
                                        "No matching results.",
                                    ))
                                    .color(palette.neutral_status_fg),
                                );
                                ui.add_space(12.0);
                            });
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("command_palette_results_scroll")
                                .max_height(360.0)
                                .show(ui, |ui| {
                                    for (row_idx, entry) in ranked.iter().enumerate() {
                                        let item = &items[entry.index];
                                        let selected =
                                            row_idx == self.ui.command_palette.selected_index;
                                        let row_fill = if selected {
                                            palette.active_button_bg
                                        } else {
                                            Color32::TRANSPARENT
                                        };
                                        let title_color = if selected {
                                            palette.active_button_fg
                                        } else {
                                            palette.channel_fg
                                        };
                                        let sub_color = if selected {
                                            palette.active_button_fg.gamma_multiply(0.8)
                                        } else {
                                            palette.neutral_status_fg
                                        };
                                        let row_resp = ui
                                            .push_id(&item.id, |ui| {
                                                egui::Frame::new()
                                                    .fill(row_fill)
                                                    .corner_radius(8)
                                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                                    .show(ui, |ui| {
                                                        ui.set_width(
                                                            (modal_width - 20.0).max(200.0),
                                                        );
                                                        ui.horizontal(|ui| {
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(&item.title)
                                                                        .size(13.5)
                                                                        .strong()
                                                                        .color(title_color),
                                                                );
                                                                if !item.subtitle.is_empty() {
                                                                    ui.label(
                                                                        RichText::new(
                                                                            &item.subtitle,
                                                                        )
                                                                        .size(11.5)
                                                                        .color(sub_color),
                                                                    );
                                                                }
                                                            });
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    if let Some(shortcut) =
                                                                        item.shortcut
                                                                    {
                                                                        ui.label(
                                                                            RichText::new(
                                                                                shortcut,
                                                                            )
                                                                            .size(11.0)
                                                                            .monospace()
                                                                            .color(sub_color),
                                                                        );
                                                                        ui.add_space(6.0);
                                                                    }
                                                                    ui.label(
                                                                        RichText::new(
                                                                            item.category.label(pt),
                                                                        )
                                                                        .size(11.0)
                                                                        .color(sub_color),
                                                                    );
                                                                },
                                                            );
                                                        });
                                                    })
                                                    .response
                                                    .interact(egui::Sense::click())
                                            })
                                            .inner
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if selected && (up_pressed || down_pressed) {
                                            row_resp.scroll_to_me(Some(egui::Align::Center));
                                        }
                                        if row_resp.clicked() {
                                            activated_action = Some(item.action.clone());
                                        }
                                        ui.add_space(2.0);
                                    }
                                });

                            if enter_pressed
                                && activated_action.is_none()
                                && let Some(entry) =
                                    ranked.get(self.ui.command_palette.selected_index)
                            {
                                activated_action = Some(items[entry.index].action.clone());
                            }
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(self.pick(
                                    "\u{2191}\u{2193} navegar  \u{2022}  Enter executar  \u{2022}  Esc fechar",
                                    "\u{2191}\u{2193} navigate  \u{2022}  Enter run  \u{2022}  Esc close",
                                ))
                                .size(11.0)
                                .color(palette.neutral_status_fg),
                            );
                        });
                    });
            });

        if let Some(action) = activated_action {
            self.execute_palette_action(action, ctx);
        } else if close_requested {
            self.ui.command_palette.close_palette();
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RepaintScheduleInput {
    pub(crate) minimized: bool,
    pub(crate) video_actively_playing: bool,
    pub(crate) native_hwnd_fallback_active: bool,
    pub(crate) user_interacting: bool,
    pub(crate) toast_or_animation_active: bool,
    pub(crate) thumbnails_loading: bool,
    pub(crate) async_jobs_active: bool,
}

pub(crate) fn compute_repaint_interval(input: RepaintScheduleInput) -> Option<Duration> {
    if input.minimized {
        return None;
    }
    if input.user_interacting {
        return Some(Duration::from_millis(16));
    }
    if input.video_actively_playing {
        return Some(Duration::from_millis(100));
    }
    if input.toast_or_animation_active || input.thumbnails_loading {
        return Some(Duration::from_millis(80));
    }
    if input.async_jobs_active || input.native_hwnd_fallback_active {
        return Some(Duration::from_millis(150));
    }
    None
}

impl FastTube {
    pub(crate) fn has_active_async_jobs(&self) -> bool {
        self.feed.search.rx.is_some()
            || self.feed.search_suggest.rx.is_some()
            || self.feed.home.rx.is_some()
            || self.feed.home.batch_rx.is_some()
            || self.feed.home.channels_rx.is_some()
            || self.library.liked_videos.rx.is_some()
            || self.channels.videos.rx.is_some()
            || self.channels.videos.more_rx.is_some()
            || self.channels.videos.sorted_rx.is_some()
            || self.channels.shorts.rx.is_some()
            || self.channels.streams.rx.is_some()
            || self.channels.playlists.rx.is_some()
            || self.channels.playlist_detail.rx.is_some()
            || self.channels.about.rx.is_some()
            || self.channels.search.rx.is_some()
            || self.auth.reauth_rx.is_some()
            || self.auth.rating_rx.is_some()
            || self.auth.sub_check_rx.is_some()
            || self.auth.sub_toggle_rx.is_some()
            || self.auth.my_channel_rx.is_some()
            || self.auth.remote_playlists_rx.is_some()
            || self.auth.playlist_action_rx.is_some()
            || self.auth.report_rx.is_some()
            || self.playback.quality_rx.is_some()
            || self.watch.live.watch_rx.is_some()
            || self.watch.live.stream_rx.is_some()
            || self.watch.live.send_rx.is_some()
            || self.watch.sponsorblock.rx.is_some()
            || self.watch.extras.comments_loading.is_some()
            || self.watch.extras.subtitles_loading.is_some()
            || self.watch.extras.transcript_loading
            || self.watch.extras.is_posting_comment()
            || self.watch.extras.is_rating_any_comment()
            || self.settings.cache_picker_rx.is_some()
            || self.settings.backup_picker_rx.is_some()
            || self.settings.oauth_picker_rx.is_some()
            || self.settings.download_picker_rx.is_some()
            || self.settings.ytdlp_rx.is_some()
            || self.settings.app_update_rx.is_some()
            || self.followed_channels.feed_rx.is_some()
            || self.downloads.manager.has_active()
            || self.queue.remote_playlist_rx.is_some()
            || self.watch.community_rx.is_some()
    }

    pub(crate) fn schedule_idle_aware_repaint(&self, ctx: &egui::Context) {
        #[cfg(windows)]
        let (video_actively_playing, native_hwnd_fallback_active) = self
            .player
            .as_ref()
            .map(|player| {
                let playing = self.selected.is_some()
                    && player.flag_cstr(c"pause") != Some(true)
                    && player.flag_cstr(c"eof-reached") != Some(true);
                let hwnd_fallback = playing && !player.is_render_api();
                (playing, hwnd_fallback)
            })
            .unwrap_or((false, false));
        #[cfg(not(windows))]
        let (video_actively_playing, native_hwnd_fallback_active) = (false, false);

        let seek_drag_active = self.playback.seek_drag.is_some();
        let (minimized, user_interacting) = ctx.input(|input| {
            let minimized =
                input.viewport().minimized.unwrap_or(false) || self.desktop.hidden_to_tray;
            let wheel_scrolled = input
                .raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::MouseWheel { .. }));
            let interacting = seek_drag_active
                || self.autoscroll.is_scrolling()
                || input.pointer.any_down()
                || input.pointer.is_moving()
                || input.smooth_scroll_delta != egui::Vec2::ZERO
                || wheel_scrolled;
            (minimized, interacting)
        });
        let overlay_auto_hide_active = video_actively_playing
            && self.playback.overlay_last_activity.elapsed().as_secs_f64()
                < overlay::OVERLAY_HIDE_DELAY_SECS;
        let thumbnails_loading =
            self.thumbnails.has_pending_work() || self.avatars.has_pending_work();
        let async_jobs_active = self.has_active_async_jobs();

        if let Some(interval) = compute_repaint_interval(RepaintScheduleInput {
            minimized,
            video_actively_playing,
            native_hwnd_fallback_active,
            user_interacting,
            toast_or_animation_active: overlay_auto_hide_active || !self.toasts.is_empty(),
            thumbnails_loading,
            async_jobs_active,
        }) {
            ctx.request_repaint_after(interval);
        }
    }

    pub(crate) fn hide_to_tray(&mut self, ctx: &egui::Context) {
        if !self.desktop.tray_active() {
            return;
        }
        self.desktop.hidden_to_tray = true;
        self.thumbnails.evict_all_gpu_textures(ctx);
        self.avatars.evict_all_gpu_textures(ctx);
        self.sync_player_window_visibility();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        ctx.request_repaint();
    }

    pub(crate) fn show_from_tray(&mut self, ctx: &egui::Context) {
        self.desktop.hidden_to_tray = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.sync_player_window_visibility();
        ctx.request_repaint();
    }

    pub(crate) fn handle_window_tray_lifecycle(&mut self, ctx: &egui::Context) {
        if !self.desktop.tray_active() {
            return;
        }
        let (close_requested, minimized) = ctx.input(|i| {
            (
                i.viewport().close_requested(),
                i.viewport().minimized.unwrap_or(false),
            )
        });
        if close_requested
            && self.settings.prefs.value.close_to_tray
            && !self.desktop.tray_quit_requested
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
            return;
        }
        if minimized && self.settings.prefs.value.minimize_to_tray && !self.desktop.hidden_to_tray {
            self.hide_to_tray(ctx);
        }
    }

    pub(crate) fn process_desktop_integrations(&mut self, ctx: &egui::Context) {
        self.desktop.autostart.poll();

        let tray_actions = self.desktop.poll_tray_actions();
        for action in tray_actions {
            match action {
                desktop::TrayAction::Show => {
                    self.show_from_tray(ctx);
                }
                desktop::TrayAction::ToggleVisibility => {
                    if self.desktop.hidden_to_tray {
                        self.show_from_tray(ctx);
                    } else {
                        self.hide_to_tray(ctx);
                    }
                }
                desktop::TrayAction::PlayPause => {
                    #[cfg(windows)]
                    {
                        let now = Instant::now();
                        if let Some(player) = &self.player {
                            let paused = player.property("pause").as_deref() == Some("yes");
                            let next = if paused { "no" } else { "yes" };
                            let _ = player.set("pause", next);
                        }
                        self.sample_player_before_transition(now);
                        self.flush_playback_history_at(now);
                    }
                }
                desktop::TrayAction::CheckUpdates => {
                    self.show_from_tray(ctx);
                    self.open_settings_tab(app_state::SettingsTab::General, ctx);
                    self.start_app_update_check(false, ctx);
                }
                desktop::TrayAction::Quit => {
                    self.desktop.tray_quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        for command in self.desktop.drain_instance_commands() {
            self.show_from_tray(ctx);
            if let desktop::InstanceCommand::OpenUrl(url) = command {
                let normalized = desktop::normalize_fasttube_protocol_url(&url).unwrap_or(url);
                if let Some(target) = command_palette::parse_direct_youtube_target(&normalized) {
                    self.open_direct_youtube_target(target, ctx);
                } else {
                    self.playback.pending_open_video = Some(Video {
                        title: normalized.clone(),
                        channel: String::new(),
                        url: normalized,
                        thumbnail: String::new(),
                        duration: String::new(),
                    });
                }
            }
        }

        let actions = self.desktop.poll_now_playing_commands();
        for action in actions {
            match action {
                desktop::NowPlayingAction::Play => {
                    #[cfg(windows)]
                    {
                        let is_live = self.active_video_is_live();
                        let mut go_live = false;
                        if let Some(player) = &self.player {
                            let ended = player.property("eof-reached").as_deref() == Some("yes");
                            if ended && is_live {
                                go_live = true;
                            } else {
                                let res = if ended {
                                    player
                                        .seek(0.0, "absolute+exact")
                                        .and_then(|()| player.set("pause", "no"))
                                } else {
                                    player.set("pause", "no")
                                };
                                if let Err(err) = res {
                                    self.message = err;
                                } else if ended {
                                    self.record_manual_seek(0.0, None, Instant::now());
                                }
                            }
                        }
                        if go_live && let Some(video) = self.selected.clone() {
                            self.go_live_edge(&video);
                        }
                    }
                }
                desktop::NowPlayingAction::Pause => {
                    #[cfg(windows)]
                    {
                        let now = Instant::now();
                        if let Some(player) = &self.player
                            && let Err(err) = player.set("pause", "yes")
                        {
                            self.message = err;
                        }
                        self.sample_player_before_transition(now);
                        self.flush_playback_history_at(now);
                    }
                }
                desktop::NowPlayingAction::PlayPause => {
                    #[cfg(windows)]
                    {
                        let now = Instant::now();
                        let is_live = self.active_video_is_live();
                        let mut go_live = false;
                        if let Some(player) = &self.player {
                            let ended = player.property("eof-reached").as_deref() == Some("yes");
                            let paused = player.property("pause").as_deref() == Some("yes");
                            if ended && is_live {
                                go_live = true;
                            } else {
                                let res = if ended {
                                    player
                                        .seek(0.0, "absolute+exact")
                                        .and_then(|()| player.set("pause", "no"))
                                } else {
                                    player.set("pause", if paused { "no" } else { "yes" })
                                };
                                if let Err(err) = res {
                                    self.message = err;
                                } else if ended {
                                    self.record_manual_seek(0.0, None, now);
                                } else if !paused {
                                    self.sample_player_before_transition(now);
                                    self.flush_playback_history_at(now);
                                }
                            }
                        }
                        if go_live && let Some(video) = self.selected.clone() {
                            self.go_live_edge(&video);
                        }
                    }
                }
                desktop::NowPlayingAction::Stop => {
                    self.close_video(ctx);
                }
                desktop::NowPlayingAction::Next => {
                    if !self.queue.data.is_empty() {
                        self.request_queue_advance();
                    } else if self.is_watch_short_mode() {
                        if let Some(video) = self.next_or_prev_short(true) {
                            self.playback.pending_open_video = Some(video);
                        }
                    } else if let Some(video) = self.next_recommended_video() {
                        self.playback.pending_open_video = Some(video);
                    }
                }
                desktop::NowPlayingAction::Previous => {
                    #[cfg(windows)]
                    let pos = self
                        .player
                        .as_ref()
                        .and_then(|p| p.number("time-pos"))
                        .unwrap_or(0.0);
                    #[cfg(not(windows))]
                    let pos = 0.0;
                    if pos > 5.0 {
                        self.seek_to_seconds(0.0);
                    } else if self.is_watch_short_mode()
                        && let Some(video) = self.next_or_prev_short(false)
                    {
                        self.playback.pending_open_video = Some(video);
                    } else {
                        self.seek_to_seconds(0.0);
                    }
                }
                desktop::NowPlayingAction::SeekByMs(delta_ms) => {
                    #[cfg(windows)]
                    {
                        let is_live = self.active_video_is_live();
                        let live_dvr = self
                            .playback
                            .active
                            .as_ref()
                            .is_some_and(|active| active.live_dvr);
                        if !is_live || live_dvr {
                            let mut go_live = false;
                            let mut seek_record: Option<(f64, Option<f64>)> = None;
                            if let Some(player) = &self.player {
                                let cur = player.number("time-pos").unwrap_or(0.0);
                                let duration = player.number("duration").filter(|&d| d > 0.0);
                                let delta_secs = delta_ms as f64 / 1000.0;
                                let target = (cur + delta_secs).max(0.0);
                                if is_live
                                    && let Some(dur) = duration
                                    && (dur <= 1.5 || target >= (dur - 1.5).max(0.0))
                                {
                                    go_live = true;
                                } else if is_live {
                                    let clamped = duration
                                        .map_or(target, |dur| target.min((dur - 1.5).max(0.0)));
                                    if let Err(err) = player.seek(clamped, "absolute+exact") {
                                        self.message = err;
                                    } else {
                                        seek_record = Some((clamped, duration));
                                    }
                                } else if let Err(err) = player.seek(delta_secs, "relative") {
                                    self.message = err;
                                } else {
                                    seek_record = Some((target, duration));
                                }
                            }
                            if go_live {
                                if let Some(video) = self.selected.clone() {
                                    self.go_live_edge(&video);
                                }
                            } else if let Some((pos, dur)) = seek_record {
                                self.record_manual_seek(pos, dur, Instant::now());
                            }
                        }
                    }
                }
                desktop::NowPlayingAction::SeekTo(target) => {
                    self.seek_to_seconds(target.as_secs_f64());
                }
                desktop::NowPlayingAction::SetVolume(vol) =>
                {
                    #[cfg(windows)]
                    if let Some(player) = &self.player {
                        let pct = (vol.clamp(0.0, 1.0) * 100.0).round();
                        if let Err(err) = player.set("volume", &format!("{pct:.0}")) {
                            self.message = err;
                        }
                    }
                }
                desktop::NowPlayingAction::Raise => {
                    self.show_from_tray(ctx);
                }
            }
        }

        let (snapshot, actively_playing) = if let Some(video) = self.selected.clone() {
            #[cfg(windows)]
            let (playing, ended, position_secs, duration_secs, volume_fraction) =
                if let Some(player) = &self.player {
                    let paused = player.property("pause").as_deref() == Some("yes");
                    let ended = player.property("eof-reached").as_deref() == Some("yes");
                    let pos = player
                        .number("time-pos")
                        .filter(|v| *v >= 0.0)
                        .or_else(|| {
                            self.playback
                                .active
                                .as_ref()
                                .map(|a| a.position_ms as f64 / 1000.0)
                        })
                        .unwrap_or(0.0);
                    let dur = player.number("duration").filter(|v| *v > 0.0).or_else(|| {
                        self.playback
                            .active
                            .as_ref()
                            .and_then(|a| a.duration_ms)
                            .map(|ms| ms as f64 / 1000.0)
                    });
                    let vol = player.number("volume").map(|v| (v / 100.0).clamp(0.0, 1.0));
                    (!paused && !ended, ended, pos, dur, vol)
                } else {
                    (false, false, 0.0, None, None)
                };
            #[cfg(not(windows))]
            let (playing, ended, position_secs, duration_secs, volume_fraction) =
                (false, false, 0.0, None, None);

            let can_next = !self.queue.data.is_empty()
                || (self.is_watch_short_mode() && self.next_or_prev_short(true).is_some())
                || self.next_recommended_video().is_some();
            let can_previous = position_secs > 5.0
                || (self.is_watch_short_mode() && self.next_or_prev_short(false).is_some());

            (
                Some(desktop::NowPlayingSnapshot {
                    video_id: video.url,
                    title: video.title,
                    channel: video.channel,
                    thumbnail_url: video.thumbnail,
                    playing,
                    ended,
                    position_secs,
                    duration_secs,
                    volume_fraction,
                    can_next,
                    can_previous,
                }),
                playing,
            )
        } else {
            (None, false)
        };

        self.desktop.sync_now_playing(snapshot, ctx);

        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let keep_awake = actively_playing && self.settings.prefs.value.keep_awake_during_playback;
        let power_events = self.desktop.sync_power(keep_awake, minimized);
        for event in power_events {
            match event {
                fastframe_power::Event::Suspending(ack) => {
                    let now = Instant::now();
                    #[cfg(windows)]
                    if let Some(player) = &self.player
                        && player.property("pause").as_deref() != Some("yes")
                    {
                        let _ = player.set("pause", "yes");
                    }
                    #[cfg(windows)]
                    self.sample_player_before_transition(now);
                    self.flush_playback_history_at(now);
                    ack.done();
                }
                fastframe_power::Event::EndingSession(ack) => {
                    let now = Instant::now();
                    #[cfg(windows)]
                    self.sample_player_before_transition(now);
                    self.flush_playback_history_at(now);
                    self.desktop.release_keep_awake();
                    ack.done();
                }
                fastframe_power::Event::Resumed => {
                    ctx.request_repaint();
                }
            }
        }

        let update_version = self
            .active_app_update_available()
            .map(|u| u.latest_version.clone());
        let video_title = self.selected.as_ref().map(|v| v.title.clone());
        self.desktop.sync_tray(
            self.desktop.hidden_to_tray,
            self.selected.is_some(),
            actively_playing,
            self.ui_pt(),
            update_version.as_deref(),
            video_title.as_deref(),
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareTargetKind {
    Embed,
    WhatsApp,
    X,
    Messages,
    Facebook,
    Email,
    Reddit,
    LinkedIn,
}

impl ShareTargetKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Embed => "Incorporar",
            Self::WhatsApp => "WhatsApp",
            Self::X => "X",
            Self::Messages => "Mensagens",
            Self::Facebook => "Facebook",
            Self::Email => "E-mail",
            Self::Reddit => "Reddit",
            Self::LinkedIn => "LinkedIn",
        }
    }

    pub(crate) fn launch_url(self, share_url: &str, title: &str) -> String {
        let encoded_url = encode_uri_component(share_url);
        let encoded_title = encode_uri_component(title);
        match self {
            Self::Embed => String::new(),
            Self::WhatsApp => format!("https://api.whatsapp.com/send?text={encoded_url}"),
            Self::X => {
                format!("https://twitter.com/intent/tweet?url={encoded_url}&text={encoded_title}")
            }
            Self::Messages => format!("sms:?body={encoded_url}"),
            Self::Facebook => format!("https://www.facebook.com/sharer/sharer.php?u={encoded_url}"),
            Self::Email => {
                format!("mailto:?subject={encoded_title}&body={encoded_url}")
            }
            Self::Reddit => {
                format!("https://www.reddit.com/submit?url={encoded_url}&title={encoded_title}")
            }
            Self::LinkedIn => {
                format!("https://www.linkedin.com/sharing/share-offsite/?url={encoded_url}")
            }
        }
    }
}

fn paint_share_target_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    kind: ShareTargetKind,
    dark_theme: bool,
    hovered: bool,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    let bg = match kind {
        ShareTargetKind::Embed => Color32::from_rgb(241, 241, 241),
        ShareTargetKind::WhatsApp => Color32::from_rgb(37, 211, 102),
        ShareTargetKind::X => Color32::BLACK,
        ShareTargetKind::Messages => Color32::WHITE,
        ShareTargetKind::Facebook => Color32::from_rgb(24, 119, 242),
        ShareTargetKind::Email => Color32::from_rgb(117, 117, 117),
        ShareTargetKind::Reddit => Color32::from_rgb(255, 69, 0),
        ShareTargetKind::LinkedIn => Color32::from_rgb(10, 102, 194),
    };
    painter.circle_filled(center, radius, bg);
    if hovered {
        painter.circle_filled(
            center,
            radius,
            Color32::from_rgba_unmultiplied(255, 255, 255, 28),
        );
    }
    if (kind == ShareTargetKind::X && dark_theme)
        || (kind == ShareTargetKind::Messages && !dark_theme)
    {
        let border = if dark_theme {
            Color32::from_rgb(66, 66, 66)
        } else {
            Color32::from_rgb(218, 218, 218)
        };
        painter.circle_stroke(center, radius, egui::Stroke::new(1.0, border));
    }

    match kind {
        ShareTargetKind::Embed => {
            let stroke = egui::Stroke::new(2.0, Color32::from_rgb(24, 24, 24));
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y - 6.0),
                    egui::pos2(center.x - 11.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 11.0, center.y),
                    egui::pos2(center.x - 5.0, center.y + 6.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 5.0, center.y - 6.0),
                    egui::pos2(center.x + 11.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 11.0, center.y),
                    egui::pos2(center.x + 5.0, center.y + 6.0),
                ],
                stroke,
            );
        }
        ShareTargetKind::WhatsApp => {
            let white = egui::Stroke::new(2.2, Color32::WHITE);
            painter.circle_stroke(center, 12.0, white);
            painter.line_segment(
                [
                    egui::pos2(center.x - 8.5, center.y + 8.5),
                    egui::pos2(center.x - 12.5, center.y + 13.0),
                ],
                white,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 12.5, center.y + 13.0),
                    egui::pos2(center.x - 6.5, center.y + 11.0),
                ],
                white,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.0, center.y - 4.0),
                    egui::pos2(center.x + 4.5, center.y + 4.5),
                ],
                egui::Stroke::new(2.4, Color32::WHITE),
            );
        }
        ShareTargetKind::X => {
            let stroke = egui::Stroke::new(2.2, Color32::WHITE);
            painter.line_segment(
                [
                    egui::pos2(center.x - 9.0, center.y - 9.0),
                    egui::pos2(center.x + 9.0, center.y + 9.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 9.0, center.y - 9.0),
                    egui::pos2(center.x - 9.0, center.y + 9.0),
                ],
                stroke,
            );
        }
        ShareTargetKind::Messages => {
            let back = egui::Rect::from_center_size(
                egui::pos2(center.x + 2.0, center.y + 1.0),
                egui::vec2(22.0, 17.0),
            );
            painter.rect_filled(back, 6.0, Color32::from_rgb(66, 133, 244));
            let front = egui::Rect::from_center_size(
                egui::pos2(center.x - 2.0, center.y - 2.0),
                egui::vec2(20.0, 15.0),
            );
            painter.rect_filled(front, 6.0, Color32::from_rgb(26, 115, 232));
        }
        ShareTargetKind::Facebook => {
            painter.text(
                egui::pos2(center.x, center.y + 1.0),
                egui::Align2::CENTER_CENTER,
                "f",
                egui::FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
        ShareTargetKind::Email => {
            let env = egui::Rect::from_center_size(center, egui::vec2(24.0, 17.0));
            let stroke = egui::Stroke::new(1.8, Color32::WHITE);
            painter.rect_stroke(env, 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [env.left_top(), egui::pos2(center.x, center.y + 2.0)],
                stroke,
            );
            painter.line_segment(
                [env.right_top(), egui::pos2(center.x, center.y + 2.0)],
                stroke,
            );
        }
        ShareTargetKind::Reddit => {
            painter.circle_filled(egui::pos2(center.x, center.y + 2.0), 9.0, Color32::WHITE);
            painter.circle_filled(egui::pos2(center.x - 3.5, center.y + 1.5), 1.6, bg);
            painter.circle_filled(egui::pos2(center.x + 3.5, center.y + 1.5), 1.6, bg);
            painter.line_segment(
                [
                    egui::pos2(center.x, center.y - 7.0),
                    egui::pos2(center.x + 3.5, center.y - 12.0),
                ],
                egui::Stroke::new(1.8, Color32::WHITE),
            );
            painter.circle_filled(
                egui::pos2(center.x + 6.0, center.y - 11.0),
                2.3,
                Color32::WHITE,
            );
        }
        ShareTargetKind::LinkedIn => {
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "in",
                egui::FontId::proportional(20.0),
                Color32::WHITE,
            );
        }
    }
}

fn open_external_url(url: &str) {
    #[cfg(all(windows, not(test)))]
    {
        let _ = std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
    }
    #[cfg(not(all(windows, not(test))))]
    {
        let _ = url;
    }
}

fn encode_uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(*byte as char);
        } else {
            out.push('%');
            let hi = (byte >> 4) & 0x0f;
            let lo = byte & 0x0f;
            out.push(
                char::from_digit(u32::from(hi), 16)
                    .unwrap_or('0')
                    .to_ascii_uppercase(),
            );
            out.push(
                char::from_digit(u32::from(lo), 16)
                    .unwrap_or('0')
                    .to_ascii_uppercase(),
            );
        }
    }
    out
}

fn parse_share_timestamp_secs(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.len() {
        1 => parts[0].trim().parse::<u64>().ok(),
        2 => {
            let mins = parts[0].trim().parse::<u64>().ok()?;
            let secs = parts[1].trim().parse::<u64>().ok()?;
            Some(mins.saturating_mul(60).saturating_add(secs))
        }
        3 => {
            let hours = parts[0].trim().parse::<u64>().ok()?;
            let mins = parts[1].trim().parse::<u64>().ok()?;
            let secs = parts[2].trim().parse::<u64>().ok()?;
            Some(
                hours
                    .saturating_mul(3600)
                    .saturating_add(mins.saturating_mul(60))
                    .saturating_add(secs),
            )
        }
        _ => None,
    }
}

fn format_share_timestamp(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

pub(crate) fn build_youtube_share_url(video_url: &str, start_secs: Option<u64>) -> String {
    if let Some(id) = history::youtube_video_id(video_url) {
        if let Some(secs) = start_secs.filter(|&s| s > 0) {
            format!("https://youtu.be/{id}?t={secs}")
        } else {
            format!("https://youtu.be/{id}")
        }
    } else {
        video_url.to_owned()
    }
}

pub(crate) fn build_youtube_embed_code(
    video_id: &str,
    title: &str,
    start_secs: Option<u64>,
) -> String {
    let escaped_title = title
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let src = if let Some(secs) = start_secs.filter(|&s| s > 0) {
        format!("https://www.youtube.com/embed/{video_id}?start={secs}")
    } else {
        format!("https://www.youtube.com/embed/{video_id}")
    };
    format!(
        "<iframe width=\"560\" height=\"315\" src=\"{src}\" title=\"{escaped_title}\" frameborder=\"0\" allow=\"accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share\" referrerpolicy=\"strict-origin-when-cross-origin\" allowfullscreen></iframe>"
    )
}

fn format_subscribers_pt(count: u64) -> String {
    if count == 1 {
        "1 inscrito".to_owned()
    } else if count < 1_000 {
        format!("{count} inscritos")
    } else if count < 1_000_000 {
        let tenths = count / 100;
        let whole = tenths / 10;
        let frac = tenths % 10;
        if frac == 0 {
            format!("{whole} mil inscritos")
        } else {
            format!("{whole},{frac} mil inscritos")
        }
    } else {
        let tenths = count / 100_000;
        let whole = tenths / 10;
        let frac = tenths % 10;
        if frac == 0 {
            format!("{whole} mi inscritos")
        } else {
            format!("{whole},{frac} mi inscritos")
        }
    }
}

fn screenshot_dir_candidates(
    user_profile: Option<&str>,
    local_app_data: Option<&str>,
) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(profile) = user_profile.map(str::trim).filter(|s| !s.is_empty()) {
        dirs.push(
            std::path::PathBuf::from(profile)
                .join("Pictures")
                .join("FastTube"),
        );
    }
    if let Some(local) = local_app_data.map(str::trim).filter(|s| !s.is_empty()) {
        let fallback = std::path::PathBuf::from(local)
            .join("FastTube")
            .join("screenshots");
        if !dirs.contains(&fallback) {
            dirs.push(fallback);
        }
    }
    if dirs.is_empty() {
        dirs.push(std::env::temp_dir().join("FastTube").join("screenshots"));
    }
    dirs
}

#[cfg_attr(not(test), allow(dead_code))]
fn resolve_screenshot_dir_from(
    user_profile: Option<&str>,
    local_app_data: Option<&str>,
) -> std::path::PathBuf {
    screenshot_dir_candidates(user_profile, local_app_data)
        .into_iter()
        .next()
        .unwrap_or_else(|| std::env::temp_dir().join("FastTube").join("screenshots"))
}

fn build_screenshot_filename(video: &Video, position_secs: f64, now_epoch_secs: u64) -> String {
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in video.title.trim().chars() {
        if slug.len() >= 48 {
            break;
        }
        let valid = !ch.is_control()
            && !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            && !ch.is_whitespace();
        if valid {
            slug.push(ch);
            prev_sep = false;
        } else if !slug.is_empty() && !prev_sep {
            slug.push('_');
            prev_sep = true;
        }
    }
    let slug = slug.trim_matches('_');
    let base = if slug.is_empty() {
        history::youtube_video_id(&video.url).unwrap_or_else(|| "video".to_owned())
    } else {
        slug.to_owned()
    };
    let clamped_secs = if position_secs.is_finite() && position_secs > 0.0 {
        position_secs.floor() as u64
    } else {
        0
    };
    let mins = clamped_secs / 60;
    let secs = clamped_secs % 60;
    format!("fasttube_{base}_{mins}m{secs:02}s_{now_epoch_secs}.png")
}

fn main() -> eframe::Result {
    let mut instance = match desktop::claim_single_instance(std::env::args()) {
        desktop::SingleInstanceOutcome::Primary(handle) => Some(handle),
        desktop::SingleInstanceOutcome::Delegated => return Ok(()),
    };
    let start_hidden = desktop::has_hidden_launch_arg(std::env::args());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_inner_size([1360.0, 840.0])
            .with_min_inner_size([480.0, 420.0])
            .with_icon(std::sync::Arc::new(desktop::window_icon_data()))
            .with_visible(!start_hidden),
        ..Default::default()
    };
    fastframe_graphics::Graphics::new("FastTube").run_native(
        || options.clone(),
        || {
            let instance_handle = instance.take();
            Box::new(move |creation| {
                let rendering = fastframe_text::detect();
                let mut app = FastTube::default();
                let mut fonts = fastframe_fonts::FontSetup::default().definitions();
                rendering.apply_to(&mut fonts);
                creation.egui_ctx.set_fonts(fonts);
                app.sync_theme_visuals(&creation.egui_ctx);
                egui_extras::install_image_loaders(&creation.egui_ctx);
                fastframe_icons::install::<Icon>(&creation.egui_ctx);
                app.desktop
                    .init(&creation.egui_ctx, instance_handle, start_hidden);
                if start_hidden && !app.desktop.tray_active() {
                    creation
                        .egui_ctx
                        .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                }
                Ok(Box::new(app))
            })
        },
    )
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
