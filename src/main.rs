#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod app;
mod app_state;
mod async_task;
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
mod history_sync;
mod http_client;
mod i18n;
mod innertube;
mod insights;
mod layout;
mod library;
mod library_ui;
mod live_chat;
mod oauth;
mod overlay;
mod persistence;
#[cfg(windows)]
mod player;
mod playlist_sync;
mod process;
mod queue;
mod recommendations;
mod search;
mod session;
mod settings;
mod settings_ui;
mod skeleton;
mod sponsorblock;
mod stream_manifest;
mod thumbnails;
mod toast;
mod transcript;
mod ui_icons;
mod updater;
mod watch_extras;
mod watch_ui;
mod win11;

use app::*;
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
pub(crate) const HISTORY_SAVE_INTERVAL: Duration = Duration::from_secs(20);
pub(crate) const HISTORY_MIN_POSITION_DELTA_MS: u64 = 1_000;
pub(crate) const HISTORY_MAX_TEXT_BYTES: usize = 4096;
pub(crate) const DIAGNOSTICS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const DEFAULT_CONFIGURED_HWDEC: &str = "auto-safe";
pub(crate) const SUBSCRIPTIONS_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

pub(crate) const DEFAULT_QUEUE_LIMIT: usize = 50;
pub(crate) const MAX_HOME_CACHED_ITEMS: usize = 300;
pub(crate) const MAX_VIDEO_METADATA_ENTRIES: usize = 1_500;
pub(crate) const MAX_CHANNEL_AVATARS_ENTRIES: usize = 500;

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
        Theme::System => "Sistema",
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
        let concrete = match theme {
            Theme::System => theme
                .resolve(win11::read_windows_apps_use_dark_theme())
                .to_concrete_theme(),
            other => other,
        };
        match concrete {
            Theme::Dark | Theme::System => Self {
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
    let concrete = match theme {
        Theme::System => theme
            .resolve(win11::read_windows_apps_use_dark_theme())
            .to_concrete_theme(),
        other => other,
    };
    let mut visuals = match concrete {
        Theme::Dark | Theme::System => {
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
    let concrete = match theme {
        Theme::System => theme
            .resolve(win11::read_windows_apps_use_dark_theme())
            .to_concrete_theme(),
        other => other,
    };
    let visuals = visuals_for_theme(concrete);
    let egui_theme = match concrete {
        Theme::Dark | Theme::System => egui::Theme::Dark,
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
#[link(name = "user32")]
unsafe extern "system" {
    fn GetClipboardSequenceNumber() -> u32;
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn os_clipboard_sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn os_clipboard_sequence_number() -> u32 {
    0
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
    pub(crate) oauth_session: Option<oauth::TokenSession>,
    pub(crate) result: Result<Vec<search::ChannelPlaylist>, String>,
}

pub(crate) struct ChannelPlaylistVideosFetchMessage {
    pub(crate) request_id: u64,
    pub(crate) target: ChannelTarget,
    pub(crate) playlist_id: String,
    pub(crate) oauth_session: Option<oauth::TokenSession>,
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
    pub(crate) has_previous: bool,
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
    pub(crate) persistence: persistence::PersistenceWorker,
    pub(crate) deferred_fonts_rx: Option<Receiver<egui::FontDefinitions>>,

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
    pub(crate) desktop_win11: app_state::Win11RuntimeState,
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
        let persistence = persistence::PersistenceWorker::new();
        let (history, history_path, history_fingerprint, history_read_failed, history_error) =
            Self::load_initial_history();
        let loaded_history_path = history_path.clone();
        if let Some(path) = history_path.as_ref()
            && !history_read_failed
        {
            persistence.record_loaded_fingerprint(
                persistence::PersistenceSlot::History,
                path.clone(),
                history_fingerprint,
            );
        }
        #[cfg(not(test))]
        let library_path = library::path().ok();
        #[cfg(test)]
        let library_path: Option<PathBuf> = None;
        #[cfg(not(test))]
        let (library, library_fingerprint, library_read_failed, library_error) =
            match library_path.as_deref() {
                Some(path) => match library::load_with_fingerprint(path) {
                    Ok((value, fp)) => (value, fp, false, None),
                    Err(error) => (
                        library::Library::default(),
                        None,
                        true,
                        Some(format!("Biblioteca local indisponível: {error}")),
                    ),
                },
                None => (
                    library::Library::default(),
                    None,
                    false,
                    Some("Não foi possível localizar a pasta de dados local.".into()),
                ),
            };
        #[cfg(test)]
        let (library, library_fingerprint, library_read_failed, library_error): (
            library::Library,
            Option<history::FileFingerprint>,
            bool,
            Option<String>,
        ) = (library::Library::default(), None, false, None);
        if let Some(path) = library_path.as_ref()
            && !library_read_failed
        {
            persistence.record_loaded_fingerprint(
                persistence::PersistenceSlot::Library,
                path.clone(),
                library_fingerprint,
            );
        }
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
        let (queue, queue_fingerprint, queue_read_failed, queue_error) = match queue_path.as_deref()
        {
            Some(path) => match Queue::load_with_fingerprint(path, DEFAULT_QUEUE_LIMIT) {
                Ok((queue, fp)) => (queue, fp, false, None),
                Err(error) => (
                    Queue::new(DEFAULT_QUEUE_LIMIT),
                    None,
                    true,
                    Some(format!(
                        "Não foi possível ler a fila salva: {error}. O arquivo foi preservado."
                    )),
                ),
            },
            None => (
                Queue::new(DEFAULT_QUEUE_LIMIT),
                None,
                true,
                if cfg!(test) {
                    None
                } else {
                    Some("Não foi possível localizar o arquivo da fila.".into())
                },
            ),
        };
        if let Some(path) = queue_path.as_ref()
            && !queue_read_failed
        {
            persistence.record_loaded_fingerprint(
                persistence::PersistenceSlot::Queue,
                path.clone(),
                queue_fingerprint,
            );
        }
        #[cfg(not(test))]
        let cached_has_saved_profile = session::has_saved_profile()
            || innertube::web_session_path().is_ok_and(|p| p.is_file());
        #[cfg(test)]
        let cached_has_saved_profile = false;
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
        let initial_repeat_mode = queue.repeat_mode();
        let mut downloads_state = app_state::DownloadState::default();
        downloads_state
            .manager
            .set_max_concurrent(app_prefs.download_max_concurrent);
        downloads_state.manager.default_options = downloads::DownloadOptions {
            embed_subs: app_prefs.download_embed_subs,
            embed_chapters: app_prefs.download_embed_chapters,
            embed_thumbnail: app_prefs.download_embed_thumbnail,
            ..downloads::DownloadOptions::default()
        };
        #[cfg(not(test))]
        {
            let dl_history_path = downloads::downloads_history_path();
            downloads_state.history_path = Some(dl_history_path.clone());
            let default_dl_dir = downloads::DownloadManager::effective_download_dir(
                app_prefs.download_dir_override.as_deref(),
            );
            if let Ok(records) = downloads::load_download_records_from(&dl_history_path) {
                downloads_state
                    .manager
                    .restore_from_records(records, &default_dl_dir);
            }
        }
        #[cfg(not(test))]
        let (initial_report_history, report_history_path) = {
            let path = Self::default_report_history_path().ok();
            let loaded = path
                .as_deref()
                .and_then(|p| Self::load_report_history_entries_from(p).ok())
                .unwrap_or_default();
            (loaded, path)
        };
        #[cfg(test)]
        let (initial_report_history, report_history_path): (
            Vec<app_state::ReportedContentEntry>,
            Option<PathBuf>,
        ) = (Vec::new(), None);
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
            persistence,
            deferred_fonts_rx: None,
            desktop: desktop::DesktopIntegrations::default(),
            desktop_win11: app_state::Win11RuntimeState::default(),

            feed: app_state::BrowseFeedState {
                search: app_state::SearchJob::default(),
                report_history: initial_report_history,
                report_history_path,
                ..app_state::BrowseFeedState::default()
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
                authenticated: session_authenticated,
                session: oauth_session,
                cached_has_saved_profile,
                ..app_state::AuthState::default()
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
                loaded_fingerprint: history_fingerprint,
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
                remote: app_state::RemoteHistoryState::default(),
            },
            queue: app_state::QueueState {
                data: queue,
                path: queue_path,
                loaded_fingerprint: queue_fingerprint,
                read_failed: queue_read_failed,
                persistence_error: queue_error,
                repeat_mode: initial_repeat_mode,
                ..app_state::QueueState::default()
            },
            library: app_state::LibraryState {
                data: library,
                revision: 0,
                path: library_path,
                loaded_fingerprint: library_fingerprint,
                read_failed: library_read_failed,
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
                detail_chip_filter: 0,
                liked_videos: app_state::LikedVideosState::default(),
                ..app_state::LibraryState::default()
            },
            followed_channels: app_state::FollowedChannelsState {
                data: followed_data,
                path: followed_path,
                error: followed_error,
                ..app_state::FollowedChannelsState::default()
            },
            downloads: downloads_state,
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

impl eframe::App for FastTube {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.watch.extras.sync_video(None);
        self.watch.live.reset_for_video(None, None);
        self.feed.search.task.cancel();
        self.feed.search_page_task.cancel();
        self.feed.home_innertube_task.cancel();
        self.feed.subs_innertube_task.cancel();
        self.feed.search_suggest.task.cancel();
        self.channels.videos.task.cancel();
        self.cancel_channel_subrequests();
        self.playback.quality_task.cancel();
        self.playback.manifest_task.cancel();
        self.playback.current_manifest = None;
        self.playback.active_direct_stream_url = None;
        self.playback.direct_stream_fallback_attempted = false;
        self.playback.pending_open_queue_entry_id = None;
        self.playback.stream_prefetcher.cancel();
        self.watch.next_task.cancel();
        self.watch.innertube_comments_task.cancel();
        self.watch.feedback_task.cancel();
        self.feed.home.stream_task.cancel();
        self.feed.home.batch_task.cancel();
        self.feed.home.channels_task.cancel();
        if let Some(cancel) = self.library.liked_videos.cancel.take() {
            cancel.cancel();
        }
        self.library.remote_playlists.cancel_active();
        if let Some(cancel) = self.library.remote_playlists.action_cancel.take() {
            cancel.cancel();
        }
        self.library.remote_playlists.action_rx = None;
        self.library.watch_later_remote.cancel_active();
        self.library.active_playlist_detail.cancel_active();
        if let Some(cancel) = self.watch.mix_cancel.take() {
            cancel.cancel();
        }
        self.watch.mix_rx = None;
        self.watch.mix_active_request_id = None;
        self.auth.reauth_task.cancel();
        self.auth.rating_task.cancel();
        self.auth.sub_check_task.cancel();
        self.auth.sub_toggle_task.cancel();
        self.auth.my_channel_task.cancel();
        self.auth.remote_playlists_task.cancel();
        self.auth.playlist_action_task.cancel();
        self.auth.report_task.cancel();
        self.followed_channels.feed_task.cancel();
        self.queue.remote_playlist_task.cancel();
        self.watch.community_task.cancel();
        self.settings.ytdlp_task.cancel();
        self.settings.app_update_task.cancel();
        self.settings.cache_picker_task.cancel();
        self.settings.download_picker_task.cancel();
        self.settings.backup_picker_task.cancel();
        self.settings.oauth_picker_task.cancel();
        if let Some(cancel) = self.history.remote.sync_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.history.remote.chip_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.history.remote.feedback_cancel.take() {
            cancel.cancel();
        }
        self.history.remote.sync_rx = None;
        self.history.remote.chip_rx = None;
        self.history.remote.feedback_rx = None;
        self.downloads.manager.cancel_all();
        self.persist_downloads_state();
        self.feed.notifications_task.cancel();
        self.feed.notifications_loading = false;
        self.feed.notifications_loading_more = false;
        self.feed.explore_task.cancel();
        self.feed.explore_loading = false;
        self.feed.explore_loading_more = false;
        self.feed.shorts_task.cancel();
        self.watch.comment_mutation_task.cancel();
        self.watch.replay_chat_task.cancel();
        if self.desktop_win11.subclass_installed
            && let Some(hwnd) = self.desktop_win11.hwnd
        {
            win11::remove_window_subclass(hwnd);
            self.desktop_win11.subclass_installed = false;
        }
        self.desktop_win11.theme_observer = None;
        let now = Instant::now();
        #[cfg(windows)]
        {
            self.remember_current_player_volume();
            self.sample_player_before_transition(now);
        }
        self.flush_remote_watch_tracking();
        self.flush_playback_history_at(now);
        self.flush_persistence();
        #[cfg(windows)]
        {
            self.player = None;
            self.playback.frame_snapshot = None;
        }
        self.desktop.release_keep_awake();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.preprocess_raw_input(raw_input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.desktop_win11.maximize_button_rect = None;
        #[cfg(windows)]
        self.poll_player_frame_events(&ctx);
        self.handle_window_tray_lifecycle(&ctx);
        self.maybe_auto_check_app_updates(&ctx);
        self.process_desktop_integrations(&ctx);
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let window_hidden =
            Self::is_window_hidden_for_power(minimized, self.desktop.hidden_to_tray);
        #[cfg(windows)]
        if let Some(player) = &self.player {
            player.set_window_hidden(window_hidden);
        }
        if self.desktop.hidden_to_tray {
            self.schedule_idle_aware_repaint(&ctx);
            return;
        }
        self.watch
            .extras
            .sync_video(self.selected.as_ref().map(|video| video.url.as_str()));
        if let Some(status) = self.watch.extras.poll_with_session(
            &mut self.auth.youtube_web_session,
            &mut self.auth.youtube_web_session_checked,
        ) {
            self.message = status;
        }
        if self.auth.youtube_web_session.is_some() {
            self.auth.cached_has_saved_profile = true;
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
        if !self.feed.home_swr_hydrated {
            self.feed.home_swr_hydrated = true;
            #[cfg(not(test))]
            {
                let _ = self.hydrate_home_from_swr_cache(&ctx);
                if self.ui.browse_section == BrowseSection::Home
                    && !self.ui.search_results_mode
                    && !self.feed.home_innertube_task.is_running()
                {
                    self.start_innertube_home_feed(None, false, true, &ctx);
                }
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
            if !self.feed.home.cached_loaded
                && !self.feed.home.batch_task.is_running()
                && !self.feed.home_innertube_task.is_running()
            {
                self.start_fast_home_feed(false, &ctx);
            }
        } else if !self.auth.authenticated && !self.feed.home.initial_public_started {
            self.feed.home.initial_public_started = true;
            if !self.feed.home.cached_loaded
                && !self.feed.home.batch_task.is_running()
                && !self.feed.home_innertube_task.is_running()
            {
                self.start_fast_home_feed(false, &ctx);
            }
        }
        if !cfg!(test)
            && !self.history.remote.initial_sync_attempted
            && (self.auth.youtube_web_session.is_some() || self.auth.cached_has_saved_profile)
        {
            self.history.remote.initial_sync_attempted = true;
            self.start_history_sync(false, false, &ctx);
        }
        if !cfg!(test)
            && !self.library.remote_playlists.initial_sync_attempted
            && (self.auth.youtube_web_session.is_some()
                || self.auth.cached_has_saved_profile
                || self.auth.authenticated)
        {
            self.library.remote_playlists.initial_sync_attempted = true;
            self.start_playlists_sync(false, false, &ctx);
        }
        if !cfg!(test)
            && !self.feed.notifications_initial_sync_attempted
            && (self.auth.youtube_web_session.is_some() || self.auth.cached_has_saved_profile)
        {
            self.feed.notifications_initial_sync_attempted = true;
            self.start_notifications_inbox_fetch(false, None, &ctx);
        }
        self.in_app_ui_pass = true;
        self.sync_theme_visuals(&ctx);
        if !self.autoscroll.is_active() {
            let _ = self.handle_frameless_window_resize(&ctx);
        }
        self.scrolling.apply(&ctx);
        self.apply_middle_autoscroll(&ctx);
        self.thumbnails.maintain(&ctx);
        self.thumbnails.begin_pass_for_frame(&ctx);
        self.avatars.maintain(&ctx);
        #[cfg(windows)]
        if self.selected.is_some() {
            let eof = self
                .playback
                .frame_snapshot
                .as_ref()
                .filter(|snap| snap.observed)
                .map_or_else(
                    || {
                        self.player
                            .as_ref()
                            .is_some_and(|player| player.flag_cstr(c"eof-reached") == Some(true))
                    },
                    |snap| snap.eof_reached == Some(true),
                );
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
                } else if has_live
                    && eof
                    && !self.playback.autoplay_eof_handled
                    && let Some((next_pos, next_video)) = self
                        .watch
                        .active_playlist
                        .as_ref()
                        .and_then(|pl| pl.peek_next())
                {
                    self.playback.autoplay_eof_handled = true;
                    if let Some(pl) = &mut self.watch.active_playlist {
                        pl.select_pos(next_pos);
                    }
                    let same_target = self
                        .selected
                        .as_ref()
                        .is_some_and(|cur| Self::same_video_target(&cur.url, &next_video.url));
                    if same_target {
                        if let Some(player) = &self.player {
                            let _ = player
                                .seek(0.0, "absolute+exact")
                                .and_then(|()| player.set("pause", "no"));
                        }
                        self.record_manual_seek(0.0, None, Instant::now());
                    } else {
                        self.playback.pending_open_keep_mini_player =
                            self.playback.in_app_mini_player;
                        self.playback.pending_open_keep_playlist = true;
                        self.playback.pending_open_suppress_back_stack = true;
                        self.playback.pending_open_video = Some(next_video);
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
            } else if self.feed.search_filters_open || self.feed.search_filters_modal_open {
                self.feed.search_filters_open = false;
                self.feed.search_filters_modal_open = false;
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
        let is_standard_watch = self.is_watch_page_active() && !self.is_watch_short_mode();
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
                is_standard_watch,
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
            if should_show_persistent_navigation_panel(
                viewport_width,
                self.ui.navigation_open,
                chromeless_video,
                is_standard_watch,
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
                is_standard_watch,
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
        let is_standard_watch_after = self.is_watch_page_active() && !self.is_watch_short_mode();
        if should_show_navigation_drawer(
            viewport_width,
            self.ui.navigation_open,
            chromeless_video,
            is_standard_watch_after,
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
            let border_color = if self.is_dark_theme() {
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
        self.handle_drag_and_drop(&ctx);
        self.sync_win11_runtime(&ctx);
        self.thumbnails.end_pass(&ctx);
        self.schedule_idle_aware_repaint(&ctx);
    }
}

fn spawn_deferred_font_loader(ctx: &egui::Context) -> Receiver<egui::FontDefinitions> {
    let (tx, rx) = mpsc::channel();
    let wake_ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("fasttube-fonts".to_owned())
        .spawn(move || {
            let rendering = fastframe_text::detect();
            let mut fonts = fastframe_fonts::FontSetup::default().definitions();
            rendering.apply_to(&mut fonts);
            let _ = tx.send(fonts);
            wake_ctx.request_repaint();
        });
    rx
}

#[cfg(not(test))]
struct StartupPrewarm {
    home_batch_rx: Receiver<(u64, HomeBatchFetchMessage)>,
    home_batch_cancel: process::Cancellation,
    wake_ctx: std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>,
    oauth_session: Option<oauth::TokenSession>,
    oauth_restore_error: Option<String>,
    youtube_web_session: Option<comment_actions::YouTubeWebSession>,
    cached_has_saved_profile: bool,
}

#[cfg(not(test))]
fn spawn_startup_prewarm() -> Option<std::thread::JoinHandle<StartupPrewarm>> {
    std::thread::Builder::new()
        .name("fasttube-prewarm".to_owned())
        .spawn(|| {
            let (oauth_session, discard_old_session, oauth_restore_error) =
                restore_session_state(oauth::load_session());
            if discard_old_session {
                let _ = oauth::delete_session();
            }
            let youtube_web_session = innertube::load_web_session()
                .ok()
                .flatten()
                .and_then(|s| playlist_sync::from_innertube_web_session(&s).ok());
            let cached_has_saved_profile =
                session::has_saved_profile() || youtube_web_session.is_some();
            let profile = UserProfile::default();
            let plan =
                RecommendationEngine::build_home_query_plan(&profile, &[], &FeedFilter::All, 0);
            let request = search::HomeBatchRequest {
                video_queries: plan.video_queries,
                channel_targets: plan.channel_targets,
                playlist_query: plan.playlist_query,
                shorts_query: plan.shorts_query,
            };
            let cancel = process::Cancellation::default();
            let (sender, receiver) = mpsc::channel();
            let wake_ctx: std::sync::Arc<std::sync::Mutex<Option<egui::Context>>> =
                std::sync::Arc::new(std::sync::Mutex::new(None));
            let wake_slot = std::sync::Arc::clone(&wake_ctx);
            let worker_cancel = cancel.clone();
            let _ = std::thread::Builder::new()
                .name("fasttube-home-prewarm".to_owned())
                .spawn(move || {
                    let result = search::fetch_home_batch(&request, &worker_cancel);
                    let _ = sender.send((
                        1,
                        HomeBatchFetchMessage {
                            request_id: 1,
                            page: 0,
                            result,
                        },
                    ));
                    if let Ok(guard) = wake_slot.lock()
                        && let Some(ctx) = guard.as_ref()
                    {
                        ctx.request_repaint();
                    }
                });
            StartupPrewarm {
                home_batch_rx: receiver,
                home_batch_cancel: cancel,
                wake_ctx,
                oauth_session,
                oauth_restore_error,
                youtube_web_session,
                cached_has_saved_profile,
            }
        })
        .ok()
}

fn main() -> eframe::Result {
    win11::set_process_app_user_model_id(win11::FASTTUBE_APP_USER_MODEL_ID);
    let mut instance = match desktop::claim_single_instance(std::env::args()) {
        desktop::SingleInstanceOutcome::Primary(handle) => Some(handle),
        desktop::SingleInstanceOutcome::Delegated => return Ok(()),
    };
    let start_hidden = desktop::has_hidden_launch_arg(std::env::args());

    // Pre-warm session restoration and initial Home feed fetch before window/GL creation.
    #[cfg(not(test))]
    let prewarm_cell = std::cell::RefCell::new(spawn_startup_prewarm());

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
            #[cfg(not(test))]
            let handle = prewarm_cell.borrow_mut().take();
            Box::new(move |creation| {
                let mut app = FastTube {
                    deferred_fonts_rx: Some(spawn_deferred_font_loader(&creation.egui_ctx)),
                    ..FastTube::default()
                };
                #[cfg(not(test))]
                if let Some(prewarm) = handle.and_then(|h| h.join().ok()) {
                    app.auth.cached_has_saved_profile = prewarm.cached_has_saved_profile;
                    if let Some(web_sess) = prewarm.youtube_web_session {
                        app.auth.youtube_web_session = Some(web_sess);
                        app.auth.youtube_web_session_checked = true;
                        app.auth.cached_has_saved_profile = true;
                    }
                    if app.auth.session.is_none() && prewarm.oauth_session.is_some() {
                        app.auth.authenticated = true;
                        app.auth.session = prewarm.oauth_session;
                    }
                    if app.message.is_empty()
                        && let Some(err) = prewarm.oauth_restore_error
                    {
                        app.message = err;
                    }
                    if let Ok(mut guard) = prewarm.wake_ctx.lock() {
                        *guard = Some(creation.egui_ctx.clone());
                    }
                    app.feed.home.batch_task.attach_receiver(
                        1,
                        prewarm.home_batch_cancel,
                        prewarm.home_batch_rx,
                    );
                    app.feed.home.has_more = true;
                    if !app.auth.authenticated {
                        app.feed.home.initial_public_started = true;
                    }
                    creation.egui_ctx.request_repaint();
                }
                app.init_win11_integrations(creation);
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
