#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;
mod diagnostics;
mod feed_prefs;
mod history;
mod layout;
mod library;
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
mod thumbnails;
mod transcript;
mod watch_extras;

use diagnostics::{DecodingDiagnostics, VideoSize, collect_from_property_reader};
use eframe::egui::{self, Color32, RichText};
use history::{Entry as HistoryEntry, History};
use layout::*;
use queue::{PlaybackEvent, Queue, QueueLoadResult, QueuePlaybackCorrelation};
use recommendations::{FeedFilter, RecommendationEngine, ScoredVideo, UserProfile};
use search::{ChannelPage, ChannelTarget, SearchResult, Video, VideoMetadata};
use settings::{BufferProfile, BufferSettings, PlaybackPreferences, Theme};
use std::cell::Cell;
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use thumbnails::ThumbnailCache;

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
    }
}

/// Largura máxima confortável para os cartões na visualização inicial de resultados.
const BROWSE_MAX_CONTENT_WIDTH: f32 = 1600.0;

/// Intervalo mínimo entre gravações periódicas do histórico em disco durante a reprodução.
const HISTORY_SAVE_INTERVAL: Duration = Duration::from_secs(5);

/// Variação mínima de posição (em ms) para justificar gravação periódica em disco.
const HISTORY_MIN_POSITION_DELTA_MS: u64 = 1_000;

/// Limite máximo de bytes para campos textuais do histórico local.
const HISTORY_MAX_TEXT_BYTES: usize = 4096;

/// Intervalo mínimo entre coletas de diagnóstico do player enquanto o painel de configurações está visível.
const DIAGNOSTICS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Política `hwdec` padrão configurada ao inicializar o `libmpv`.
const DEFAULT_CONFIGURED_HWDEC: &str = "auto-safe";

const DEFAULT_QUEUE_LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
struct DiagnosticsDisplay {
    status_line: String,
    configured_line: String,
    effective_line: String,
    metadata_line: Option<String>,
    summary_line: String,
    note_line: &'static str,
}

fn format_diagnostics_display(
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

fn update_decoding_diagnostics<F>(
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

fn buffer_profile_name(profile: BufferProfile) -> &'static str {
    match profile {
        BufferProfile::Small => "Pequeno",
        BufferProfile::Default => "Padrão",
        BufferProfile::Large => "Grande",
        BufferProfile::Custom(_) => "Personalizado",
    }
}

fn buffer_profile_summary(profile: BufferProfile) -> String {
    let values: BufferSettings = profile.values();
    format!(
        "{} ({} / {} MiB, {}s)",
        buffer_profile_name(profile),
        values.max_mib,
        values.back_mib,
        values.cache_seconds
    )
}

fn theme_name(theme: Theme) -> &'static str {
    match theme {
        Theme::Dark => "Escuro",
        Theme::Light => "Claro",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThemePalette {
    icon_fg: Color32,
    header_button_bg: Color32,
    header_button_fg: Color32,
    nav_button_bg: Color32,
    nav_button_fg: Color32,
    active_button_bg: Color32,
    active_button_fg: Color32,
    theater_inactive_bg: Color32,
    theater_inactive_fg: Color32,
    playing_nav_bg: Color32,
    playing_nav_fg: Color32,
    player_bar_bg: Color32,
    metadata_card_bg: Color32,
    channel_fg: Color32,
    description_fg: Color32,
    card_bg: Color32,
    card_stroke: Color32,
    card_alt_bg: Color32,
    card_alt_stroke: Color32,
    card_playing_bg: Color32,
    card_playing_stroke: Color32,
    playing_badge_fg: Color32,
    queue_next_bg: Color32,
    snippet_fg: Color32,
    resume_status_fg: Color32,
    watched_status_fg: Color32,
    neutral_status_fg: Color32,
    info_box_bg: Color32,
    info_box_stroke: Color32,
    info_accent_fg: Color32,
    profile_selected_bg: Color32,
    profile_selected_stroke: Color32,
    profile_unselected_bg: Color32,
    profile_unselected_stroke: Color32,
    warning_fg: Color32,
    error_fg: Color32,
    notice_bg: Color32,
    notice_stroke: Color32,
    notice_fg: Color32,
    error_bg: Color32,
    error_stroke: Color32,
    error_banner_fg: Color32,
    message_box_bg: Color32,
    diag_inactive_fg: Color32,
    diag_hw_fg: Color32,
    diag_sw_fg: Color32,
    diag_unknown_fg: Color32,
}

impl ThemePalette {
    fn for_theme(theme: Theme) -> Self {
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

fn visuals_for_theme(theme: Theme) -> egui::Visuals {
    let mut visuals = match theme {
        Theme::Dark => {
            let mut v = egui::Visuals::dark();
            v.panel_fill = Color32::from_rgb(15, 15, 15);
            v.window_fill = Color32::from_rgb(33, 33, 33);
            v.extreme_bg_color = Color32::from_rgb(18, 18, 18);
            v.faint_bg_color = Color32::from_rgb(33, 33, 33);
            v.selection.bg_fill = Color32::from_rgb(255, 0, 51);
            v.selection.stroke = egui::Stroke::new(1.5, Color32::WHITE);
            v.hyperlink_color = Color32::from_rgb(62, 166, 255);
            v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(120, 180, 255));
            v.widgets.active.bg_stroke = egui::Stroke::new(2.0, Color32::from_rgb(255, 92, 110));
            v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(241, 241, 241);
            v.widgets.inactive.bg_fill = Color32::from_rgb(39, 39, 39);
            v.widgets.inactive.fg_stroke.color = Color32::from_rgb(241, 241, 241);
            v.widgets.hovered.bg_fill = Color32::from_rgb(58, 58, 58);
            v.widgets.hovered.fg_stroke.color = Color32::WHITE;
            v.widgets.active.bg_fill = Color32::from_rgb(72, 72, 72);
            v.widgets.active.fg_stroke.color = Color32::WHITE;
            v.weak_text_color = Some(Color32::from_rgb(170, 170, 170));
            v
        }
        Theme::Light => {
            let mut v = egui::Visuals::light();
            v.panel_fill = Color32::WHITE;
            v.window_fill = Color32::WHITE;
            v.extreme_bg_color = Color32::from_rgb(249, 249, 249);
            v.faint_bg_color = Color32::from_rgb(242, 242, 242);
            v.selection.bg_fill = Color32::from_rgb(6, 95, 212);
            v.selection.stroke = egui::Stroke::new(1.5, Color32::WHITE);
            v.hyperlink_color = Color32::from_rgb(6, 95, 212);
            v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(30, 90, 170));
            v.widgets.active.bg_stroke = egui::Stroke::new(2.0, Color32::from_rgb(135, 20, 32));
            v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.widgets.inactive.bg_fill = Color32::from_rgb(242, 242, 242);
            v.widgets.inactive.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.widgets.hovered.bg_fill = Color32::from_rgb(229, 229, 229);
            v.widgets.hovered.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.widgets.active.bg_fill = Color32::from_rgb(217, 217, 217);
            v.widgets.active.fg_stroke.color = Color32::from_rgb(15, 15, 15);
            v.weak_text_color = Some(Color32::from_rgb(96, 96, 96));
            v
        }
    };
    fastframe_text::detect().apply_to_visuals(&mut visuals);
    visuals
}

fn apply_theme_visuals(ctx: &egui::Context, theme: Theme) {
    let visuals = visuals_for_theme(theme);
    let egui_theme = match theme {
        Theme::Dark => egui::Theme::Dark,
        Theme::Light => egui::Theme::Light,
    };
    ctx.set_theme(egui_theme);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size.y = 38.0;
        style.spacing.slider_width = 110.0;
    });
}

fn format_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            formatted.push('.');
        }
        formatted.push(ch);
    }
    formatted
}

fn format_view_count(count: u64) -> String {
    if count == 1 {
        "1 visualização".into()
    } else {
        format!("{} visualizações", format_thousands(count))
    }
}

fn format_upload_date(raw: &str) -> Option<String> {
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

fn clean_description(raw: Option<&str>) -> Option<String> {
    let text = raw?.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

fn description_snippet(raw: Option<&str>, max_chars: usize) -> Option<String> {
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

fn format_card_metadata_line(duration: &str, metadata: Option<&VideoMetadata>) -> Option<String> {
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

fn format_subscriber_count(count: u64) -> String {
    if count == 1 {
        "1 inscrito".into()
    } else {
        format!("{} inscritos", format_thousands(count))
    }
}

fn channel_target_display_label(target: &ChannelTarget) -> &str {
    match target {
        ChannelTarget::ChannelId(id) => id.as_str(),
        ChannelTarget::Handle(handle) => handle.as_str(),
    }
}

fn channel_page_display_title(page: &ChannelPage) -> &str {
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

fn format_channel_metadata_line(page: &ChannelPage) -> Option<String> {
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

fn format_channel_media_line(page: &ChannelPage) -> Option<String> {
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

fn parse_duration_ms(raw: &str) -> Option<u64> {
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

fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn sanitize_history_title(title: &str, video_id: &str) -> String {
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

fn sanitize_history_channel(channel: &str) -> String {
    truncate_utf8_bytes(channel.trim(), HISTORY_MAX_TEXT_BYTES)
}

fn format_history_status(entry: &HistoryEntry) -> String {
    if let Some(resume_ms) = entry.resume_position_ms() {
        let pos_text = format_time(resume_ms as f64 / 1000.0);
        if let Some(dur_ms) = entry.duration_ms {
            format!(
                "↺ Retomar em {pos_text} / {}",
                format_time(dur_ms as f64 / 1000.0)
            )
        } else {
            format!("↺ Retomar em {pos_text}")
        }
    } else if entry.is_near_end() && entry.position_ms > 0 {
        if let Some(dur_ms) = entry.duration_ms {
            format!(
                "✓ Assistido ({}) • Recomeça do início",
                format_time(dur_ms as f64 / 1000.0)
            )
        } else {
            "✓ Assistido • Recomeça do início".into()
        }
    } else if let Some(dur_ms) = entry.duration_ms {
        format!("Duração: {}", format_time(dur_ms as f64 / 1000.0))
    } else {
        "No histórico local".into()
    }
}

#[derive(Clone, Debug)]
struct ActivePlayback {
    video_id: Option<String>,
    url: String,
    title: String,
    channel: String,
    position_ms: u64,
    duration_ms: Option<u64>,
    resume_target_ms: Option<u64>,
    resume_seek_pending: bool,
    start_option_cleared: bool,
    has_live_sample: bool,
    stale_guard_pos_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ChannelTab {
    #[default]
    Videos,
    Playlists,
    About,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BrowseSection {
    #[default]
    Home,
    Explore,
    Subscriptions,
    Shorts,
    History,
    Queue,
    Library,
}

struct SearchFetchMessage {
    request_id: u64,
    result: Result<Vec<SearchResult>, String>,
}

struct ChannelFetchMessage {
    request_id: u64,
    target: ChannelTarget,
    result: Result<search::ChannelFetchOutcome, String>,
}

struct ChannelMoreVideosFetchMessage {
    request_id: u64,
    target: ChannelTarget,
    result: Result<search::ChannelVideosBatch, String>,
}

struct ChannelPlaylistsFetchMessage {
    request_id: u64,
    target: ChannelTarget,
    result: Result<Vec<search::ChannelPlaylist>, String>,
}

struct ChannelPlaylistVideosFetchMessage {
    request_id: u64,
    target: ChannelTarget,
    playlist_id: String,
    result: Result<Vec<SearchResult>, String>,
}

struct ChannelAboutFetchMessage {
    request_id: u64,
    target: ChannelTarget,
    result: Result<search::ChannelAboutInfo, String>,
}

enum HomeFetchMessage {
    Progress(oauth::SubscriptionProgress),
    Finished(
        Option<oauth::TokenSession>,
        Result<search::HomeFeed, String>,
    ),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BackupImportMode {
    Replace,
    #[default]
    Merge,
}

#[derive(Clone)]
struct ImportedUserData {
    library: library::Library,
    queue: Queue,
    history: History,
    playback_preferences: PlaybackPreferences,
    theme: Theme,
    buffer_profile: BufferProfile,
    feed: feed_prefs::FeedPreferences,
}

#[derive(Clone, Debug)]
struct BackupPreview {
    path: PathBuf,
    library_items: usize,
    favorites: usize,
    watch_later: usize,
    collections: usize,
    bookmarks: usize,
    queue_items: usize,
    history_entries: usize,
    merge_added: usize,
    merge_conflicts: usize,
    merge_skipped: usize,
    history_progress_updates: usize,
}

#[derive(Default)]
struct FeedDerivedCache {
    profile: UserProfile,
    chips: Vec<FeedFilter>,
    ranked: Vec<ScoredVideo>,
    generation: u64,
    filter: FeedFilter,
    history_len: usize,
    queue_len: usize,
    selected_url: Option<String>,
}

struct FastTube {
    query: String,
    videos: Vec<Video>,
    feed_data_generation: u64,
    video_metadata: HashMap<String, VideoMetadata>,
    selected_metadata: Option<VideoMetadata>,
    watch_extras: watch_extras::WatchExtras,
    thumbnails: ThumbnailCache,
    search: Option<Receiver<SearchFetchMessage>>,
    search_cancel: Option<process::Cancellation>,
    home_search: Option<Receiver<HomeFetchMessage>>,
    home_cancel: Option<process::Cancellation>,
    reauth: Option<Receiver<Result<oauth::TokenSession, String>>>,
    reauth_cancel: Option<process::Cancellation>,
    session_authenticated: bool,
    oauth_session: Option<oauth::TokenSession>,
    video_ratings: HashMap<String, oauth::VideoRating>,
    rating_request: Option<Receiver<Result<(String, Option<oauth::VideoRating>), String>>>,
    rating_cancel: Option<process::Cancellation>,
    oauth_restore_attempted: bool,
    initial_public_home_started: bool,
    home_playlist_urls: Vec<String>,
    home_title: Option<String>,
    home_limit_reached: bool,
    subscription_channels: Vec<search::HomeChannel>,
    subscription_channels_loaded: bool,
    subscription_channels_error: Option<String>,
    pending_home_items: Vec<SearchResult>,
    search_request_id: u64,
    active_search_request_id: Option<u64>,
    channel_page: Option<ChannelPage>,
    channel_view_open: bool,
    channel_tab: ChannelTab,
    channel_search: Option<Receiver<ChannelFetchMessage>>,
    channel_cancel: Option<process::Cancellation>,
    channel_request_id: u64,
    active_channel_request_id: Option<u64>,
    channel_loading_target: Option<ChannelTarget>,
    channel_last_target: Option<ChannelTarget>,
    channel_error: Option<String>,
    channel_video_limit: usize,
    channel_from_cache: bool,
    channel_more_videos_search: Option<Receiver<ChannelMoreVideosFetchMessage>>,
    channel_more_videos_cancel: Option<process::Cancellation>,
    active_channel_more_videos_request_id: Option<u64>,
    channel_more_videos_error: Option<String>,
    channel_videos_has_more: bool,
    channel_videos_reached_max: bool,
    channel_playlists: Vec<search::ChannelPlaylist>,
    channel_playlists_loaded: bool,
    channel_playlists_search: Option<Receiver<ChannelPlaylistsFetchMessage>>,
    channel_playlists_cancel: Option<process::Cancellation>,
    active_channel_playlists_request_id: Option<u64>,
    channel_playlists_error: Option<String>,
    channel_selected_playlist: Option<search::ChannelPlaylist>,
    channel_playlist_videos: Vec<SearchResult>,
    channel_playlist_videos_loaded: bool,
    channel_playlist_videos_search: Option<Receiver<ChannelPlaylistVideosFetchMessage>>,
    channel_playlist_videos_cancel: Option<process::Cancellation>,
    active_channel_playlist_videos_request_id: Option<u64>,
    channel_playlist_videos_error: Option<String>,
    channel_about: Option<search::ChannelAboutInfo>,
    channel_about_loaded: bool,
    channel_about_search: Option<Receiver<ChannelAboutFetchMessage>>,
    channel_about_cancel: Option<process::Cancellation>,
    active_channel_about_request_id: Option<u64>,
    channel_about_error: Option<String>,
    message: String,
    selected: Option<Video>,
    scrolling: fastframe_scroll::Scrolling,
    fullscreen: bool,
    theater_mode: bool,
    navigation_open: bool,
    search_results_mode: bool,
    settings_open: bool,
    shortcuts_open: bool,
    video_window_visible: bool,
    buffer_profile: BufferProfile,
    playback_preferences: PlaybackPreferences,
    playback_settings_path: Option<PathBuf>,
    loaded_playback_settings_path: Option<PathBuf>,
    playback_settings_error: Option<String>,
    theme: Theme,
    theme_settings_path: Option<PathBuf>,
    loaded_theme_settings_path: Option<PathBuf>,
    theme_settings_error: Option<String>,
    applied_theme: Option<Theme>,
    active_player_buffer: Option<BufferProfile>,
    settings_path: Option<PathBuf>,
    settings_error: Option<String>,
    settings_notice: Option<String>,
    history: History,
    history_path: Option<PathBuf>,
    loaded_history_path: Option<PathBuf>,
    history_read_failed: bool,
    history_error: Option<String>,
    history_notice: Option<String>,
    history_dirty: bool,
    library: library::Library,
    library_path: Option<PathBuf>,
    library_error: Option<String>,
    mini_player: bool,
    mini_player_restore_size: Option<egui::Vec2>,
    bookmark_note: String,
    collection_name: String,
    feed_hide_watched: bool,
    feed_hide_shorts: bool,
    feed_hide_lives: bool,
    feed_newest_first: bool,
    feed_channel: Option<String>,
    feed_prefs_path: Option<PathBuf>,
    backup_path_text: String,
    backup_status: Option<String>,
    backup_confirm_import: bool,
    backup_import_mode: BackupImportMode,
    backup_preview: Option<BackupPreview>,
    pending_import: Option<ImportedUserData>,
    last_history_save_at: Option<Instant>,
    last_saved_position_ms: Option<u64>,
    last_saved_duration_ms: Option<Option<u64>>,
    active_playback: Option<ActivePlayback>,
    quality: Option<u32>,
    qualities: Vec<u32>,
    quality_search: Option<Receiver<Result<Vec<u32>, String>>>,
    quality_cancel: Option<process::Cancellation>,
    quality_error: Option<String>,
    seek_drag: Option<f64>,
    overlay_last_activity: Instant,
    decoding_diagnostics: DecodingDiagnostics,
    last_diagnostics_poll_at: Option<Instant>,
    queue: Queue,
    queue_path: Option<PathBuf>,
    queue_read_failed: bool,
    queue_persistence_error: Option<String>,
    queue_eof_handled: bool,
    queue_error: Option<String>,
    queue_notice: Option<String>,
    last_rejected_video: Option<Video>,
    pending_queue_video: Option<Video>,
    pending_queue_entry_id: Option<u64>,
    queue_correlation: QueuePlaybackCorrelation,
    pending_queue_advance: bool,
    in_app_ui_pass: bool,
    feed_filter: FeedFilter,
    feed_cache_dirty: bool,
    feed_cache: FeedDerivedCache,
    sidebar_subscriptions_expanded: bool,
    browse_section: BrowseSection,
    #[cfg(windows)]
    player: Option<player::Player>,
}

fn restore_session_state(
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
        Self {
            query: String::new(),
            videos: Vec::new(),
            feed_data_generation: 0,
            video_metadata: HashMap::new(),
            selected_metadata: None,
            watch_extras: watch_extras::WatchExtras::default(),
            thumbnails: ThumbnailCache::default(),
            search: None,
            search_cancel: None,
            home_search: None,
            home_cancel: None,
            reauth: None,
            reauth_cancel: None,
            session_authenticated,
            oauth_session,
            video_ratings: HashMap::new(),
            rating_request: None,
            rating_cancel: None,
            oauth_restore_attempted: false,
            initial_public_home_started: false,
            home_playlist_urls: Vec::new(),
            home_title: None,
            home_limit_reached: false,
            subscription_channels: Vec::new(),
            subscription_channels_loaded: false,
            subscription_channels_error: None,
            pending_home_items: Vec::new(),
            search_request_id: 0,
            active_search_request_id: None,
            channel_page: None,
            channel_view_open: false,
            channel_tab: ChannelTab::Videos,
            channel_search: None,
            channel_cancel: None,
            channel_request_id: 0,
            active_channel_request_id: None,
            channel_loading_target: None,
            channel_last_target: None,
            channel_error: None,
            channel_video_limit: search::DEFAULT_CHANNEL_VIDEOS_LIMIT,
            channel_from_cache: false,
            channel_more_videos_search: None,
            channel_more_videos_cancel: None,
            active_channel_more_videos_request_id: None,
            channel_more_videos_error: None,
            channel_videos_has_more: false,
            channel_videos_reached_max: false,
            channel_playlists: Vec::new(),
            channel_playlists_loaded: false,
            channel_playlists_search: None,
            channel_playlists_cancel: None,
            active_channel_playlists_request_id: None,
            channel_playlists_error: None,
            channel_selected_playlist: None,
            channel_playlist_videos: Vec::new(),
            channel_playlist_videos_loaded: false,
            channel_playlist_videos_search: None,
            channel_playlist_videos_cancel: None,
            active_channel_playlist_videos_request_id: None,
            channel_playlist_videos_error: None,
            channel_about: None,
            channel_about_loaded: false,
            channel_about_search: None,
            channel_about_cancel: None,
            active_channel_about_request_id: None,
            channel_about_error: None,
            message: oauth_restore_error
                .unwrap_or_else(|| "Pesquise um video ou cole um link do YouTube.".into()),
            selected: None,
            scrolling: fastframe_scroll::Scrolling::default(),
            fullscreen: false,
            theater_mode: false,
            navigation_open: false,
            search_results_mode: false,
            settings_open: false,
            shortcuts_open: false,
            video_window_visible: false,
            buffer_profile,
            playback_preferences,
            playback_settings_path,
            loaded_playback_settings_path,
            playback_settings_error,
            theme,
            theme_settings_path,
            loaded_theme_settings_path,
            theme_settings_error,
            applied_theme: None,
            active_player_buffer: None,
            settings_path,
            settings_error,
            settings_notice: None,
            history,
            history_path,
            loaded_history_path,
            history_read_failed,
            history_error,
            history_notice: None,
            history_dirty: false,
            library,
            library_path,
            library_error,
            mini_player: false,
            mini_player_restore_size: None,
            bookmark_note: String::new(),
            collection_name: String::new(),
            feed_hide_watched: feed_prefs.hide_watched,
            feed_hide_shorts: feed_prefs.hide_shorts,
            feed_hide_lives: feed_prefs.hide_lives,
            feed_newest_first: feed_prefs.newest_first,
            feed_channel: feed_prefs.channel,
            feed_prefs_path,
            backup_path_text: String::new(),
            backup_status: initial_backup_status,
            backup_confirm_import: false,
            backup_import_mode: BackupImportMode::default(),
            backup_preview: None,
            pending_import: None,
            last_history_save_at: None,
            last_saved_position_ms: None,
            last_saved_duration_ms: None,
            active_playback: None,
            quality: None,
            qualities: Vec::new(),
            quality_search: None,
            quality_cancel: None,
            quality_error: None,
            seek_drag: None,
            overlay_last_activity: Instant::now(),
            decoding_diagnostics: DecodingDiagnostics::unknown(Some(DEFAULT_CONFIGURED_HWDEC)),
            last_diagnostics_poll_at: None,
            queue,
            queue_path,
            queue_read_failed,
            queue_eof_handled: false,
            queue_persistence_error: queue_error,
            queue_error: None,
            queue_notice: None,
            last_rejected_video: None,
            pending_queue_video: None,
            pending_queue_entry_id: None,
            queue_correlation: QueuePlaybackCorrelation::default(),
            pending_queue_advance: false,
            in_app_ui_pass: false,
            feed_filter: FeedFilter::All,
            feed_cache_dirty: true,
            feed_cache: FeedDerivedCache::default(),
            sidebar_subscriptions_expanded: false,
            browse_section: BrowseSection::Home,
            #[cfg(windows)]
            player: None,
        }
    }
}

impl FastTube {
    fn default_user_data_paths() -> io::Result<Vec<(&'static str, PathBuf)>> {
        let history_path = history::history_path()?;
        Ok(vec![
            ("library", library::path()?),
            ("queue", history_path.with_file_name("queue.json")),
            ("history", history_path),
            ("playback", settings::playback_path()?),
            ("theme", settings::theme_path()?),
            ("feed", feed_prefs::path()?),
            ("buffer", settings::settings_path()?),
        ])
    }

    fn recover_interrupted_import() -> Option<String> {
        let paths = Self::default_user_data_paths().ok()?;
        let root = paths.first()?.1.parent()?.to_path_buf();
        let refs: Vec<_> = paths
            .iter()
            .map(|(name, path)| (*name, path.as_path()))
            .collect();
        match backup::recover_pending_import(&root, &refs) {
            Ok(true) => Some(
                "Uma importaÃ§Ã£o interrompida foi desfeita antes da abertura; os dados anteriores foram restaurados."
                    .into(),
            ),
            Ok(false) => None,
            Err(error) => Some(format!(
                "Foi encontrado um ponto de restauraÃ§Ã£o de importaÃ§Ã£o, mas ele nÃ£o pÃ´de ser aplicado: {error}"
            )),
        }
    }

    fn persist_queue(&mut self) {
        if self.queue_read_failed {
            return;
        }
        if let Some(path) = &self.queue_path {
            if let Err(error) = self.queue.save(path) {
                self.queue_persistence_error =
                    Some(format!("Não foi possível salvar a fila: {error}"));
            } else {
                self.queue_persistence_error = None;
            }
        }
    }
    fn load_initial_settings() -> (BufferProfile, Option<PathBuf>, Option<String>) {
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

    fn load_settings_from_path(path: &Path) -> Result<BufferProfile, String> {
        settings::load(path).map_err(|error| {
            format!(
                "Erro ao ler as configurações em '{}' ({error}). Usando o perfil Padrão (64/16 MiB, 30s).",
                path.display()
            )
        })
    }

    fn load_initial_playback_settings() -> (PlaybackPreferences, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (PlaybackPreferences::default(), None, None);
        }
        match settings::playback_path() {
            Ok(path) => match Self::load_playback_settings_from_path(&path) {
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

    fn load_playback_settings_from_path(path: &Path) -> Result<PlaybackPreferences, String> {
        settings::load_playback(path).map_err(|error| {
            format!(
                "Erro ao ler preferências de reprodução em '{}' ({error}); usando 1× e qualidade automática.",
                path.display()
            )
        })
    }

    fn ensure_playback_settings_synchronized(&mut self) {
        if self.loaded_playback_settings_path == self.playback_settings_path {
            return;
        }
        let had_previous_path = self.loaded_playback_settings_path.is_some();
        self.loaded_playback_settings_path = self.playback_settings_path.clone();
        let Some(path) = self.playback_settings_path.clone() else {
            return;
        };
        match Self::load_playback_settings_from_path(&path) {
            Ok(loaded) => {
                if had_previous_path || self.playback_preferences == PlaybackPreferences::default()
                {
                    self.playback_preferences = loaded;
                }
                self.playback_settings_error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.playback_preferences = PlaybackPreferences::default();
                }
                self.playback_settings_error = Some(error);
            }
        }
    }

    fn apply_playback_preferences(&mut self, preferences: PlaybackPreferences) {
        self.ensure_playback_settings_synchronized();
        self.playback_preferences = preferences;
        let save_result = match self.playback_settings_path.clone().ok_or_else(|| {
            if cfg!(test) {
                Err("Caminho de preferências de reprodução não configurado no teste".to_owned())
            } else {
                settings::playback_path().map_err(|error| error.to_string())
            }
        }) {
            Ok(path) => settings::save_playback(&path, preferences).map_err(|error| {
                format!(
                    "Não foi possível salvar preferências de reprodução em '{}' ({error}); a escolha vale apenas nesta sessão.",
                    path.display()
                )
            }),
            Err(Ok(path)) => {
                let res = settings::save_playback(&path, preferences).map_err(|error| {
                    format!(
                        "Não foi possível salvar preferências de reprodução em '{}' ({error}); a escolha vale apenas nesta sessão.",
                        path.display()
                    )
                });
                self.playback_settings_path = Some(path.clone());
                self.loaded_playback_settings_path = Some(path);
                res
            }
            Err(Err(error)) => Err(format!(
                "Não foi possível salvar preferências de reprodução ({error}); a escolha vale apenas nesta sessão."
            )),
        };
        self.playback_settings_error = save_result.err();
    }

    fn load_initial_theme_settings() -> (Theme, Option<PathBuf>, Option<String>) {
        if cfg!(test) {
            return (Theme::default(), None, None);
        }
        match settings::theme_path() {
            Ok(path) => match Self::load_theme_settings_from_path(&path) {
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

    fn load_theme_settings_from_path(path: &Path) -> Result<Theme, String> {
        settings::load_theme(path).map_err(|error| {
            format!(
                "Erro ao ler tema da interface em '{}' ({error}); usando o tema Escuro padrão.",
                path.display()
            )
        })
    }

    fn ensure_theme_settings_synchronized(&mut self) {
        if self.loaded_theme_settings_path == self.theme_settings_path {
            return;
        }
        let had_previous_path = self.loaded_theme_settings_path.is_some();
        self.loaded_theme_settings_path = self.theme_settings_path.clone();
        let Some(path) = self.theme_settings_path.clone() else {
            return;
        };
        match Self::load_theme_settings_from_path(&path) {
            Ok(loaded) => {
                if had_previous_path || self.theme == Theme::default() {
                    self.theme = loaded;
                    self.applied_theme = None;
                }
                self.theme_settings_error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.theme = Theme::default();
                    self.applied_theme = None;
                }
                self.theme_settings_error = Some(error);
            }
        }
    }

    fn sync_theme_visuals(&mut self, ctx: &egui::Context) {
        self.ensure_theme_settings_synchronized();
        let want_dark = self.theme == Theme::Dark;
        if self.applied_theme != Some(self.theme)
            || ctx.global_style().visuals.dark_mode != want_dark
        {
            apply_theme_visuals(ctx, self.theme);
            self.applied_theme = Some(self.theme);
        }
    }

    fn apply_theme(&mut self, theme: Theme) {
        self.ensure_theme_settings_synchronized();
        self.theme = theme;
        self.applied_theme = None;
        let save_result = match self.theme_settings_path.clone().ok_or_else(|| {
            if cfg!(test) {
                Err("Caminho de configurações de tema não configurado no teste".to_owned())
            } else {
                settings::theme_path().map_err(|error| error.to_string())
            }
        }) {
            Ok(path) => settings::save_theme(&path, theme).map_err(|error| {
                format!(
                    "Não foi possível salvar o tema em '{}' ({error}); a escolha vale apenas nesta sessão.",
                    path.display()
                )
            }),
            Err(Ok(path)) => {
                let res = settings::save_theme(&path, theme).map_err(|error| {
                    format!(
                        "Não foi possível salvar o tema em '{}' ({error}); a escolha vale apenas nesta sessão.",
                        path.display()
                    )
                });
                self.theme_settings_path = Some(path.clone());
                self.loaded_theme_settings_path = Some(path);
                res
            }
            Err(Err(error)) => Err(format!(
                "Não foi possível salvar o tema ({error}); a escolha vale apenas nesta sessão."
            )),
        };
        self.theme_settings_error = save_result.err();
    }

    fn palette(&self) -> ThemePalette {
        ThemePalette::for_theme(self.theme)
    }

    fn overlay_style(&self) -> overlay::OverlayStyle {
        let palette = self.palette();
        let (panel, accent) = match self.theme {
            Theme::Dark => (
                Color32::from_rgba_unmultiplied(16, 16, 20, 232),
                Color32::from_rgb(255, 70, 70),
            ),
            Theme::Light => (
                Color32::from_rgba_unmultiplied(250, 250, 252, 238),
                Color32::from_rgb(176, 24, 34),
            ),
        };
        overlay::OverlayStyle {
            panel,
            icon: palette.icon_fg,
            text: palette.description_fg,
            accent,
        }
    }

    fn load_initial_history() -> (History, Option<PathBuf>, bool, Option<String>) {
        match history::history_path() {
            Ok(path) => match Self::load_history_from_path(&path) {
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

    fn load_history_from_path(path: &Path) -> Result<History, String> {
        history::load(path).map_err(|error| {
            format!(
                "Erro ao ler o histórico local em '{}' ({error}). O arquivo existente foi preservado sem sobrescrita.",
                path.display()
            )
        })
    }

    fn ensure_history_synchronized(&mut self) {
        if self.loaded_history_path == self.history_path {
            return;
        }
        let had_previous_path = self.loaded_history_path.is_some();
        self.loaded_history_path = self.history_path.clone();
        let Some(path) = self.history_path.clone() else {
            self.history_read_failed = false;
            return;
        };
        match Self::load_history_from_path(&path) {
            Ok(loaded) => {
                if had_previous_path || self.history.entries().is_empty() {
                    self.history = loaded;
                    self.invalidate_feed_cache();
                }
                self.history_read_failed = false;
                self.history_error = None;
            }
            Err(error) => {
                if had_previous_path {
                    self.history = History::default();
                    self.invalidate_feed_cache();
                }
                self.history_read_failed = true;
                self.history_error = Some(error);
            }
        }
    }

    fn save_history_to_disk(&mut self, now: Instant) -> bool {
        self.ensure_history_synchronized();
        if self.history_read_failed {
            return false;
        }
        let path = match self.history_path.clone().ok_or_else(|| {
            history::history_path().map_err(|error| {
                format!("Não foi possível determinar o caminho para salvar o histórico: {error}")
            })
        }) {
            Ok(path) => path,
            Err(Ok(path)) => {
                let mut loaded = match Self::load_history_from_path(&path) {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        self.history_path = Some(path.clone());
                        self.loaded_history_path = Some(path);
                        self.history_read_failed = true;
                        self.history_error = Some(error);
                        return false;
                    }
                };
                for entry in self.history.entries().iter().rev() {
                    let _ = loaded.record(entry.clone());
                }
                self.history = loaded;
                self.history_path = Some(path.clone());
                self.loaded_history_path = Some(path.clone());
                path
            }
            Err(Err(message)) => {
                self.history_error = Some(message);
                return false;
            }
        };
        if let Err(error) = Self::load_history_from_path(&path) {
            self.history_read_failed = true;
            self.history_error = Some(error);
            return false;
        }
        match history::save(&path, &self.history) {
            Ok(()) => {
                self.history_error = None;
                self.history_dirty = false;
                self.last_history_save_at = Some(now);
                if let Some(active) = &self.active_playback {
                    self.last_saved_position_ms = Some(active.position_ms);
                    self.last_saved_duration_ms = Some(active.duration_ms);
                }
                true
            }
            Err(error) => {
                self.history_error = Some(format!(
                    "Não foi possível salvar o histórico em '{}': {error}",
                    path.display()
                ));
                false
            }
        }
    }

    fn planned_resume_ms(&mut self, video: &Video) -> Option<u64> {
        self.ensure_history_synchronized();
        let video_id = history::youtube_video_id(&video.url)?;
        let fallback_duration_ms = parse_duration_ms(&video.duration);
        let entry = self.history.get(&video_id)?;
        if entry.duration_ms.is_some() {
            return self.history.resume_position_ms(&video_id);
        }
        history::should_resume(entry.position_ms, fallback_duration_ms)
    }

    fn record_active_playback_in_memory(&mut self) -> bool {
        let Some(active) = &mut self.active_playback else {
            return false;
        };
        if !active.has_live_sample {
            return false;
        }
        let Some(video_id) = active.video_id.clone() else {
            return false;
        };
        if let Some(duration_ms) = active.duration_ms {
            if duration_ms == 0 {
                active.duration_ms = None;
            } else if active.position_ms > duration_ms {
                active.position_ms = duration_ms;
            }
        }
        let entry = HistoryEntry {
            video_id: video_id.clone(),
            title: sanitize_history_title(&active.title, &video_id),
            channel: sanitize_history_channel(&active.channel),
            position_ms: active.position_ms,
            duration_ms: active.duration_ms,
        };
        if self.history.entries().first() == Some(&entry) {
            return false;
        }
        if self.history.record(entry).is_ok() {
            self.history_dirty = true;
            self.invalidate_feed_cache();
            true
        } else {
            false
        }
    }

    fn should_persist_playback_now(&self, now: Instant, force: bool) -> bool {
        if !self.history_dirty || self.history_read_failed {
            return false;
        }
        if force {
            return true;
        }
        let Some(last_save) = self.last_history_save_at else {
            return true;
        };
        if now.saturating_duration_since(last_save) < HISTORY_SAVE_INTERVAL {
            return false;
        }
        let Some(active) = &self.active_playback else {
            return true;
        };
        let pos_changed = self.last_saved_position_ms.is_none_or(|saved| {
            active.position_ms.abs_diff(saved) >= HISTORY_MIN_POSITION_DELTA_MS
        });
        let dur_changed = self
            .last_saved_duration_ms
            .is_none_or(|saved| saved != active.duration_ms);
        pos_changed || dur_changed
    }

    fn flush_playback_history_at(&mut self, now: Instant) {
        self.ensure_history_synchronized();
        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, true) {
            self.save_history_to_disk(now);
        }
    }

    fn activate_video_playback(
        &mut self,
        video: &Video,
        resume_target_ms: Option<u64>,
        previous_player_pos_ms: Option<u64>,
        now: Instant,
    ) {
        self.ensure_history_synchronized();
        let video_id = history::youtube_video_id(&video.url);
        let fallback_duration_ms = parse_duration_ms(&video.duration);
        let existing_duration_ms = video_id
            .as_deref()
            .and_then(|id| self.history.get(id))
            .and_then(|entry| entry.duration_ms);
        let duration_ms = existing_duration_ms.or(fallback_duration_ms);
        let initial_position_ms = resume_target_ms.unwrap_or(0);
        self.active_playback = Some(ActivePlayback {
            video_id: video_id.clone(),
            url: video.url.clone(),
            title: video.title.clone(),
            channel: video.channel.clone(),
            position_ms: initial_position_ms,
            duration_ms,
            resume_target_ms,
            resume_seek_pending: resume_target_ms.is_some(),
            start_option_cleared: false,
            has_live_sample: false,
            stale_guard_pos_ms: previous_player_pos_ms,
        });
        self.last_saved_position_ms = None;
        self.last_saved_duration_ms = None;
        if video_id.is_some() {
            self.history_notice = None;
        }
        let _ = now;
    }

    fn current_player_live_pos_ms(&self) -> Option<u64> {
        self.active_playback
            .as_ref()
            .filter(|active| active.has_live_sample)
            .map(|active| active.position_ms)
    }

    #[cfg(windows)]
    fn sample_player_before_transition(&mut self, now: Instant) {
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

    fn process_player_telemetry(
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
            .active_playback
            .as_ref()
            .is_none_or(|active| active.url != video.url)
        {
            let prev_pos = self
                .active_playback
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

        let Some(active) = &mut self.active_playback else {
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

        if !active.has_live_sample {
            if let Some(stale_ms) = active.stale_guard_pos_ms {
                let expected_ms = active.resume_target_ms.unwrap_or(0);
                if stale_ms.abs_diff(expected_ms) > 2_000
                    && sample_pos_ms.abs_diff(stale_ms) <= 1_500
                {
                    return (
                        active.position_ms as f64 / 1000.0,
                        active.duration_ms.map(|dur| dur as f64 / 1000.0),
                        None,
                        false,
                    );
                }
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
        if active.resume_seek_pending {
            active.resume_seek_pending = false;
            if let Some(target_ms) = active.resume_target_ms {
                if history::should_resume(target_ms, active.duration_ms).is_none() {
                    active.resume_target_ms = None;
                    sample_pos_ms = 0;
                    if pos_secs > 1.0 {
                        pending_seek_secs = Some(0.0);
                    }
                } else if sample_pos_ms + 1_000 < target_ms {
                    pending_seek_secs = Some(target_ms as f64 / 1000.0);
                }
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

        if ended {
            if let Some(dur_ms) = active.duration_ms {
                sample_pos_ms = sample_pos_ms.max(dur_ms);
            }
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

    fn record_manual_seek(&mut self, position_secs: f64, duration_secs: Option<f64>, now: Instant) {
        if !position_secs.is_finite() {
            return;
        }
        let Some(active) = &mut self.active_playback else {
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
        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, true) {
            self.save_history_to_disk(now);
        }
    }

    fn record_video_in_history(
        &mut self,
        video: &Video,
        position_ms: u64,
        duration_ms: Option<u64>,
    ) -> io::Result<()> {
        self.ensure_history_synchronized();
        let video_id = history::youtube_video_id(&video.url).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "apenas URLs de vídeo do YouTube com ID validado são aceitas no histórico",
            )
        })?;
        let duration_ms = duration_ms
            .or_else(|| parse_duration_ms(&video.duration))
            .filter(|&dur| dur > 0);
        let position_ms = duration_ms
            .map(|dur| position_ms.min(dur))
            .unwrap_or(position_ms);
        let entry = HistoryEntry {
            video_id: video_id.clone(),
            title: sanitize_history_title(&video.title, &video_id),
            channel: sanitize_history_channel(&video.channel),
            position_ms,
            duration_ms,
        };
        self.history.record(entry)?;
        self.history_dirty = true;
        self.invalidate_feed_cache();
        Ok(())
    }

    fn clear_history(&mut self) {
        self.ensure_history_synchronized();
        self.history.clear();
        self.invalidate_feed_cache();
        self.history_dirty = false;
        self.last_saved_position_ms = None;
        self.last_saved_duration_ms = None;
        if let Some(active) = &mut self.active_playback {
            active.resume_target_ms = None;
            active.resume_seek_pending = false;
        }
        if self.history_read_failed {
            self.history_notice = Some(
                "Histórico da sessão limpo. O arquivo corrompido em disco foi preservado sem sobrescrita."
                    .into(),
            );
            return;
        }
        if self.history_path.is_some() {
            if self.save_history_to_disk(Instant::now()) {
                self.history_notice = Some("Histórico local limpo.".into());
            }
        } else {
            self.history_notice = Some("Histórico local limpo.".into());
        }
    }

    fn remove_history_entry(&mut self, video_id: &str) -> bool {
        self.ensure_history_synchronized();
        if !self.history.remove(video_id) {
            return false;
        }
        self.invalidate_feed_cache();
        if !self.history_read_failed && self.history_path.is_some() {
            self.save_history_to_disk(Instant::now());
        }
        true
    }

    fn video_for_history_entry(&self, entry: &HistoryEntry) -> Option<Video> {
        let canonical_url = history::youtube_watch_url(&entry.video_id)?;
        if let Some(existing) = self.videos.iter().find(|video| {
            history::youtube_video_id(&video.url).as_deref() == Some(entry.video_id.as_str())
        }) {
            return Some(existing.clone());
        }
        if let Some(channel_video) = self.channel_page.as_ref().and_then(|page| {
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
            .channel_playlist_videos
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

    fn apply_buffer_profile(&mut self, profile: BufferProfile) {
        self.buffer_profile = profile;
        let save_result = match self.settings_path.clone().ok_or_else(|| {
            settings::settings_path().map_err(|error| {
                format!("Não foi possível determinar o caminho para salvar configurações: {error}")
            })
        }) {
            Ok(path) => settings::save(&path, profile).map_err(|error| {
                format!(
                    "Não foi possível salvar as configurações em '{}': {error}",
                    path.display()
                )
            }),
            Err(Ok(path)) => {
                let res = settings::save(&path, profile).map_err(|error| {
                    format!(
                        "Não foi possível salvar as configurações em '{}': {error}",
                        path.display()
                    )
                });
                self.settings_path = Some(path);
                res
            }
            Err(Err(message)) => Err(message),
        };

        let pending_restart = self.has_pending_buffer_restart();
        match save_result {
            Ok(()) => {
                self.invalidate_feed_cache();
                self.settings_error = None;
                self.settings_notice = Some(if pending_restart {
                    format!(
                        "Perfil {} salvo. Como o player já está ativo, a alteração entrará em vigor após fechar o vídeo atual (reiniciar o player) e abrir o próximo vídeo.",
                        buffer_profile_summary(profile)
                    )
                } else {
                    format!(
                        "Perfil {} salvo e pronto para ser aplicado ao iniciar o player no próximo vídeo.",
                        buffer_profile_summary(profile)
                    )
                });
            }
            Err(error) => {
                self.settings_error = Some(format!(
                    "{error} O perfil {} permanece selecionado apenas nesta sessão.",
                    buffer_profile_name(profile)
                ));
                self.settings_notice = if pending_restart {
                    Some(
                        "O player já está ativo: feche o vídeo atual (reinicie o player) para aplicar o novo perfil na sessão."
                            .into(),
                    )
                } else {
                    None
                };
            }
        }
    }

    fn has_pending_buffer_restart(&self) -> bool {
        self.active_player_buffer
            .is_some_and(|active| active.values() != self.buffer_profile.values())
    }

    fn reset_decoding_diagnostics(&mut self) {
        self.decoding_diagnostics = DecodingDiagnostics::unknown(Some(DEFAULT_CONFIGURED_HWDEC));
        self.last_diagnostics_poll_at = None;
    }

    #[cfg(test)]
    fn refresh_decoding_diagnostics_with<F>(
        &mut self,
        now: Instant,
        has_active_video: bool,
        read_property: F,
    ) -> bool
    where
        F: FnMut(&str) -> Option<String>,
    {
        update_decoding_diagnostics(
            &mut self.decoding_diagnostics,
            &mut self.last_diagnostics_poll_at,
            self.settings_open,
            has_active_video,
            now,
            read_property,
        )
    }

    fn refresh_player_diagnostics(&mut self, now: Instant) -> bool {
        if self.selected.is_none() {
            self.reset_decoding_diagnostics();
            return false;
        }
        #[cfg(windows)]
        if let Some(player) = &self.player {
            return update_decoding_diagnostics(
                &mut self.decoding_diagnostics,
                &mut self.last_diagnostics_poll_at,
                self.settings_open,
                true,
                now,
                |name| player.property(name),
            );
        }
        let _ = now;
        false
    }

    fn should_show_video_window(&self) -> bool {
        should_show_video_window(self.selected.is_some(), self.settings_open)
    }

    fn sync_player_window_visibility(&mut self) {
        let visible = self.should_show_video_window();
        self.video_window_visible = visible;
        #[cfg(windows)]
        if let Some(player) = &mut self.player {
            player.set_visible(visible);
        }
    }

    fn metadata_for(&self, url: &str) -> Option<&VideoMetadata> {
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
                self.channel_page.as_ref().and_then(|page| {
                    page.videos
                        .iter()
                        .find(|item| item.video.url == url)
                        .map(|item| &item.metadata)
                })
            })
            .or_else(|| {
                self.channel_playlist_videos
                    .iter()
                    .find(|item| item.video.url == url)
                    .map(|item| &item.metadata)
            })
    }

    fn channel_target_for_video(&self, video: &Video) -> Option<ChannelTarget> {
        self.metadata_for(&video.url)
            .and_then(search::channel_target_for_metadata)
    }

    fn is_channel_loading(&self) -> bool {
        self.active_channel_request_id.is_some()
    }

    fn is_channel_more_videos_loading(&self) -> bool {
        self.active_channel_more_videos_request_id.is_some()
    }

    fn is_channel_playlists_loading(&self) -> bool {
        self.active_channel_playlists_request_id.is_some()
    }

    fn is_channel_playlist_videos_loading(&self) -> bool {
        self.active_channel_playlist_videos_request_id.is_some()
    }

    fn is_channel_about_loading(&self) -> bool {
        self.active_channel_about_request_id.is_some()
    }

    fn cancel_channel_subrequests(&mut self) {
        if let Some(cancel) = self.channel_more_videos_cancel.take() {
            cancel.cancel();
        }
        self.channel_more_videos_search = None;
        self.active_channel_more_videos_request_id = None;
        if let Some(cancel) = self.channel_playlists_cancel.take() {
            cancel.cancel();
        }
        self.channel_playlists_search = None;
        self.active_channel_playlists_request_id = None;
        if let Some(cancel) = self.channel_playlist_videos_cancel.take() {
            cancel.cancel();
        }
        self.channel_playlist_videos_search = None;
        self.active_channel_playlist_videos_request_id = None;
        if let Some(cancel) = self.channel_about_cancel.take() {
            cancel.cancel();
        }
        self.channel_about_search = None;
        self.active_channel_about_request_id = None;
    }

    fn reset_channel_tabs_state(&mut self, ctx: &egui::Context) {
        self.cancel_channel_subrequests();
        for pl in &self.channel_playlists {
            let thumb = &pl.thumbnail;
            if !thumb.is_empty() && !self.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channel_playlist_videos {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channel_tab = ChannelTab::Videos;
        self.channel_more_videos_error = None;
        self.channel_videos_has_more = false;
        self.channel_videos_reached_max = false;
        self.channel_playlists.clear();
        self.channel_playlists_loaded = false;
        self.channel_playlists_error = None;
        self.channel_selected_playlist = None;
        self.channel_playlist_videos.clear();
        self.channel_playlist_videos_loaded = false;
        self.channel_playlist_videos_error = None;
        self.channel_about = None;
        self.channel_about_loaded = false;
        self.channel_about_error = None;
    }

    fn cancel_channel_loading(&mut self) {
        if let Some(cancel) = self.channel_cancel.take() {
            cancel.cancel();
        }
        self.active_channel_request_id = None;
        self.channel_loading_target = None;
        self.channel_search = None;
        self.cancel_channel_subrequests();
    }

    fn current_channel_target(&self) -> Option<ChannelTarget> {
        self.channel_loading_target
            .clone()
            .or_else(|| self.channel_page.as_ref().map(|page| page.target.clone()))
            .or_else(|| self.channel_last_target.clone())
    }

    fn apply_search_results(&mut self, ctx: &egui::Context, results: Vec<SearchResult>) {
        self.message = if results.is_empty() {
            "Nenhum vídeo encontrado.".into()
        } else {
            String::new()
        };
        if self.channel_view_open {
            for previous in &self.videos {
                let thumb = &previous.thumbnail;
                if !thumb.is_empty()
                    && !results.iter().any(|item| &item.video.thumbnail == thumb)
                    && !self.channel_page.as_ref().is_some_and(|page| {
                        page.videos
                            .iter()
                            .any(|item| &item.video.thumbnail == thumb)
                    })
                    && !self
                        .channel_playlist_videos
                        .iter()
                        .any(|item| &item.video.thumbnail == thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
        }
        let channel_videos_len = self
            .channel_page
            .as_ref()
            .map_or(0, |page| page.videos.len());
        let mut videos = Vec::with_capacity(results.len());
        let mut metadata_map = HashMap::with_capacity(
            results.len()
                + self.queue.len()
                + channel_videos_len
                + self.channel_playlist_videos.len(),
        );
        for item in results {
            metadata_map.insert(item.video.url.clone(), item.metadata);
            videos.push(item.video);
        }
        if !self.channel_view_open {
            self.thumbnails
                .retain_for_videos(ctx, &self.videos, &videos);
        }
        if let Some(page) = &self.channel_page {
            for item in &page.videos {
                metadata_map
                    .entry(item.video.url.clone())
                    .or_insert_with(|| item.metadata.clone());
            }
        }
        for item in &self.channel_playlist_videos {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for queued in self.queue.videos() {
            if !metadata_map.contains_key(&queued.url) {
                if let Some(existing) = self.video_metadata.get(&queued.url) {
                    metadata_map.insert(queued.url.clone(), existing.clone());
                }
            }
        }
        if let Some(selected) = &self.selected {
            if let Some(found) = metadata_map.get(&selected.url) {
                self.selected_metadata = Some(found.clone());
            } else if let Some(existing) = &self.selected_metadata {
                metadata_map.insert(selected.url.clone(), existing.clone());
            }
        }
        self.videos = videos;
        self.feed_data_generation = self.feed_data_generation.wrapping_add(1);
        self.video_metadata = metadata_map;
        self.feed_cache_dirty = true;
    }

    fn invalidate_feed_cache(&mut self) {
        self.feed_cache_dirty = true;
    }

    fn ensure_feed_cache(&mut self) {
        let history_len = self.history.entries().len();
        let queue_len = self.queue.len();
        let selected_url = self.selected.as_ref().map(|video| video.url.clone());
        let cache_is_current = self.feed_cache.generation == self.feed_data_generation
            && self.feed_cache.filter == self.feed_filter
            && self.feed_cache.history_len == history_len
            && self.feed_cache.queue_len == queue_len
            && self.feed_cache.selected_url == selected_url;
        if !self.feed_cache_dirty && cache_is_current {
            return;
        }
        let profile = UserProfile::build(&self.history, &self.queue, self.selected.as_ref());
        self.feed_cache.chips =
            RecommendationEngine::available_chips(&self.videos, &self.video_metadata, &profile);
        self.feed_cache.ranked = RecommendationEngine::rank_videos(
            &self.videos,
            &self.video_metadata,
            &profile,
            &self.feed_filter,
        );
        self.feed_cache.profile = profile;
        self.feed_cache.generation = self.feed_data_generation;
        self.feed_cache.filter = self.feed_filter.clone();
        self.feed_cache.history_len = history_len;
        self.feed_cache.queue_len = queue_len;
        self.feed_cache.selected_url = selected_url;
        self.feed_cache_dirty = false;
    }

    fn apply_channel_page(&mut self, ctx: &egui::Context, page: ChannelPage) {
        if let Some(previous_page) = &self.channel_page {
            for old_item in &previous_page.videos {
                let thumb = &old_item.video.thumbnail;
                if !thumb.is_empty()
                    && !self.videos.iter().any(|v| &v.thumbnail == thumb)
                    && !page.videos.iter().any(|v| &v.video.thumbnail == thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
        }
        for item in &page.videos {
            self.video_metadata
                .insert(item.video.url.clone(), item.metadata.clone());
        }
        if let Some(selected) = &self.selected {
            if let Some(found) = self.video_metadata.get(&selected.url) {
                self.selected_metadata = Some(found.clone());
            }
        }
        let effective_limit = self
            .channel_video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        let loaded_count = page.videos.len();
        self.channel_videos_reached_max = loaded_count >= search::MAX_CHANNEL_VIDEOS_LIMIT;
        self.channel_videos_has_more =
            !self.channel_videos_reached_max && loaded_count >= effective_limit;
        self.channel_more_videos_error = None;
        self.channel_last_target = Some(page.target.clone());
        self.channel_error = None;
        self.channel_page = Some(page);
    }

    fn begin_channel_request(
        &mut self,
        target: ChannelTarget,
        ctx: &egui::Context,
    ) -> Option<(u64, ChannelTarget)> {
        let Some(valid_target) = search::parse_channel_target(&target.canonical_url()) else {
            self.channel_error = Some(
                "Não foi possível abrir o canal: identificador ou URL do canal inválido ou não resolvido com segurança."
                    .into(),
            );
            return None;
        };
        let is_different_channel = self.channel_page.as_ref().is_some_and(|prev| {
            prev.target != valid_target && prev.canonical_url != valid_target.canonical_url()
        }) || (self.channel_page.is_none()
            && self.channel_last_target.as_ref() != Some(&valid_target));
        self.cancel_channel_subrequests();
        self.channel_request_id = self.channel_request_id.wrapping_add(1);
        let request_id = self.channel_request_id;
        self.active_channel_request_id = Some(request_id);
        self.channel_loading_target = Some(valid_target.clone());
        self.channel_last_target = Some(valid_target.clone());
        self.channel_error = None;
        self.channel_more_videos_error = None;
        self.channel_view_open = true;
        self.settings_open = false;
        self.sync_player_window_visibility();
        if is_different_channel {
            if let Some(previous_page) = &self.channel_page {
                for item in &previous_page.videos {
                    let thumb = &item.video.thumbnail;
                    if !thumb.is_empty() && !self.videos.iter().any(|v| &v.thumbnail == thumb) {
                        ctx.forget_image(thumb);
                    }
                }
                self.channel_page = None;
            }
            self.reset_channel_tabs_state(ctx);
        }
        Some((request_id, valid_target))
    }

    fn open_channel(&mut self, target: ChannelTarget, ctx: &egui::Context) -> bool {
        self.open_channel_with_options(target, ctx, false)
    }

    fn open_channel_with_options(
        &mut self,
        target: ChannelTarget,
        ctx: &egui::Context,
        force_refresh: bool,
    ) -> bool {
        if let Some(cancel) = self.home_cancel.take() {
            cancel.cancel();
        }
        self.home_search = None;
        let Some((request_id, valid_target)) = self.begin_channel_request(target, ctx) else {
            return false;
        };
        if force_refresh {
            self.channel_playlists_loaded = false;
            self.channel_playlists_error = None;
            self.channel_about_loaded = false;
            self.channel_about_error = None;
        }
        let (sender, receiver) = mpsc::channel();
        if let Some(cancel) = self.channel_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channel_cancel = Some(cancel.clone());
        self.channel_search = Some(receiver);
        let input = valid_target.canonical_url();
        let limit = self.channel_video_limit;
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

    fn begin_channel_more_videos_request(&mut self) -> Option<(u64, ChannelTarget, usize, usize)> {
        if self.is_channel_loading() || self.is_channel_more_videos_loading() {
            return None;
        }
        let page = self.channel_page.as_ref()?;
        let current_len = page.videos.len();
        if current_len >= search::MAX_CHANNEL_VIDEOS_LIMIT {
            self.channel_videos_reached_max = true;
            self.channel_videos_has_more = false;
            return None;
        }
        let start = current_len + 1;
        let remaining = search::MAX_CHANNEL_VIDEOS_LIMIT.saturating_sub(current_len);
        let count = remaining.min(search::CHANNEL_VIDEOS_PAGE_STEP);
        if count == 0 {
            self.channel_videos_reached_max = true;
            self.channel_videos_has_more = false;
            return None;
        }
        let target = page.target.clone();
        self.channel_request_id = self.channel_request_id.wrapping_add(1);
        let request_id = self.channel_request_id;
        self.active_channel_more_videos_request_id = Some(request_id);
        self.channel_more_videos_error = None;
        Some((request_id, target, start, count))
    }

    fn load_more_channel_videos(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target, start, count)) = self.begin_channel_more_videos_request()
        else {
            return false;
        };
        if let Some(cancel) = self.channel_more_videos_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channel_more_videos_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channel_more_videos_search = Some(receiver);
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

    fn cancel_channel_more_videos(&mut self) {
        if let Some(cancel) = self.channel_more_videos_cancel.take() {
            cancel.cancel();
        }
        self.channel_more_videos_search = None;
        self.active_channel_more_videos_request_id = None;
    }

    fn complete_channel_more_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelVideosBatch, String>,
    ) -> bool {
        if self.active_channel_more_videos_request_id != Some(request_id) {
            return false;
        }
        if self
            .channel_page
            .as_ref()
            .is_none_or(|page| &page.target != target)
        {
            self.active_channel_more_videos_request_id = None;
            self.channel_more_videos_search = None;
            self.channel_more_videos_cancel = None;
            return false;
        }
        self.active_channel_more_videos_request_id = None;
        self.channel_more_videos_search = None;
        self.channel_more_videos_cancel = None;
        match result {
            Ok(batch) => {
                self.channel_more_videos_error = None;
                self.channel_videos_has_more = batch.has_more;
                self.channel_videos_reached_max = batch.reached_max_limit;
                if let Some(page) = &mut self.channel_page {
                    for item in batch.videos {
                        if page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT {
                            self.channel_videos_reached_max = true;
                            self.channel_videos_has_more = false;
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
                        self.channel_videos_reached_max = true;
                        self.channel_videos_has_more = false;
                    }
                    let effective_limit = self
                        .channel_video_limit
                        .max(page.videos.len())
                        .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
                    search::update_cached_channel_videos(
                        &page.target,
                        effective_limit,
                        &page.videos,
                    );
                }
                if let Some(selected) = &self.selected {
                    if let Some(found) = self.video_metadata.get(&selected.url) {
                        self.selected_metadata = Some(found.clone());
                    }
                }
            }
            Err(error) => {
                self.channel_more_videos_error = Some(error);
            }
        }
        true
    }

    fn begin_channel_playlists_request(&mut self) -> Option<(u64, ChannelTarget)> {
        if self.is_channel_playlists_loading() {
            return None;
        }
        let target = self
            .channel_page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_channel_target())?;
        self.channel_request_id = self.channel_request_id.wrapping_add(1);
        let request_id = self.channel_request_id;
        self.active_channel_playlists_request_id = Some(request_id);
        self.channel_playlists_error = None;
        Some((request_id, target))
    }

    fn start_channel_playlists(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.begin_channel_playlists_request() else {
            return false;
        };
        if let Some(cancel) = self.channel_playlists_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channel_playlists_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channel_playlists_search = Some(receiver);
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

    fn cancel_channel_playlists(&mut self) {
        if let Some(cancel) = self.channel_playlists_cancel.take() {
            cancel.cancel();
        }
        self.channel_playlists_search = None;
        self.active_channel_playlists_request_id = None;
    }

    fn complete_channel_playlists(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<Vec<search::ChannelPlaylist>, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.active_channel_playlists_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target) {
            self.active_channel_playlists_request_id = None;
            self.channel_playlists_search = None;
            self.channel_playlists_cancel = None;
            return false;
        }
        self.active_channel_playlists_request_id = None;
        self.channel_playlists_search = None;
        self.channel_playlists_cancel = None;
        match result {
            Ok(playlists) => {
                for old_pl in &self.channel_playlists {
                    let thumb = &old_pl.thumbnail;
                    if !thumb.is_empty()
                        && !playlists.iter().any(|p| p.thumbnail == *thumb)
                        && !self.videos.iter().any(|v| &v.thumbnail == thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
                self.channel_playlists = playlists;
                self.channel_playlists_loaded = true;
                self.channel_playlists_error = None;
            }
            Err(error) => {
                self.channel_playlists_error = Some(error);
            }
        }
        true
    }

    fn begin_channel_playlist_videos_request(
        &mut self,
        playlist: search::ChannelPlaylist,
        ctx: &egui::Context,
    ) -> Option<(u64, ChannelTarget, String)> {
        if !search::is_valid_playlist_id(&playlist.id) {
            self.channel_playlist_videos_error =
                Some("Não foi possível abrir a playlist: identificador inválido.".into());
            return None;
        }
        let target = self
            .channel_page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_channel_target())?;
        if let Some(cancel) = self.channel_playlist_videos_cancel.take() {
            cancel.cancel();
        }
        for old_item in &self.channel_playlist_videos {
            let thumb = &old_item.video.thumbnail;
            if !thumb.is_empty() && !self.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channel_playlist_videos.clear();
        self.channel_playlist_videos_loaded = false;
        self.channel_playlist_videos_error = None;
        let playlist_id = playlist.id.clone();
        self.channel_selected_playlist = Some(playlist);
        self.channel_request_id = self.channel_request_id.wrapping_add(1);
        let request_id = self.channel_request_id;
        self.active_channel_playlist_videos_request_id = Some(request_id);
        Some((request_id, target, playlist_id))
    }

    fn open_channel_public_playlist(
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
        self.channel_playlist_videos_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channel_playlist_videos_search = Some(receiver);
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

    fn close_channel_public_playlist(&mut self, ctx: &egui::Context) {
        if let Some(cancel) = self.channel_playlist_videos_cancel.take() {
            cancel.cancel();
        }
        self.channel_playlist_videos_search = None;
        self.active_channel_playlist_videos_request_id = None;
        self.channel_playlist_videos_error = None;
        self.channel_playlist_videos_loaded = false;
        self.channel_selected_playlist = None;
        for old_item in &self.channel_playlist_videos {
            let thumb = &old_item.video.thumbnail;
            if !thumb.is_empty() && !self.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channel_playlist_videos.clear();
    }

    fn cancel_channel_playlist_videos(&mut self) {
        if let Some(cancel) = self.channel_playlist_videos_cancel.take() {
            cancel.cancel();
        }
        self.channel_playlist_videos_search = None;
        self.active_channel_playlist_videos_request_id = None;
    }

    fn complete_channel_playlist_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        playlist_id: &str,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.active_channel_playlist_videos_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target)
            || self
                .channel_selected_playlist
                .as_ref()
                .is_none_or(|pl| pl.id != playlist_id)
        {
            self.active_channel_playlist_videos_request_id = None;
            self.channel_playlist_videos_search = None;
            self.channel_playlist_videos_cancel = None;
            return false;
        }
        self.active_channel_playlist_videos_request_id = None;
        self.channel_playlist_videos_search = None;
        self.channel_playlist_videos_cancel = None;
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected {
                    if let Some(found) = self.video_metadata.get(&selected.url) {
                        self.selected_metadata = Some(found.clone());
                    }
                }
                self.channel_playlist_videos = videos;
                self.channel_playlist_videos_loaded = true;
                self.channel_playlist_videos_error = None;
            }
            Err(error) => {
                self.channel_playlist_videos_error = Some(error);
            }
        }
        true
    }

    fn begin_channel_about_request(&mut self) -> Option<(u64, ChannelTarget)> {
        if self.is_channel_about_loading() {
            return None;
        }
        let target = self
            .channel_page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_channel_target())?;
        self.channel_request_id = self.channel_request_id.wrapping_add(1);
        let request_id = self.channel_request_id;
        self.active_channel_about_request_id = Some(request_id);
        self.channel_about_error = None;
        Some((request_id, target))
    }

    fn start_channel_about(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.begin_channel_about_request() else {
            return false;
        };
        if let Some(cancel) = self.channel_about_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.channel_about_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.channel_about_search = Some(receiver);
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

    fn cancel_channel_about(&mut self) {
        if let Some(cancel) = self.channel_about_cancel.take() {
            cancel.cancel();
        }
        self.channel_about_search = None;
        self.active_channel_about_request_id = None;
    }

    fn complete_channel_about(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelAboutInfo, String>,
    ) -> bool {
        if self.active_channel_about_request_id != Some(request_id) {
            return false;
        }
        if self.current_channel_target().as_ref() != Some(target) {
            self.active_channel_about_request_id = None;
            self.channel_about_search = None;
            self.channel_about_cancel = None;
            return false;
        }
        self.active_channel_about_request_id = None;
        self.channel_about_search = None;
        self.channel_about_cancel = None;
        match result {
            Ok(about) => {
                if let Some(page) = &mut self.channel_page {
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
                self.channel_about = Some(about);
                self.channel_about_loaded = true;
                self.channel_about_error = None;
            }
            Err(error) => {
                self.channel_about_error = Some(error);
            }
        }
        true
    }

    #[cfg(test)]
    fn open_channel_for_metadata(&mut self, metadata: &VideoMetadata, ctx: &egui::Context) -> bool {
        let Some(target) = search::channel_target_for_metadata(metadata) else {
            self.channel_error = Some(
                "Não foi possível abrir o canal: os metadados do vídeo não fornecem um ID UC ou handle @ válido."
                    .into(),
            );
            return false;
        };
        self.open_channel(target, ctx)
    }

    fn complete_channel_request(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<ChannelPage, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.active_channel_request_id != Some(request_id)
            || self.channel_loading_target.as_ref() != Some(target)
        {
            return false;
        }
        self.active_channel_request_id = None;
        self.channel_loading_target = None;
        self.channel_search = None;
        match result {
            Ok(page) => {
                self.apply_channel_page(ctx, page);
            }
            Err(error) => {
                self.channel_error = Some(error);
            }
        }
        true
    }

    fn complete_channel_outcome(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelFetchOutcome, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.active_channel_request_id != Some(request_id)
            || self.channel_loading_target.as_ref() != Some(target)
        {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.channel_from_cache = outcome.from_cache;
                self.complete_channel_request(request_id, target, Ok(outcome.page), ctx)
            }
            Err(error) => {
                self.channel_from_cache = false;
                self.complete_channel_request(request_id, target, Err(error), ctx)
            }
        }
    }

    fn close_channel_view(&mut self) {
        if let Some(cancel) = self.channel_cancel.take() {
            cancel.cancel();
        }
        self.cancel_channel_subrequests();
        self.channel_view_open = false;
        self.active_channel_request_id = None;
        self.channel_loading_target = None;
        self.channel_search = None;
    }

    fn enqueue_video(&mut self, video: Video) -> Result<(), Video> {
        let title = video.title.clone();
        match self.queue.add(video) {
            Ok(()) => {
                self.set_pending_queue_warning();
                self.last_rejected_video = None;
                self.queue_notice = Some(format!(
                    "Adicionado à fila ({}/{}): {title}",
                    self.queue.len(),
                    self.queue.limit()
                ));
                self.persist_queue();
                Ok(())
            }
            Err(rejected) => {
                self.queue_notice = None;
                self.queue_error = Some(format!(
                    "Fila cheia ({}/{}): '{}' foi rejeitado sem perda.",
                    self.queue.len(),
                    self.queue.limit(),
                    rejected.title
                ));
                self.last_rejected_video = Some(rejected.clone());
                Err(rejected)
            }
        }
    }

    fn remove_from_queue(&mut self, index: usize) -> Option<Video> {
        let removed_entry_id = self.queue.entry_id(index);
        match self.queue.remove(index) {
            Some(removed) => {
                self.invalidate_feed_cache();
                let pending_id_matches = removed_entry_id
                    .zip(self.pending_queue_entry_id)
                    .is_some_and(|(removed, pending)| removed == pending);
                let legacy_video_matches = self.pending_queue_entry_id.is_none()
                    && self.pending_queue_video.as_ref() == Some(&removed);
                if pending_id_matches || legacy_video_matches {
                    self.cancel_pending_queue_video();
                }
                self.set_pending_queue_warning();
                self.queue_notice = Some(format!(
                    "Removido da fila ({}/{}): {}",
                    self.queue.len(),
                    self.queue.limit(),
                    removed.title
                ));
                self.persist_queue();
                Some(removed)
            }
            None => {
                self.queue_error = Some(format!("Índice inválido para remoção na fila: {index}."));
                None
            }
        }
    }

    fn reorder_queue(&mut self, from: usize, to: usize) -> bool {
        if self.queue.reorder(from, to) {
            self.set_pending_queue_warning();
            self.queue_notice = Some(format!("Fila reordenada ({} → {}).", from + 1, to + 1));
            self.persist_queue();
            true
        } else {
            self.queue_error = Some(format!(
                "Não foi possível reordenar a fila ({from} → {to})."
            ));
            false
        }
    }

    fn advance_queue_at(&mut self, now: Instant) -> Option<Video> {
        if self.pending_queue_video.is_some() {
            self.queue_error =
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
        let Some((next_video, next_entry_id)) = self.queue.first_entry_id().and_then(|entry_id| {
            self.queue
                .videos()
                .first()
                .cloned()
                .map(|video| (video, entry_id))
        }) else {
            self.queue_error = Some("A fila está vazia; nenhum próximo vídeo para avançar.".into());
            return None;
        };
        let previous_player_pos_ms = self.current_player_live_pos_ms();
        #[cfg(windows)]
        self.sample_player_before_transition(now);
        self.flush_playback_history_at(now);
        let resume_target_ms = self.planned_resume_ms(&next_video);
        self.quality = None;
        self.qualities.clear();
        self.quality_search = None;
        self.quality_error = None;
        self.seek_drag = None;
        self.reset_decoding_diagnostics();
        self.selected_metadata = self.metadata_for(&next_video.url).cloned();
        self.activate_video_playback(&next_video, resume_target_ms, previous_player_pos_ms, now);
        self.selected = Some(next_video.clone());
        self.invalidate_feed_cache();
        self.pending_queue_video = Some(next_video.clone());
        self.pending_queue_entry_id = Some(next_entry_id);
        self.queue_correlation.begin();
        self.sync_player_window_visibility();
        self.set_pending_queue_warning();
        Some(next_video)
    }

    fn advance_queue_with_frame(
        &mut self,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if self.pending_queue_video.is_some() {
            self.queue_error =
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
        let Some((candidate, entry_id)) = self.queue.first_entry_id().and_then(|entry_id| {
            self.queue
                .videos()
                .first()
                .cloned()
                .map(|video| (video, entry_id))
        }) else {
            self.queue_error = Some("A fila está vazia; nenhum próximo vídeo para avançar.".into());
            return None;
        };
        if !self.open_video(candidate.clone(), frame, ctx) {
            self.queue_error = Some(
                "Não foi possível iniciar o próximo vídeo; o item foi mantido na fila.".into(),
            );
            return None;
        }
        self.pending_queue_video = Some(candidate.clone());
        self.pending_queue_entry_id = Some(entry_id);
        self.queue_correlation.begin();
        self.set_pending_queue_warning();
        self.queue_notice = Some(format!(
            "Abrindo da fila ({} itens; aguardando o início): {}",
            self.queue.len(),
            candidate.title
        ));
        Some(candidate)
    }

    fn report_pending_queue_playback_observed(&mut self) {
        self.queue_notice = Some(
            "Reprodu\u{00e7}\u{00e3}o observada; aguardando confirma\u{00e7}\u{00e3}o do libmpv."
                .into(),
        );
    }

    fn set_pending_queue_warning(&mut self) {
        self.queue_error = None;
    }

    fn report_pending_queue_load_error(&mut self) {
        if self.pending_queue_video.is_some() {
            self.queue_error = Some(
                "O libmpv relatou um erro durante a abertura; o item permanece na fila. Confira o player e remova-o manualmente pelo botão × se desejar.".into(),
            );
        }
    }

    fn cancel_pending_queue_video(&mut self) {
        self.pending_queue_video = None;
        self.pending_queue_entry_id = None;
        self.queue_correlation.cancel();
    }

    fn handle_queue_player_event(&mut self, event: PlaybackEvent) {
        match self.queue_correlation.handle(event) {
            QueueLoadResult::Confirmed { .. } => {
                let entry_id = self.pending_queue_entry_id.take();
                if let Some(candidate) = self.pending_queue_video.take() {
                    let removed = match entry_id {
                        Some(entry_id) => self.queue.remove_entry(entry_id),
                        None => self
                            .queue
                            .videos()
                            .iter()
                            .position(|video| *video == candidate)
                            .and_then(|index| self.queue.remove(index)),
                    };
                    if removed.is_some() {
                        self.invalidate_feed_cache();
                        self.persist_queue();
                        self.queue_error = None;
                        self.queue_notice = Some(format!(
                            "Reproduzindo da fila ({} restantes): {}",
                            self.queue.len(),
                            candidate.title
                        ));
                    } else {
                        self.queue_error = Some("O item confirmado nao esta mais na fila; nenhum outro item foi removido.".into());
                    }
                }
            }
            QueueLoadResult::Failed { .. } => {
                self.pending_queue_video = None;
                self.pending_queue_entry_id = None;
                self.queue_error =
                    Some("O libmpv falhou ao carregar o item; ele continua na fila.".into());
            }
            QueueLoadResult::None => {}
        }
    }

    fn queue_auto_advance_ready(&self, ended: bool) -> bool {
        ended
            && self.pending_queue_video.is_none()
            && !self.queue.is_empty()
            && self.selected.is_some()
            && self
                .active_playback
                .as_ref()
                .is_some_and(|active| active.has_live_sample)
    }

    fn request_queue_advance(&mut self) {
        if self.in_app_ui_pass {
            self.pending_queue_advance = true;
        } else {
            let _ = self.advance_queue_at(Instant::now());
        }
    }

    fn begin_search_request(&mut self) -> Option<(u64, String)> {
        let query = self.query.trim().to_owned();
        if query.is_empty() {
            return None;
        }
        if let Some(cancel) = self.home_cancel.take() {
            cancel.cancel();
        }
        self.home_search = None;
        self.home_title = None;
        self.home_playlist_urls.clear();
        self.search_request_id = self.search_request_id.wrapping_add(1);
        let request_id = self.search_request_id;
        self.active_search_request_id = Some(request_id);
        self.channel_view_open = false;
        self.cancel_channel_subrequests();
        // A nova pesquisa substitui a navegação atual: descarta o pedido de canal
        // em andamento para não manter o cabeçalho em "Carregando canal..." nem
        // aplicar uma página antiga depois que o usuário já mudou de contexto.
        if self.active_channel_request_id.is_some() {
            if let Some(cancel) = self.channel_cancel.take() {
                cancel.cancel();
            }
            self.active_channel_request_id = None;
            self.channel_loading_target = None;
            self.channel_search = None;
        }
        self.message = "Pesquisando...".into();
        Some((request_id, query))
    }

    fn complete_search_request(
        &mut self,
        request_id: u64,
        result: Result<Vec<SearchResult>, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.active_search_request_id != Some(request_id) {
            return false;
        }
        self.active_search_request_id = None;
        self.search = None;
        match result {
            Ok(results) => self.apply_search_results(ctx, results),
            Err(error) => self.message = error,
        }
        true
    }

    fn start_public_home(&mut self, ctx: &egui::Context) {
        if self.search.is_some() || self.home_search.is_some() {
            return;
        }
        self.browse_section = BrowseSection::Home;
        self.search_results_mode = false;
        self.feed_filter = FeedFilter::Recommended;
        self.ensure_feed_cache();
        let profile = self.feed_cache.profile.clone();
        self.query = RecommendationEngine::build_discovery_query(&profile, &self.feed_filter);
        self.start_search(ctx);
    }

    fn clear_visual_caches(&mut self, ctx: &egui::Context) {
        let current_videos = std::mem::take(&mut self.videos);
        self.thumbnails.reset(ctx, &current_videos);
        self.videos = current_videos;
        self.qualities.clear();
        self.feed_cache_dirty = true;
        self.settings_notice = Some(
            "Miniaturas e resoluções em cache foram liberadas. O feed continua disponível e os dados locais não foram alterados."
                .into(),
        );
    }

    fn start_search(&mut self, ctx: &egui::Context) {
        let Some((request_id, query)) = self.begin_search_request() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        if let Some(cancel) = self.search_cancel.take() {
            cancel.cancel();
        }
        let cancel = process::Cancellation::default();
        self.search_cancel = Some(cancel.clone());
        self.search = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = search::find_with_cancel(&query, &cancel);
            let _ = sender.send(SearchFetchMessage { request_id, result });
            ctx.request_repaint();
        });
    }

    fn start_browser_feed(&mut self, target: String, interactive: bool, ctx: &egui::Context) {
        if target == ":ytrec" && self.home_search.is_some() {
            return;
        }
        if let Some(cancel) = self.home_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.search_cancel.take() {
            cancel.cancel();
        }
        self.search = None;
        self.active_search_request_id = None;
        self.search_results_mode = false;
        self.close_channel_view();
        self.home_playlist_urls.clear();
        self.home_limit_reached = false;
        self.pending_home_items.clear();
        if target == ":ytrec" {
            self.subscription_channels_error = None;
        }
        self.apply_search_results(ctx, Vec::new());
        let title = if target == ":ytrec" {
            "Inscrições: envios recentes"
        } else if target == ":google-playlists" {
            "Playlists da conta Google"
        } else if target.starts_with("fasttube-google-playlist:") {
            "Vídeos da playlist"
        } else {
            "Playlist do YouTube"
        };
        self.home_title = Some(title.into());
        self.message = if target == ":ytrec" {
            "Consultando suas inscrições..."
        } else if target == ":google-playlists" || target.starts_with("fasttube-google-playlist:") {
            "Consultando suas playlists Google..."
        } else {
            "Consultando a playlist..."
        }
        .into();
        let cancel = process::Cancellation::default();
        self.home_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.home_search = Some(receiver);
        let ctx = ctx.clone();
        let saved_session = self.oauth_session.clone();
        std::thread::spawn(move || {
            let result = if target == ":ytrec"
                || target == ":google-playlists"
                || target.starts_with("fasttube-google-playlist:")
            {
                match saved_session {
                    Some(mut session) => {
                        if let Err(error) = session.refresh_if_needed(&cancel) {
                            (Some(session), Err(error))
                        } else {
                            let feed = if target == ":google-playlists" {
                                oauth::playlists_feed(&session.access_token, &cancel)
                            } else if let Some(id) =
                                target.strip_prefix("fasttube-google-playlist:")
                            {
                                oauth::playlist_videos_feed(&session.access_token, id, &cancel)
                            } else {
                                let progress_sender = sender.clone();
                                let progress_ctx = ctx.clone();
                                oauth::subscriptions_feed(
                                    &session.access_token,
                                    &cancel,
                                    move |progress| {
                                        let _ = progress_sender
                                            .send(HomeFetchMessage::Progress(progress));
                                        progress_ctx.request_repaint();
                                    },
                                )
                            };
                            (Some(session), feed)
                        }
                    }
                    None if interactive => match oauth::authorize(&cancel) {
                        Ok(session) => {
                            let feed = if target == ":google-playlists" {
                                oauth::playlists_feed(&session.access_token, &cancel)
                            } else if let Some(id) =
                                target.strip_prefix("fasttube-google-playlist:")
                            {
                                oauth::playlist_videos_feed(&session.access_token, id, &cancel)
                            } else {
                                let progress_sender = sender.clone();
                                let progress_ctx = ctx.clone();
                                oauth::subscriptions_feed(
                                    &session.access_token,
                                    &cancel,
                                    move |progress| {
                                        let _ = progress_sender
                                            .send(HomeFetchMessage::Progress(progress));
                                        progress_ctx.request_repaint();
                                    },
                                )
                            };
                            (Some(session), feed)
                        }
                        Err(error) => (None, Err(error)),
                    },
                    None => (
                        None,
                        Err("Entre na conta para consultar suas inscrições.".into()),
                    ),
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

    fn start_reauth(&mut self, ctx: &egui::Context) {
        if self.reauth.is_some() {
            return;
        }
        let cancel = process::Cancellation::default();
        self.reauth_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.reauth = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(oauth::authorize(&cancel));
            ctx.request_repaint();
        });
    }

    fn cancel_reauth(&mut self) {
        if let Some(cancel) = self.reauth_cancel.take() {
            cancel.cancel();
        }
        self.reauth = None;
    }

    fn start_video_rating(
        &mut self,
        video_id: String,
        rating: Option<oauth::VideoRating>,
        ctx: &egui::Context,
    ) {
        if self.rating_request.is_some() {
            return;
        }
        let Some(session) = self.oauth_session.as_ref() else {
            self.message = "Entre com Google para avaliar este vídeo.".into();
            self.start_reauth(ctx);
            return;
        };
        let token = session.access_token.clone();
        let cancel = process::Cancellation::default();
        self.rating_cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.rating_request = Some(receiver);
        let api_rating = rating.unwrap_or(oauth::VideoRating::None);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = oauth::rate_video(&token, &video_id, api_rating, &cancel)
                .map(|()| (video_id, rating));
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn set_oauth_session(&mut self, session: Option<oauth::TokenSession>) -> Option<String> {
        if session.is_none() {
            self.video_ratings.clear();
        }
        self.oauth_session = session;
        #[cfg(test)]
        return None;
        #[cfg(not(test))]
        match self.oauth_session.as_ref() {
            Some(session) => oauth::save_session(session).err(),
            None => oauth::delete_session().err(),
        }
    }

    fn cancel_login(&mut self) {
        if let Some(cancel) = self.home_cancel.take() {
            cancel.cancel();
        }
        self.home_search = None;
        self.pending_home_items.clear();
        self.home_title = None;
        self.home_limit_reached = false;
        self.message = "Login cancelado. Você pode tentar novamente.".into();
    }

    fn cancel_home_feed(&mut self, ctx: &egui::Context) {
        if let Some(cancel) = self.home_cancel.take() {
            cancel.cancel();
        }
        self.home_search = None;
        if !self.pending_home_items.is_empty() {
            let partial = std::mem::take(&mut self.pending_home_items);
            self.apply_search_results(ctx, partial);
        }
        self.message =
            "Carregamento cancelado. Os resultados já recebidos permanecem disponíveis.".into();
    }

    fn poll_async_tasks(&mut self, ctx: &egui::Context) {
        self.poll_reauth(ctx);
        self.poll_rating(ctx);
        self.poll_home(ctx);
        self.poll_quality();
        self.poll_search(ctx);
        self.poll_channel(ctx);
        self.poll_channel_more_videos();
        self.poll_channel_playlists(ctx);
        self.poll_channel_playlist_videos();
        self.poll_channel_about();
    }

    fn poll_reauth(&mut self, ctx: &egui::Context) {
        let reauth_result = self.reauth.as_ref().map(Receiver::try_recv);
        let mut reauth_finished = None;
        match reauth_result {
            Some(Ok(result)) => reauth_finished = Some(result),
            Some(Err(mpsc::TryRecvError::Empty)) => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.reauth = None;
                self.reauth_cancel = None;
                self.message = "Autoriza\u{00e7}\u{00e3}o interrompida.".into();
            }
            None => {}
        }
        if let Some(result) = reauth_finished {
            self.reauth = None;
            self.reauth_cancel = None;
            match result {
                Ok(session) => {
                    self.session_authenticated = true;
                    let persistence_error = self.set_oauth_session(Some(session));
                    self.watch_extras.reset_comments();
                    self.message = persistence_error
                        .map(|error| format!("Autorização concluída, mas não foi possível salvar a sessão: {error}"))
                        .unwrap_or_else(|| "Comentários autorizados com sucesso.".into());
                    if self.watch_extras.tab == watch_extras::Tab::Comments
                        && self.watch_extras.video_url.is_some()
                    {
                        self.watch_extras
                            .start_comments(self.oauth_session.as_ref(), false, ctx);
                    }
                }
                Err(error) => {
                    self.message = error.clone();
                    if self.watch_extras.tab == watch_extras::Tab::Comments {
                        self.watch_extras.comments_error = Some(error);
                    }
                }
            }
        }
    }

    fn poll_rating(&mut self, ctx: &egui::Context) {
        let rating_result = self.rating_request.as_ref().map(Receiver::try_recv);
        match rating_result {
            Some(Ok(Ok((video_id, rating)))) => {
                if let Some(rating) = rating {
                    self.video_ratings.insert(video_id, rating);
                    self.message = match rating {
                        oauth::VideoRating::Like => "Você gostou deste vídeo.".into(),
                        oauth::VideoRating::Dislike => "Você não gostou deste vídeo.".into(),
                        oauth::VideoRating::None => "Avaliação removida.".into(),
                    };
                } else {
                    self.video_ratings.remove(&video_id);
                    self.message = "Avaliação removida.".into();
                }
                self.rating_request = None;
                self.rating_cancel = None;
            }
            Some(Ok(Err(error))) => {
                self.message = error;
                self.rating_request = None;
                self.rating_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.message = "A avaliação foi interrompida.".into();
                self.rating_request = None;
                self.rating_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            None => {}
        }
    }

    fn poll_home(&mut self, ctx: &egui::Context) {
        let mut home_messages = Vec::new();
        let mut home_disconnected = false;
        if let Some(receiver) = &self.home_search {
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
                    if self.home_title.as_deref()
                        == Some("Inscri\u{00e7}\u{00f5}es: envios recentes")
                    {
                        self.subscription_channels = progress.channels.clone();
                        self.subscription_channels_loaded = true;
                        self.subscription_channels_error = None;
                    }
                    self.home_limit_reached |= progress.reached_limit;
                    self.pending_home_items = progress.items;
                    self.message = if self.home_limit_reached {
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
                    self.pending_home_items.clear();
                    self.session_authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    let is_subscription_feed = self.home_title.as_deref()
                        == Some("Inscri\u{00e7}\u{00f5}es: envios recentes");
                    self.home_search = None;
                    self.home_cancel = None;
                    if is_subscription_feed {
                        self.subscription_channels = feed.channels.clone();
                        self.subscription_channels_loaded = true;
                        self.subscription_channels_error = None;
                    }
                    self.home_playlist_urls = feed.playlists;
                    self.apply_search_results(ctx, feed.items);
                    if self.home_limit_reached {
                        self.message = "Limite de 100 canais por atualiza\u{e7}\u{e3}o atingido; algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.".into();
                    } else if self.videos.is_empty() {
                        self.message = if self.home_title.as_deref()
                            == Some("Playlists da conta Google")
                        {
                            "Nenhuma playlist encontrada nesta conta.".into()
                        } else if self.home_title.as_deref() == Some("Vídeos da playlist") {
                            "Nenhum vídeo encontrado nesta playlist.".into()
                        } else {
                            "Nenhum envio encontrado nas inscri\u{e7}\u{f5}es consultadas.".into()
                        };
                    }
                    if let Some(error) = persistence_error {
                        self.message = format!("{} (sessao local: {error})", self.message);
                    }
                }
                HomeFetchMessage::Finished(token, Err(error)) => {
                    let is_subscription_feed = self.home_title.as_deref()
                        == Some("Inscri\u{00e7}\u{00f5}es: envios recentes");
                    if is_subscription_feed {
                        self.subscription_channels_error = Some(error.clone());
                    }
                    if !self.pending_home_items.is_empty() {
                        let partial = std::mem::take(&mut self.pending_home_items);
                        self.apply_search_results(ctx, partial);
                    }
                    self.home_search = None;
                    self.home_cancel = None;
                    self.session_authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    self.message = persistence_error
                        .map(|save_error| format!("{error} ({save_error})"))
                        .unwrap_or(error);
                }
            }
        }
        if home_disconnected && self.home_search.is_some() {
            self.home_search = None;
            self.home_cancel = None;
            if !self.pending_home_items.is_empty() {
                let partial = std::mem::take(&mut self.pending_home_items);
                self.apply_search_results(ctx, partial);
            }
            self.message = "A consulta foi interrompida.".into();
        }
        if self.home_search.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn poll_quality(&mut self) {
        if let Some(receiver) = &self.quality_search {
            match receiver.try_recv() {
                Ok(Ok(qualities)) => {
                    self.qualities = qualities;
                    self.quality_search = None;
                }
                Ok(Err(error)) => {
                    self.quality_error = Some(error);
                    self.quality_search = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.quality_error = Some("A consulta de resoluções foi interrompida.".into());
                    self.quality_search = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    fn poll_search(&mut self, ctx: &egui::Context) {
        loop {
            let outcome = self.search.as_ref().map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ = self.complete_search_request(msg.request_id, msg.result, ctx);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.active_search_request_id.is_some() {
                        self.message = "A pesquisa foi interrompida.".into();
                    }
                    self.active_search_request_id = None;
                    self.search = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    fn poll_channel(&mut self, ctx: &egui::Context) {
        loop {
            let outcome = self
                .channel_search
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ =
                        self.complete_channel_outcome(msg.request_id, &msg.target, msg.result, ctx);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.active_channel_request_id.is_some() {
                        self.channel_error = Some("A consulta do canal foi interrompida.".into());
                    }
                    self.active_channel_request_id = None;
                    self.channel_loading_target = None;
                    self.channel_search = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    fn poll_channel_more_videos(&mut self) {
        loop {
            let outcome = self
                .channel_more_videos_search
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ =
                        self.complete_channel_more_videos(msg.request_id, &msg.target, msg.result);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.active_channel_more_videos_request_id.is_some() {
                        self.channel_more_videos_error =
                            Some("O carregamento de mais vídeos do canal foi interrompido.".into());
                    }
                    self.active_channel_more_videos_request_id = None;
                    self.channel_more_videos_search = None;
                    self.channel_more_videos_cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    fn poll_channel_playlists(&mut self, ctx: &egui::Context) {
        loop {
            let outcome = self
                .channel_playlists_search
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
                    if self.active_channel_playlists_request_id.is_some() {
                        self.channel_playlists_error =
                            Some("A consulta de playlists públicas foi interrompida.".into());
                    }
                    self.active_channel_playlists_request_id = None;
                    self.channel_playlists_search = None;
                    self.channel_playlists_cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    fn poll_channel_playlist_videos(&mut self) {
        loop {
            let outcome = self
                .channel_playlist_videos_search
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
                    if self.active_channel_playlist_videos_request_id.is_some() {
                        self.channel_playlist_videos_error =
                            Some("A consulta dos vídeos da playlist foi interrompida.".into());
                    }
                    self.active_channel_playlist_videos_request_id = None;
                    self.channel_playlist_videos_search = None;
                    self.channel_playlist_videos_cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }

    fn poll_channel_about(&mut self) {
        loop {
            let outcome = self
                .channel_about_search
                .as_ref()
                .map(|receiver| receiver.try_recv());
            match outcome {
                Some(Ok(msg)) => {
                    let _ = self.complete_channel_about(msg.request_id, &msg.target, msg.result);
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    if self.active_channel_about_request_id.is_some() {
                        self.channel_about_error =
                            Some("A consulta de informações da aba Sobre foi interrompida.".into());
                    }
                    self.active_channel_about_request_id = None;
                    self.channel_about_search = None;
                    self.channel_about_cancel = None;
                    break;
                }
                Some(Err(mpsc::TryRecvError::Empty)) | None => break,
            }
        }
    }
    fn open_video(&mut self, video: Video, frame: &eframe::Frame, ctx: &egui::Context) -> bool {
        self.cancel_pending_queue_video();
        #[cfg(windows)]
        {
            let now = Instant::now();
            let previous_player_pos_ms = self.current_player_live_pos_ms();
            self.sample_player_before_transition(now);
            self.flush_playback_history_at(now);
            self.ensure_playback_settings_synchronized();
            let resume_target_ms = self.planned_resume_ms(&video);

            if self.player.is_none() {
                let profile = self.buffer_profile;
                match player::Player::new(frame, profile.values(), ctx) {
                    Ok(player) => {
                        self.player = Some(player);
                        self.active_player_buffer = Some(profile);
                    }
                    Err(error) => {
                        self.message = error;
                        return false;
                    }
                }
            }
            if let Some(player) = &mut self.player {
                if let Err(error) =
                    player.set("options/ytdl-format", &self.playback_preferences.format())
                {
                    self.message = error;
                    return false;
                }
                if let Err(error) = player.set(
                    "speed",
                    &format!(
                        "{}",
                        f64::from(self.playback_preferences.speed_quarters) / 4.0
                    ),
                ) {
                    self.message = error;
                    return false;
                }
                if let Err(error) = player.set("options/start", "none") {
                    self.message = error;
                    return false;
                }
                let _ = player.set("pause", "no");
                if let Err(error) = player.play(&video.url) {
                    let _ = player.set("options/start", "none");
                    self.message = error;
                    return false;
                }
            }
            self.message.clear();
            self.quality = None;
            self.qualities.clear();
            self.quality_error = None;
            self.seek_drag = None;
            self.reset_decoding_diagnostics();
            let (sender, receiver) = mpsc::channel();
            if let Some(cancel) = self.quality_cancel.take() {
                cancel.cancel();
            }
            let cancel = process::Cancellation::default();
            self.quality_cancel = Some(cancel.clone());
            self.quality_search = Some(receiver);
            let url = video.url.clone();
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let _ = sender.send(search::resolutions_with_cancel(&url, &cancel));
                ctx_clone.request_repaint();
            });
            self.selected_metadata = self.metadata_for(&video.url).cloned();
            self.activate_video_playback(&video, resume_target_ms, previous_player_pos_ms, now);
            self.selected = Some(video);
            self.navigation_open = false;
            self.invalidate_feed_cache();
            self.sync_player_window_visibility();
            true
        }
        #[cfg(not(windows))]
        {
            let _ = (video, frame, ctx);
            self.message = "A reprodução está disponível somente no Windows.".into();
            false
        }
    }

    fn close_video(&mut self, ctx: &egui::Context) {
        self.cancel_pending_queue_video();
        self.watch_extras.sync_video(None);
        if let Some(cancel) = self.quality_cancel.take() {
            cancel.cancel();
        }
        let now = Instant::now();
        #[cfg(windows)]
        self.sample_player_before_transition(now);
        self.flush_playback_history_at(now);
        self.active_playback = None;
        self.selected = None;
        self.navigation_open = false;
        self.invalidate_feed_cache();
        self.selected_metadata = None;
        self.seek_drag = None;
        self.quality = None;
        self.qualities.clear();
        self.quality_search = None;
        self.quality_error = None;
        self.active_player_buffer = None;
        self.reset_decoding_diagnostics();
        #[cfg(windows)]
        {
            if let Some(player) = &mut self.player {
                player.set_visible(false);
            }
            self.player = None;
        }
        if self.fullscreen {
            self.toggle_fullscreen(ctx);
        }
        self.sync_player_window_visibility();
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.fullscreen = !self.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        self.sync_player_window_visibility();
    }

    fn toggle_theater_mode(&mut self) {
        self.theater_mode = !self.theater_mode;
        self.sync_player_window_visibility();
    }

    fn header_menu_button_ui(&mut self, ui: &mut egui::Ui) {
        let nav_visible = self.navigation_open
            && !self.fullscreen
            && !(self.selected.is_some() && self.theater_mode);
        let nav_tip = if self.selected.is_some() && self.theater_mode {
            "Mostrar navega\u{00e7}\u{00e3}o lateral e sair do modo teatro (T)"
        } else if nav_visible {
            "Ocultar navega\u{00e7}\u{00e3}o"
        } else {
            "Mostrar navega\u{00e7}\u{00e3}o"
        };
        let button = egui::Button::new(RichText::new("\u{2630}").size(18.0))
            .fill(Color32::TRANSPARENT)
            .corner_radius(18);
        if ui
            .add_sized([36.0, 36.0], button)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(nav_tip)
            .clicked()
        {
            if self.selected.is_some() && self.theater_mode {
                self.theater_mode = false;
                self.navigation_open = true;
                self.sync_player_window_visibility();
            } else {
                self.navigation_open = !self.navigation_open;
            }
        }
    }

    fn header_brand_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette) -> bool {
        let logo_clicked = paint_youtube_logo_badge(ui).clicked();
        ui.add_space(2.0);
        let brand_clicked = ui
            .add(egui::Button::new(RichText::new("FastTube").strong().size(19.5)).frame(false))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked();
        ui.label(
            RichText::new("BR")
                .size(9.5)
                .color(palette.neutral_status_fg),
        );
        logo_clicked || brand_clicked
    }

    fn handle_header_home(&mut self, ctx: &egui::Context) {
        if !(self.selected.is_some()
            || self.channel_view_open
            || self.browse_section != BrowseSection::Home
            || self.search_results_mode
            || self.feed_filter != FeedFilter::All)
        {
            return;
        }
        self.browse_section = BrowseSection::Home;
        self.search_results_mode = false;
        self.feed_filter = FeedFilter::All;
        self.invalidate_feed_cache();
        self.close_channel_view();
        if self.selected.is_some() {
            self.close_video(ctx);
        }
        if self.home_search.is_none() && self.search.is_none() {
            self.start_public_home(ctx);
        }
    }

    fn header_search_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        search_width: f32,
    ) {
        let search_width = search_width.clamp(220.0, 640.0);
        let pill_bg = if self.theme == Theme::Dark {
            Color32::from_rgb(18, 18, 18)
        } else {
            Color32::WHITE
        };
        let pill_stroke = if self.theme == Theme::Dark {
            Color32::from_rgb(48, 48, 48)
        } else {
            Color32::from_rgb(204, 204, 204)
        };
        let mut trigger_search = false;
        egui::Frame::new()
            .fill(pill_bg)
            .stroke(egui::Stroke::new(1.0, pill_stroke))
            .corner_radius(20)
            .inner_margin(egui::Margin::symmetric(14, 0))
            .show(ui, |ui| {
                ui.set_width((search_width - 28.0).max(160.0));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let clear_slot = if self.query.is_empty() { 0.0 } else { 24.0 };
                    let input_w = (search_width - 120.0 - clear_slot).max(70.0);
                    let search = ui.add_sized(
                        [input_w, 38.0],
                        egui::TextEdit::singleline(&mut self.query)
                            .frame(egui::Frame::NONE)
                            .hint_text("Pesquisar"),
                    );
                    let enter = search.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if !self.query.is_empty()
                        && ui
                            .add_sized(
                                [24.0, 38.0],
                                egui::Button::new(RichText::new("\u{00d7}").size(15.0))
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false),
                            )
                            .on_hover_text("Limpar pesquisa")
                            .clicked()
                    {
                        self.query.clear();
                    }
                    let button_bg = if self.theme == Theme::Dark {
                        Color32::from_rgb(34, 34, 34)
                    } else {
                        Color32::from_rgb(240, 240, 240)
                    };
                    let search_clicked = ui
                        .add_sized(
                            [52.0, 38.0],
                            egui::Button::image(Icon::Search.image(palette.icon_fg, 17.0))
                                .fill(button_bg)
                                .corner_radius(0),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Pesquisar (Enter)")
                        .clicked();
                    trigger_search = search_clicked || enter;
                });
            });
        if trigger_search {
            self.search_results_mode = true;
            self.browse_section = BrowseSection::Home;
            self.feed_filter = FeedFilter::All;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.start_search(ctx);
        }
    }

    fn header_recommendation_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let button = egui::Button::new(RichText::new("\u{1f399}").size(16.0))
            .fill(palette.header_button_bg)
            .corner_radius(20);
        if ui
            .add_sized([40.0, 40.0], button)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Ativar recomendados para voc\u{00ea}")
            .clicked()
        {
            self.browse_section = BrowseSection::Home;
            self.search_results_mode = false;
            self.feed_filter = FeedFilter::Recommended;
            self.invalidate_feed_cache();
            self.close_channel_view();
            if self.videos.is_empty() && self.search.is_none() && self.home_search.is_none() {
                self.ensure_feed_cache();
                let profile = self.feed_cache.profile.clone();
                self.query =
                    RecommendationEngine::build_discovery_query(&profile, &self.feed_filter);
                self.start_search(ctx);
            }
        }
    }

    fn navigate_from_shortcut(&mut self, ctx: &egui::Context, section: BrowseSection) {
        self.settings_open = false;
        self.search_results_mode = false;
        self.close_channel_view();
        if self.selected.is_some() {
            self.close_video(ctx);
        }
        self.browse_section = section;
        if section == BrowseSection::Home {
            self.feed_filter = FeedFilter::All;
            self.invalidate_feed_cache();
        }
    }

    fn shortcuts_window_ui(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        let mut open = self.shortcuts_open;
        egui::Window::new("Atalhos e acessibilidade")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.label(
                    "Use Tab e Shift+Tab para percorrer controles. Enter ou Espaço ativa o item focado.",
                );
                ui.separator();
                ui.label(RichText::new("Reprodução").strong());
                for (key, action) in [
                    ("Espaço ou K", "reproduzir/pausar"),
                    ("← / →", "voltar ou avançar 10 segundos"),
                    ("M", "silenciar"),
                    ("T", "modo teatro"),
                    ("F", "tela cheia"),
                    ("Esc", "fechar tela cheia ou painel"),
                ] {
                    ui.horizontal(|ui| {
                        ui.monospace(key);
                        ui.label(action);
                    });
                }
                ui.separator();
                ui.label(RichText::new("Navegação").strong());
                for (key, action) in [
                    ("Alt+1", "Início"),
                    ("Alt+2", "Explorar"),
                    ("Alt+3", "Histórico"),
                    ("Alt+4", "Fila"),
                    ("Alt+5", "Biblioteca"),
                ] {
                    ui.horizontal(|ui| {
                        ui.monospace(key);
                        ui.label(action);
                    });
                }
                ui.separator();
                ui.label(RichText::new("Legendas").strong());
                ui.label(
                    "Em Transcrição, o trecho atual acompanha o vídeo, a fonte pode ser ampliada e o modo de alto contraste fica disponível na própria aba.",
                );
                if ui.button("Fechar").clicked() {
                    self.shortcuts_open = false;
                }
            });
        self.shortcuts_open = open;
    }

    fn handle_help_shortcuts(&mut self, ctx: &egui::Context) {
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
            self.shortcuts_open = !self.shortcuts_open;
        }
        if escape_pressed && self.shortcuts_open {
            self.shortcuts_open = false;
        }
    }

    fn header_actions_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        show_back_button: bool,
    ) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            if self.session_authenticated {
                let _ = paint_channel_avatar(ui, "Voc\u{00ea}", 30.0);
            } else {
                let button = egui::Button::new(
                    RichText::new("Entrar")
                        .size(12.5)
                        .strong()
                        .color(palette.header_button_fg),
                )
                .fill(palette.header_button_bg)
                .corner_radius(16);
                if ui
                    .add_sized([64.0, 32.0], button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(
                        "Entrar com Google para carregar inscri\u{00e7}\u{00f5}es e playlists",
                    )
                    .clicked()
                {
                    self.start_browser_feed(":ytrec".into(), true, ctx);
                }
            }
            ui.add_space(4.0);
            let help_button = egui::Button::new(
                RichText::new("?")
                    .size(16.0)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(if self.shortcuts_open {
                palette.active_button_bg
            } else {
                palette.header_button_bg
            })
            .corner_radius(18);
            if ui
                .add_sized([36.0, 36.0], help_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Atalhos e acessibilidade (F1)")
                .clicked()
            {
                self.shortcuts_open = !self.shortcuts_open;
            }
            ui.add_space(4.0);

            let has_alert = self.settings_error.is_some()
                || self.playback_settings_error.is_some()
                || self.theme_settings_error.is_some()
                || self.has_pending_buffer_restart();
            let settings_button =
                egui::Button::new(RichText::new("\u{2699}").color(if has_alert {
                    palette.warning_fg
                } else if self.settings_open {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                }))
                .fill(if self.settings_open {
                    palette.active_button_bg
                } else {
                    palette.header_button_bg
                })
                .corner_radius(18);
            let settings_tip = format!(
                "Configura\u{00e7}\u{00f5}es do player (buffer: {} \u{2022} tema: {})",
                buffer_profile_summary(self.buffer_profile),
                theme_name(self.theme)
            );
            if ui
                .add_sized([36.0, 36.0], settings_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(settings_tip)
                .clicked()
            {
                self.settings_open = !self.settings_open;
                self.sync_player_window_visibility();
            }

            if show_back_button {
                let back_button = egui::Button::new(RichText::new("\u{2190}").size(17.0))
                    .fill(palette.header_button_bg)
                    .corner_radius(18);
                if ui
                    .add_sized([36.0, 36.0], back_button)
                    .on_hover_text("Fechar reprodu\u{00e7}\u{00e3}o e voltar")
                    .clicked()
                {
                    self.close_video(ctx);
                }
            }
        });
    }

    fn header_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        let palette = self.palette();
        let viewport_width = ui.available_width();
        let layout_mode = compute_header_layout_mode(viewport_width);
        let show_back_button = self.selected.is_some() && viewport_width > 720.0;
        ui.add_space(8.0);

        match layout_mode {
            HeaderLayoutMode::SingleRow => {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    self.header_menu_button_ui(ui);
                    ui.add_space(6.0);
                    if self.header_brand_ui(ui, palette) {
                        self.handle_header_home(ctx);
                    }

                    let reserve_right = if show_back_button { 312.0 } else { 236.0 };
                    let (leading_space, search_width) =
                        compute_header_search_layout(ui.available_width(), reserve_right);
                    ui.add_space(leading_space);
                    self.header_search_ui(ui, ctx, palette, search_width);
                    if ui.available_width() >= 52.0 {
                        ui.add_space(8.0);
                        self.header_recommendation_ui(ui, ctx, palette);
                    }
                    self.header_actions_ui(ui, ctx, palette, show_back_button);
                });
            }
            HeaderLayoutMode::TwoRows => {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    self.header_menu_button_ui(ui);
                    ui.add_space(6.0);
                    if self.header_brand_ui(ui, palette) {
                        self.handle_header_home(ctx);
                    }
                    self.header_actions_ui(ui, ctx, palette, show_back_button);
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let available = (ui.available_width() - 52.0).max(220.0);
                    let search_width = available.min(640.0);
                    let leftover = (ui.available_width() - search_width - 48.0).max(0.0);
                    ui.add_space(leftover * 0.5);
                    self.header_search_ui(ui, ctx, palette, search_width);
                    ui.add_space(8.0);
                    self.header_recommendation_ui(ui, ctx, palette);
                });
            }
        }
        ui.add_space(4.0);
    }

    fn navigation_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        self.ensure_history_synchronized();
        let palette = self.palette();

        egui::ScrollArea::vertical()
            .id_salt("youtube_left_sidebar_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                self.navigation_primary_ui(ui, ctx, palette);
                self.navigation_subscriptions_ui(ui, ctx, palette);
                self.navigation_shortcuts_ui(ui, ctx, palette);
                self.navigation_utilities_ui(ui, ctx, palette);
                self.navigation_now_playing_ui(ui, ctx, palette);
            });
    }

    fn navigation_primary_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let in_home = self.selected.is_none()
            && !self.settings_open
            && !self.channel_view_open
            && self.browse_section == BrowseSection::Home
            && !self.search_results_mode
            && !matches!(self.feed_filter, FeedFilter::Shorts);

        if sidebar_pill_item(ui, "⌂", "Início", in_home, false, &palette).clicked() {
            self.settings_open = false;
            self.search_results_mode = false;
            self.browse_section = BrowseSection::Home;
            self.feed_filter = FeedFilter::All;
            self.invalidate_feed_cache();
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            } else {
                self.sync_player_window_visibility();
            }
            if self.home_search.is_none() && self.search.is_none() {
                if self.session_authenticated {
                    self.start_browser_feed(":ytrec".into(), false, ctx);
                } else {
                    self.start_public_home(ctx);
                }
            }
        }

        ui.add_space(2.0);
        let in_shorts = self.selected.is_none()
            && !self.settings_open
            && !self.channel_view_open
            && (self.browse_section == BrowseSection::Shorts
                || matches!(self.feed_filter, FeedFilter::Shorts));
        if sidebar_pill_item(ui, "⚡", "Shorts", in_shorts, false, &palette).clicked() {
            self.settings_open = false;
            self.search_results_mode = false;
            self.browse_section = BrowseSection::Shorts;
            self.feed_filter = FeedFilter::Shorts;
            self.invalidate_feed_cache();
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
        }

        ui.add_space(2.0);
        let in_subscriptions = self.selected.is_none()
            && !self.settings_open
            && !self.channel_view_open
            && self.browse_section == BrowseSection::Subscriptions;
        if sidebar_pill_item(ui, "▤", "Inscrições", in_subscriptions, false, &palette).clicked()
        {
            self.settings_open = false;
            self.search_results_mode = false;
            self.browse_section = BrowseSection::Subscriptions;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.start_browser_feed(":ytrec".into(), !self.session_authenticated, ctx);
        }

        ui.add_space(2.0);
        let in_explore = self.selected.is_none()
            && !self.settings_open
            && !self.channel_view_open
            && self.browse_section == BrowseSection::Explore;
        if sidebar_pill_item(ui, "⌕", "Explorar", in_explore, false, &palette).clicked() {
            self.settings_open = false;
            self.search_results_mode = false;
            self.browse_section = BrowseSection::Explore;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
        }
    }

    fn navigation_subscriptions_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        // Seção "Inscrições >" igual ao YouTube
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("Inscrições  ›").size(15.0).strong());
        });
        ui.add_space(4.0);

        let sidebar_channels = self.subscription_channels.clone();
        if !self.session_authenticated {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Entre para ver os canais que voc\u{00ea} segue")
                        .small()
                        .weak(),
                );
            });
        } else if sidebar_channels.is_empty()
            && !self.subscription_channels_loaded
            && self.home_search.is_some()
        {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.spinner();
                ui.label(
                    RichText::new("Carregando inscri\u{00e7}\u{00f5}es...")
                        .small()
                        .weak(),
                );
            });
        } else if sidebar_channels.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                let text = if self.subscription_channels_error.is_some() {
                    "N\u{00e3}o foi poss\u{00ed}vel carregar as inscri\u{00e7}\u{00f5}es."
                } else {
                    "Voc\u{00ea} ainda n\u{00e3}o segue nenhum canal."
                };
                ui.label(RichText::new(text).small().weak());
            });
        } else {
            let visible_limit = if self.sidebar_subscriptions_expanded {
                sidebar_channels.len()
            } else {
                sidebar_channels.len().min(7)
            };
            let mut clicked_channel = None;
            for channel in sidebar_channels.iter().take(visible_limit) {
                let active = matches!(
                    self.current_channel_target(),
                    Some(ChannelTarget::ChannelId(channel_id))
                        if channel_id == channel.channel_id
                );
                if sidebar_channel_item(ui, &channel.title, active, &palette).clicked() {
                    clicked_channel = Some(channel.clone());
                }
                ui.add_space(2.0);
            }
            if let Some(channel) = clicked_channel {
                self.settings_open = false;
                self.search_results_mode = false;
                self.browse_section = BrowseSection::Home;
                self.close_channel_view();
                if self.selected.is_some() {
                    self.close_video(ctx);
                }
                self.open_channel(ChannelTarget::ChannelId(channel.channel_id), ctx);
            }

            if sidebar_channels.len() > 7 {
                let toggle_label = if self.sidebar_subscriptions_expanded {
                    "Mostrar menos"
                } else {
                    "Mostrar mais"
                };
                let toggle_icon = if self.sidebar_subscriptions_expanded {
                    "\u{2302}"
                } else {
                    "\u{2304}"
                };
                if sidebar_pill_item(ui, toggle_icon, toggle_label, false, false, &palette)
                    .clicked()
                {
                    self.sidebar_subscriptions_expanded = !self.sidebar_subscriptions_expanded;
                }
            }
        }
    }

    fn navigation_shortcuts_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        // Seção "Você >" igual ao YouTube
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("Você  ›").size(15.0).strong());
        });
        ui.add_space(4.0);

        let channel_target = self.current_channel_target();
        let channel_active = self.channel_view_open && !self.settings_open;
        if sidebar_pill_item(ui, "👤", "Seu canal", channel_active, false, &palette).clicked() {
            self.settings_open = false;
            self.search_results_mode = false;
            if channel_target.is_some() {
                self.channel_view_open = true;
                self.sync_player_window_visibility();
            } else {
                self.browse_section = BrowseSection::Home;
                self.feed_filter = FeedFilter::Recommended;
                self.invalidate_feed_cache();
            }
        }

        ui.add_space(2.0);
        let history_active = self.browse_section == BrowseSection::History
            && !self.channel_view_open
            && self.selected.is_none();
        let history_label = if self.history.entries().is_empty() {
            "Histórico".to_owned()
        } else {
            format!("Histórico ({})", self.history.entries().len())
        };
        if sidebar_pill_item(ui, "🕘", &history_label, history_active, false, &palette).clicked()
        {
            self.settings_open = false;
            self.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.browse_section = if self.browse_section == BrowseSection::History {
                BrowseSection::Home
            } else {
                BrowseSection::History
            };
        }

        ui.add_space(2.0);
        let playlists_active = self.home_title.as_deref() == Some("Playlists da conta Google")
            || self.home_title.as_deref() == Some("Vídeos da playlist");
        if sidebar_pill_item(ui, "☰", "Playlists", playlists_active, false, &palette).clicked() {
            self.settings_open = false;
            self.search_results_mode = false;
            self.browse_section = BrowseSection::Home;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            let interactive = !self.session_authenticated;
            self.start_browser_feed(":google-playlists".into(), interactive, ctx);
        }

        ui.add_space(2.0);
        let queue_active = self.browse_section == BrowseSection::Queue
            && !self.channel_view_open
            && self.selected.is_none();
        let queue_label = if self.queue.is_empty() {
            "Fila de reprodução".to_owned()
        } else {
            format!("Fila ({}/{})", self.queue.len(), self.queue.limit())
        };
        if sidebar_pill_item(ui, "⏭", &queue_label, queue_active, false, &palette).clicked() {
            self.settings_open = false;
            self.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.browse_section = if self.browse_section == BrowseSection::Queue {
                BrowseSection::Home
            } else {
                BrowseSection::Queue
            };
        }
    }

    fn navigation_utilities_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        ui.add_space(2.0);
        if sidebar_pill_item(
            ui,
            "▣",
            "Biblioteca",
            self.browse_section == BrowseSection::Library && self.selected.is_none(),
            false,
            &palette,
        )
        .clicked()
        {
            self.settings_open = false;
            self.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.browse_section = BrowseSection::Library;
        }

        ui.add_space(2.0);
        if sidebar_pill_item(
            ui,
            "⚙",
            "Configurações",
            self.settings_open,
            false,
            &palette,
        )
        .clicked()
        {
            self.settings_open = !self.settings_open;
            self.sync_player_window_visibility();
        }

        ui.add_space(2.0);
        if self.home_search.is_none()
            && sidebar_pill_item(ui, "🚪", "Sair da conta", false, false, &palette).clicked()
        {
            self.close_video(ctx);
            let persistence_error = self.set_oauth_session(None);
            self.session_authenticated = false;
            self.home_title = None;
            self.home_playlist_urls.clear();
            self.pending_home_items.clear();
            self.subscription_channels.clear();
            self.subscription_channels_loaded = false;
            self.subscription_channels_error = None;
            self.apply_search_results(ctx, Vec::new());
            self.browse_section = BrowseSection::Home;
            self.feed_filter = FeedFilter::Recommended;
            let logout_error = persistence_error.unwrap_or_default();
            self.start_public_home(ctx);
            if !logout_error.is_empty() {
                self.message = logout_error;
            }
        }
    }

    fn navigation_now_playing_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        if let Some(video) = self.selected.clone() {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            let playing_button = egui::Button::new(
                RichText::new("▶  Assistindo")
                    .strong()
                    .color(palette.playing_nav_fg),
            )
            .fill(palette.playing_nav_bg)
            .corner_radius(10);
            if ui
                .add_sized([ui.available_width(), 34.0], playing_button)
                .on_hover_text(&video.title)
                .clicked()
            {
                self.settings_open = false;
                self.search_results_mode = false;
                self.sync_player_window_visibility();
            }

            ui.add_space(4.0);
            let theater_label = if self.theater_mode {
                "▭  Sair do teatro"
            } else {
                "▭  Modo teatro"
            };
            if ui
                .add_sized(
                    [ui.available_width(), 30.0],
                    egui::Button::new(RichText::new(theater_label).small()).corner_radius(10),
                )
                .on_hover_text("Alternar modo teatro (T)")
                .clicked()
            {
                self.toggle_theater_mode();
            }

            ui.add_space(4.0);
            if ui
                .add_sized(
                    [ui.available_width(), 30.0],
                    egui::Button::new(RichText::new("✕  Fechar vídeo").small()).corner_radius(10),
                )
                .on_hover_text("Encerrar reprodução atual")
                .clicked()
            {
                self.close_video(ctx);
            }
        }
    }

    fn compact_navigation_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let palette = self.palette();
        ui.set_min_width(COMPACT_NAVIGATION_WIDTH);
        ui.set_max_width(COMPACT_NAVIGATION_WIDTH);
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            let home_active = self.selected.is_none()
                && !self.settings_open
                && !self.channel_view_open
                && self.browse_section == BrowseSection::Home
                && !self.search_results_mode
                && !matches!(self.feed_filter, FeedFilter::Shorts);
            if compact_sidebar_item(ui, "\u{2302}", "In\u{00ed}cio", home_active, &palette)
                .clicked()
            {
                self.settings_open = false;
                self.search_results_mode = false;
                self.browse_section = BrowseSection::Home;
                self.feed_filter = FeedFilter::All;
                self.invalidate_feed_cache();
                self.close_channel_view();
                if self.selected.is_some() {
                    self.close_video(ctx);
                }
                if self.home_search.is_none() && self.search.is_none() {
                    self.start_public_home(ctx);
                }
            }

            let shorts_active = self.browse_section == BrowseSection::Shorts
                || matches!(self.feed_filter, FeedFilter::Shorts);
            if compact_sidebar_item(ui, "\u{26a1}", "Shorts", shorts_active, &palette).clicked() {
                self.settings_open = false;
                self.search_results_mode = false;
                self.browse_section = BrowseSection::Shorts;
                self.feed_filter = FeedFilter::Shorts;
                self.invalidate_feed_cache();
                self.close_channel_view();
                if self.selected.is_some() {
                    self.close_video(ctx);
                }
            }

            let subscriptions_active = self.selected.is_none()
                && !self.settings_open
                && !self.channel_view_open
                && self.browse_section == BrowseSection::Subscriptions;
            if compact_sidebar_item(
                ui,
                "\u{25a4}",
                "Inscri\u{00e7}\u{00f5}es",
                subscriptions_active,
                &palette,
            )
            .clicked()
            {
                self.settings_open = false;
                self.search_results_mode = false;
                self.browse_section = BrowseSection::Subscriptions;
                self.close_channel_view();
                if self.selected.is_some() {
                    self.close_video(ctx);
                }
                self.start_browser_feed(":ytrec".into(), !self.session_authenticated, ctx);
            }

            let you_active = self.selected.is_none()
                && !self.settings_open
                && matches!(
                    self.browse_section,
                    BrowseSection::History | BrowseSection::Library | BrowseSection::Queue
                );
            if compact_sidebar_item(ui, "\u{25cb}", "Voc\u{00ea}", you_active, &palette).clicked() {
                self.settings_open = false;
                self.search_results_mode = false;
                self.browse_section = BrowseSection::History;
                self.close_channel_view();
                if self.selected.is_some() {
                    self.close_video(ctx);
                }
            }
        });
    }

    fn watch_navigation_drawer_ui(&mut self, ctx: &egui::Context) {
        let content = ctx.content_rect();
        let width = layout::compute_youtube_sidebar_width(content.width());
        let top = content.top() + compute_header_height(content.width(), false);
        let height = (content.bottom() - top).max(120.0);
        let fill = if self.theme == Theme::Dark {
            Color32::from_rgb(15, 15, 15)
        } else {
            Color32::from_rgb(255, 255, 255)
        };
        let stroke = if self.theme == Theme::Dark {
            Color32::from_rgb(48, 48, 48)
        } else {
            Color32::from_rgb(222, 222, 222)
        };
        egui::Area::new(egui::Id::new("watch_navigation_drawer"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(content.left(), top))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, stroke))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(width, height));
                        ui.set_max_width(width);
                        ui.set_height(height);
                        self.navigation_ui(ui, ctx);
                    });
            });
        if self.selected.is_none() {
            self.navigation_open = false;
        }
    }

    fn settings_panel_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.settings_panel_diagnostic_ui(ui);
        self.sync_theme_visuals(&ctx);
        self.ensure_playback_settings_synchronized();
        self.settings_panel_feed_ui(ui, &ctx);
        self.settings_panel_backup_ui(ui);
        self.settings_panel_appearance_ui(ui, &ctx);
        let palette = self.palette();
        self.settings_panel_player_ui(ui, palette);
        self.settings_panel_buffer_ui(ui, palette);
        self.settings_panel_storage_status_ui(ui, palette);
        self.settings_panel_decoding_diagnostics_ui(ui, palette);
    }

    fn settings_panel_diagnostic_ui(&mut self, ui: &mut egui::Ui) {
        if !self.message.is_empty() {
            let kind = diagnostics::classify_failure(&self.message);
            let guidance =
                diagnostics::guidance(kind, self.session_authenticated, self.selected.is_some());
            let category = match kind {
                diagnostics::FailureKind::Login => "Login",
                diagnostics::FailureKind::Quota => "Cota da API",
                diagnostics::FailureKind::Connection => "Conexão",
                diagnostics::FailureKind::Playback => "Reprodução/dependências",
                diagnostics::FailureKind::Unknown => "Falha não classificada",
            };
            ui.group(|ui| {
                ui.strong(format!("Diagnóstico: {category}"));
                ui.label(guidance.summary);
                ui.add_space(3.0);
                ui.label(RichText::new("Próximos passos").small().strong());
                for step in guidance.steps {
                    ui.label(RichText::new(format!("- {step}")).small());
                }
                if ui.button("Copiar relatório redigido").clicked() {
                    ui.ctx().copy_text(diagnostics::safe_report(
                        kind,
                        self.session_authenticated,
                        self.selected.is_some(),
                    ));
                }
            });
        }
    }

    fn settings_panel_feed_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.ensure_feed_cache();
        ui.heading("Feed e memória");
        ui.label(
            "As recomendações usam somente o histórico, a fila e o contexto de reprodução já armazenados neste computador. O FastTube carrega metadados de até 20 resultados por descoberta e limita a retenção visual para reduzir picos de RAM.",
        );
        let profile = self.feed_cache.profile.clone();
        let topics = profile.top_topics(4);
        ui.label(
            RichText::new(if topics.is_empty() {
                "Interesses: ainda aprendendo".to_owned()
            } else {
                format!("Interesses: {}", topics.join(" · "))
            })
            .small()
            .color(self.palette().info_accent_fg),
        );
        ui.label(
            RichText::new(format!(
                "Em memória agora: {} vídeos, {} metadados, {} itens na fila.",
                self.videos.len(),
                self.video_metadata.len(),
                self.queue.len()
            ))
            .small()
            .weak(),
        );
        if ui
            .button("Liberar caches visuais")
            .on_hover_text("Descarta texturas de miniaturas e resoluções carregadas; mantém o feed e os dados locais")
            .clicked()
        {
            self.clear_visual_caches(ctx);
        }
    }

    fn settings_panel_backup_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Backup de dados locais");
        ui.label("Inclui biblioteca, marcadores, fila, histórico e preferências. Sessões OAuth, tokens e cookies ficam excluídos.");
        ui.horizontal(|ui| {
            ui.label("Arquivo:");
            ui.text_edit_singleline(&mut self.backup_path_text);
        });
        ui.horizontal(|ui| {
            if ui.button("Exportar").clicked() {
                self.export_user_data();
            }
            if ui
                .button("Analisar importação")
                .on_hover_text(
                    "Valida o arquivo e mostra uma prévia antes de alterar qualquer dado.",
                )
                .clicked()
            {
                self.analyze_import();
            }
        });
        if self.backup_confirm_import {
            let preview = self.backup_preview.clone();
            egui::Window::new("Prévia da importação")
                .collapsible(false)
                .resizable(false)
                .default_width(440.0)
                .show(ui.ctx(), |ui| {
                    let Some(preview) = preview.as_ref() else {
                        ui.label("A prévia não está disponível.");
                        if ui.button("Fechar").clicked() {
                            self.backup_confirm_import = false;
                        }
                        return;
                    };
                    ui.label(
                        RichText::new(
                            preview
                                .path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("backup selecionado"),
                        )
                        .strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(format!(
                        "Arquivo: {} itens na biblioteca ({} favoritos, {} Assistir depois, {} coleções, {} marcadores), {} na fila e {} no histórico.",
                        preview.library_items,
                        preview.favorites,
                        preview.watch_later,
                        preview.collections,
                        preview.bookmarks,
                        preview.queue_items,
                        preview.history_entries
                    ));
                    ui.separator();
                    ui.label(RichText::new("Como aplicar").strong());
                    ui.radio_value(
                        &mut self.backup_import_mode,
                        BackupImportMode::Merge,
                        "Mesclar com os dados locais (recomendado)",
                    );
                    ui.label(
                        RichText::new(
                            "Adiciona itens ausentes sem apagar os atuais. A biblioteca local prevalece em conflitos, o histórico mantém o maior progresso e a fila respeita o limite de 50.",
                        )
                        .small()
                        .weak(),
                    );
                    ui.radio_value(
                        &mut self.backup_import_mode,
                        BackupImportMode::Replace,
                        "Substituir todos os dados",
                    );
                    ui.label(
                        RichText::new(
                            "Remove a visão atual de biblioteca, fila, histórico e preferências e aplica somente o backup.",
                        )
                        .small()
                        .weak(),
                    );
                    ui.separator();
                    match self.backup_import_mode {
                        BackupImportMode::Merge => {
                            ui.label(format!(
                                "Prévia: {} itens novos, {} conflitos mantidos e {} excedentes do limite.",
                                preview.merge_added,
                                preview.merge_conflicts,
                                preview.merge_skipped
                            ));
                            if preview.history_progress_updates > 0 {
                                ui.label(format!(
                                    "{} entradas do histórico terão progresso atualizado.",
                                    preview.history_progress_updates
                                ));
                            }
                        }
                        BackupImportMode::Replace => {
                            ui.label(
                                "As preferências do arquivo também substituirão tema, velocidade, qualidade e buffer.",
                            );
                        }
                    }
                    ui.label(
                        RichText::new(
                            "Antes da gravação, o FastTube cria um ponto de restauração. Se a operação falhar ou for interrompida, os arquivos anteriores serão recuperados.",
                        )
                        .small(),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Aplicar backup").clicked() {
                            self.backup_confirm_import = false;
                            self.import_user_data();
                        }
                        if ui.button("Cancelar").clicked() {
                            self.backup_confirm_import = false;
                            self.backup_preview = None;
                            self.pending_import = None;
                            self.backup_status = Some("Importação cancelada; dados atuais preservados.".into());
                        }
                    });
                });
        }
        if let Some(status) = &self.backup_status {
            ui.label(status);
        }
    }

    fn settings_panel_appearance_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.separator();
        ui.heading("Aparência");
        ui.horizontal(|ui| {
            ui.label("Tema da interface:");
            let mut selected_theme = self.theme;
            ui.selectable_value(&mut selected_theme, Theme::Dark, "Escuro (padrão)");
            ui.selectable_value(&mut selected_theme, Theme::Light, "Claro");
            if selected_theme != self.theme {
                self.apply_theme(selected_theme);
                self.sync_theme_visuals(ctx);
            }
        });
        let palette = self.palette();
        ui.label(
            RichText::new(
                "Aplicado imediatamente aos painéis e cartões da interface (a superfície de vídeo e o slot 16:9 das miniaturas permanecem escuros).",
            )
            .small()
            .weak(),
        );
        if let Some(error) = &self.theme_settings_error {
            ui.label(RichText::new(error).color(palette.warning_fg));
        }
    }

    fn settings_panel_player_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette) {
        ui.separator();
        ui.heading("Configurações do player");
        ui.label("Preferências para os próximos vídeos");
        let mut preferences = self.playback_preferences;
        egui::ComboBox::from_id_salt("default_speed")
            .selected_text(format!(
                "Velocidade padrão: {:.2}×",
                f64::from(preferences.speed_quarters) / 4.0
            ))
            .show_ui(ui, |ui| {
                for speed in 1..=8 {
                    ui.selectable_value(
                        &mut preferences.speed_quarters,
                        speed,
                        format!("{:.2}×", f64::from(speed) / 4.0),
                    );
                }
            });
        egui::ComboBox::from_id_salt("default_quality")
            .selected_text(
                preferences
                    .max_height
                    .map(|h| format!("Qualidade padrão: até {h}p"))
                    .unwrap_or_else(|| "Qualidade padrão: automática".into()),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut preferences.max_height, None, "Automática (até 1080p)");
                for height in [360, 480, 720, 1080, 1440, 2160] {
                    ui.selectable_value(
                        &mut preferences.max_height,
                        Some(height),
                        format!("Até {height}p"),
                    );
                }
            });
        ui.label(RichText::new("A resolução depende dos formatos disponíveis; se não houver formato dentro do limite, será usado o melhor disponível. A resolução real aparece no player quando conhecida.").small().weak());
        if preferences != self.playback_preferences {
            self.apply_playback_preferences(preferences);
        }
        if let Some(error) = &self.playback_settings_error {
            ui.label(RichText::new(error).color(palette.warning_fg));
        }
    }

    fn settings_panel_buffer_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette) {
        ui.separator();
        ui.heading("Buffer do player");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Escolha o perfil de cache do demuxer do libmpv. O perfil Padrão (64 MiB de avanço, 16 MiB de retorno e 30s de cache) equilibra consumo moderado de RAM com boa estabilidade de reprodução.",
            )
            .weak(),
        );
        if self.selected.is_some() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Enquanto esta janela está aberta, a superfície nativa do vídeo fica oculta temporariamente para não cobrir o menu, sem pausar nem encerrar a reprodução.",
                )
                .small()
                .color(palette.info_accent_fg),
            );
        }
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(palette.info_box_bg)
            .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.label(RichText::new("Trade-off entre memória e rede:").small().strong());
                ui.label(
                    RichText::new(
                        "• Buffer menor reduz o uso de memória RAM, mas oferece menor folga contra oscilações de rede ou picos de bitrate.\n• Buffer maior aumenta a tolerância a conexões instáveis e buscas rápidas no vídeo, em troca de maior retenção de RAM durante a reprodução.",
                    )
                    .small()
                    .weak(),
                );
            });

        ui.add_space(10.0);
        let profiles = [
            (
                BufferProfile::Small,
                "Pequeno — 32 MiB máx / 8 MiB retorno / 15s",
                "Menor uso de memória RAM; recomendado para máquinas com pouca memória e conexão estável.",
            ),
            (
                BufferProfile::Default,
                "Padrão — 64 MiB máx / 16 MiB retorno / 30s (recomendado)",
                "Equilíbrio padrão do FastTube entre baixo consumo de memória e fluidez na reprodução.",
            ),
            (
                BufferProfile::Large,
                "Grande — 128 MiB máx / 32 MiB retorno / 60s",
                "Maior folga de cache para conexões instáveis ou vídeos longos, com maior consumo de RAM.",
            ),
        ];

        let mut chosen_profile = None;
        for (profile, title, description) in profiles {
            let is_selected = self.buffer_profile == profile;
            let fill = if is_selected {
                palette.profile_selected_bg
            } else {
                palette.profile_unselected_bg
            };
            let stroke = if is_selected {
                egui::Stroke::new(1.0, palette.profile_selected_stroke)
            } else {
                egui::Stroke::new(1.0, palette.profile_unselected_stroke)
            };
            egui::Frame::new()
                .fill(fill)
                .stroke(stroke)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let mut selected = is_selected;
                    if ui
                        .radio_value(&mut selected, true, RichText::new(title).strong())
                        .clicked()
                        && !is_selected
                    {
                        chosen_profile = Some(profile);
                    }
                    ui.label(RichText::new(description).small().weak());
                });
            ui.add_space(6.0);
        }

        if let BufferProfile::Custom(values) = self.buffer_profile {
            egui::Frame::new()
                .fill(palette.profile_selected_bg)
                .stroke(egui::Stroke::new(1.0, palette.profile_selected_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new(format!(
                            "Personalizado (carregado do arquivo) — {} MiB máx / {} MiB retorno / {}s",
                            values.max_mib, values.back_mib, values.cache_seconds
                        ))
                        .strong(),
                    );
                    ui.label(
                        RichText::new(
                            "Perfil personalizado ativo. Selecionar Pequeno, Padrão ou Grande substituirá este perfil no arquivo.",
                        )
                        .small()
                        .weak(),
                    );
                });
            ui.add_space(6.0);
        }

        if let Some(profile) = chosen_profile {
            self.apply_buffer_profile(profile);
        }

        ui.add_space(4.0);
        egui::Frame::new()
            .fill(palette.info_box_bg)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("Quando a configuração entra em vigor:").small().strong());
                if let Some(active) = self.active_player_buffer {
                    if active.values() != self.buffer_profile.values() {
                        ui.label(
                            RichText::new(format!(
                                "⚠ O player em execução foi iniciado com {} e o perfil selecionado agora é {}. Como as opções de buffer do libmpv são aplicadas na criação do player e a instância é reutilizada enquanto aberta, feche o vídeo atual (✕ Fechar vídeo / botão Resultados) ou reinicie o player antes de abrir o próximo vídeo para aplicar o novo buffer.",
                                buffer_profile_summary(active),
                                buffer_profile_summary(self.buffer_profile)
                            ))
                            .small()
                            .color(palette.warning_fg),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!(
                                "Player ativo com {}. Se você alterar o perfil durante a reprodução, a mudança só entrará em vigor após fechar o vídeo atual (reiniciar o player) e abrir o próximo vídeo.",
                                buffer_profile_summary(active)
                            ))
                            .small()
                            .weak(),
                        );
                    }
                } else {
                    ui.label(
                        RichText::new(format!(
                            "O player está inativo. O perfil {} será aplicado imediatamente ao iniciar o player no próximo vídeo aberto.",
                            buffer_profile_summary(self.buffer_profile)
                        ))
                        .small()
                        .weak(),
                    );
                }
            });
    }

    fn settings_panel_storage_status_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette) {
        if let Some(notice) = &self.settings_notice {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(palette.notice_bg)
                .stroke(egui::Stroke::new(1.0, palette.notice_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(notice).small().color(palette.notice_fg));
                });
        }

        if let Some(error) = &self.settings_error {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new(format!("Erro de armazenamento: {error}"))
                            .small()
                            .color(palette.error_banner_fg),
                    );
                });
        }

        if let Some(path) = &self.settings_path {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("Arquivo de persistência: {}", path.display()))
                    .small()
                    .weak(),
            );
        }
    }

    fn settings_panel_decoding_diagnostics_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette) {
        if self.selected.is_none() {
            self.reset_decoding_diagnostics();
        }
        let diag_display =
            format_diagnostics_display(&self.decoding_diagnostics, self.selected.is_some());
        let status_color = if !self.selected.is_some() {
            palette.diag_inactive_fg
        } else if self
            .decoding_diagnostics
            .effective_decoding
            .is_hardware_active()
        {
            palette.diag_hw_fg
        } else if self.decoding_diagnostics.effective_decoding.is_software() {
            palette.diag_sw_fg
        } else {
            palette.diag_unknown_fg
        };

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(
            RichText::new("Diagnóstico de decodificação de vídeo (libmpv)")
                .size(15.0)
                .strong(),
        );
        ui.add_space(4.0);
        egui::Frame::new()
            .fill(palette.info_box_bg)
            .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new(&diag_display.status_line)
                        .small()
                        .strong()
                        .color(status_color),
                );
                ui.add_space(2.0);
                ui.label(RichText::new(&diag_display.configured_line).small());
                ui.label(RichText::new(&diag_display.effective_line).small());
                if let Some(meta_line) = &diag_display.metadata_line {
                    ui.label(RichText::new(meta_line).small());
                } else {
                    ui.label(
                        RichText::new(if self.selected.is_some() {
                            "Metadados da faixa (video-codec / tamanho): indisponíveis no momento"
                        } else {
                            "Metadados da faixa (video-codec / tamanho): inativo (sem vídeo aberto)"
                        })
                        .small()
                        .weak(),
                    );
                }
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("Resumo: {}", diag_display.summary_line))
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);
                ui.label(RichText::new(diag_display.note_line).small().weak());
            });
    }

    fn settings_window_ui(&mut self, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        if !self.settings_open {
            self.sync_player_window_visibility();
            return;
        }
        let now = Instant::now();
        self.refresh_player_diagnostics(now);
        if self.selected.is_some() {
            ctx.request_repaint_after(DIAGNOSTICS_REFRESH_INTERVAL);
        }
        let mut open = self.settings_open;
        let top_offset = if self.fullscreen {
            24.0
        } else {
            compute_header_height(ctx.content_rect().width(), false) + 4.0
        };
        let max_scroll_height = (ctx.content_rect().height() - top_offset - 56.0).max(260.0);
        let window_width = (ctx.content_rect().width() - 32.0).clamp(320.0, 500.0);
        egui::Window::new("⚙ Configurações do FastTube")
            .open(&mut open)
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .default_width(window_width)
            .anchor(egui::Align2::CENTER_TOP, [0.0, top_offset])
            .show(ctx, |ui| {
                ui.set_max_width((window_width - 24.0).max(280.0));
                egui::ScrollArea::vertical()
                    .id_salt("settings_panel_scroll")
                    .max_height(max_scroll_height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        self.settings_panel_ui(ui);
                    });
            });
        self.settings_open = open;
        if !self.settings_open && self.selected.is_none() {
            self.reset_decoding_diagnostics();
        }
        self.sync_player_window_visibility();
    }

    fn paint_hidden_video_notice(&self, ui: &egui::Ui, rect: egui::Rect) {
        if self.settings_open {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Vídeo oculto temporariamente enquanto o menu de configurações está aberto",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(165, 165, 175),
            );
        }
    }

    #[cfg(windows)]
    fn player_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        column_width: f32,
        available_height: f32,
        mode: WatchLayoutMode,
    ) {
        let Some(video) = self.selected.clone() else {
            self.sync_player_window_visibility();
            return;
        };
        self.sync_player_window_visibility();
        let settings_open = self.settings_open;
        let fullscreen = self.fullscreen;
        let theater_mode = self.theater_mode;

        let player_size = compute_player_size_with_theater(
            column_width,
            available_height,
            fullscreen,
            theater_mode,
            mode,
        );
        let width = player_size.x;
        let height = player_size.y;
        let compact_controls = watch_controls_are_compact(width);

        let mut video_area: Option<(egui::Rect, egui::Response)> = None;
        let (
            player_events,
            poll_err,
            paused,
            muted,
            ended,
            raw_path,
            raw_position,
            raw_duration,
            raw_volume,
        ) = {
            let Some(player) = &mut self.player else {
                return;
            };
            let (player_events, poll_err) = player.poll_events();
            ctx.request_repaint_after(std::time::Duration::from_millis(250));

            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
            ui.painter().rect_filled(rect, 4.0, Color32::BLACK);
            if settings_open {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Vídeo oculto temporariamente enquanto o menu de configurações está aberto",
                    egui::FontId::proportional(13.0),
                    Color32::from_rgb(165, 165, 175),
                );
            } else if player.paint_into(ui.painter(), rect) {
                // A render API do libmpv desenha o vídeo dentro do egui, então a
                // barra sobreposta pode ficar por cima sem uma janela nativa.
                video_area = Some((rect, response));
            }
            player.resize(rect, ctx.pixels_per_point());

            (
                player_events,
                poll_err,
                player.property("pause").as_deref() == Some("yes"),
                player.property("mute").as_deref() == Some("yes"),
                player.property("eof-reached").as_deref() == Some("yes"),
                player.property("path"),
                player.number("time-pos").filter(|&pos| pos >= 0.0),
                player.number("duration").filter(|&dur| dur > 0.0),
                player.number("volume").unwrap_or(100.0).clamp(0.0, 100.0),
            )
        };
        for event in player_events {
            self.handle_queue_player_event(event);
        }
        if let Some(error) = poll_err {
            self.message = error;
            self.report_pending_queue_load_error();
        }
        ui.add_space(6.0);

        let now = Instant::now();
        let (position, duration, pending_seek, clear_start_option) = self.process_player_telemetry(
            &video,
            raw_path.as_deref(),
            raw_position,
            raw_duration,
            ended,
            now,
        );
        if !ended
            && self.pending_queue_video.as_ref() == Some(&video)
            && self.active_playback.as_ref().is_some_and(|active| {
                active.has_live_sample
                    && active.url == video.url
                    && raw_path.as_deref().is_some_and(|path| {
                        path == video.url
                            || (history::youtube_video_id(path).is_some()
                                && history::youtube_video_id(path)
                                    == history::youtube_video_id(&video.url))
                    })
            })
        {
            self.report_pending_queue_playback_observed();
        }
        if clear_start_option {
            if let Some(player) = &self.player {
                let _ = player.set("options/start", "none");
            }
        }
        if let Some(target_secs) = pending_seek {
            if let Some(player) = &self.player {
                if let Err(error) = player.seek(target_secs, "absolute+exact") {
                    if let Some(active) = &mut self.active_playback {
                        active.resume_seek_pending = true;
                    }
                    self.message = error;
                }
            }
        }

        let mut close_clicked = false;
        let mut fullscreen_toggled = false;
        let mut theater_toggled = false;
        let mut settings_toggled = false;
        let mut next_clicked = false;
        let has_queued = !self.queue.is_empty();
        let mut manual_seek_recorded: Option<f64> = None;
        let mut flush_on_pause = false;
        let mut library_changed = false;
        let palette = self.palette();
        // Na página normal, os controles ficam somente sobre o vídeo, como no YouTube.
        // A barra externa permanece disponível como fallback em tela cheia.
        if fullscreen {
            self.fullscreen_seek_ui(ui, width, position, duration, now);

            let mut close_clicked = false;
            let mut fullscreen_toggled = false;
            let mut theater_toggled = false;
            let mut settings_toggled = false;
            let mut next_clicked = false;
            let has_queued = !self.queue.is_empty();
            let mut manual_seek_recorded: Option<f64> = None;
            let mut flush_on_pause = false;
            let mut library_changed = false;
            let palette = self.palette();
            egui::Frame::new()
                .fill(palette.player_bar_bg)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    let Some(player) = &self.player else {
                        return;
                    };
                    ui.set_max_width(width);
                    ui.horizontal_wrapped(|ui| {
                        let icon = if paused || ended {
                            Icon::Play
                        } else {
                            Icon::Pause
                        };
                        if ui
                            .add(egui::Button::image(icon.image(palette.icon_fg, 18.0)))
                            .on_hover_text(if paused || ended {
                                "Reproduzir (Espaço ou K)"
                            } else {
                                "Pausar (Espaço ou K)"
                            })
                            .clicked()
                        {
                            let result = if ended {
                                player
                                    .seek(0.0, "absolute+exact")
                                    .and_then(|()| player.set("pause", "no"))
                            } else {
                                player.set("pause", if paused { "no" } else { "yes" })
                            };
                            match result {
                                Ok(()) => {
                                    if ended {
                                        manual_seek_recorded = Some(0.0);
                                    } else if !paused {
                                        flush_on_pause = true;
                                    }
                                }
                                Err(error) => self.message = error,
                            }
                        }
                        if ui
                            .add_enabled(has_queued, egui::Button::new("⏭ Próximo"))
                            .on_hover_text("Avançar manualmente para o próximo vídeo da fila")
                            .clicked()
                        {
                            next_clicked = true;
                        }
                        if ui
                            .button("−10s")
                            .on_hover_text("Retroceder 10 segundos (←)")
                            .clicked()
                        {
                            match player.seek(-10.0, "relative") {
                                Ok(()) => manual_seek_recorded = Some((position - 10.0).max(0.0)),
                                Err(error) => self.message = error,
                            }
                        }
                        if ui
                            .button("+10s")
                            .on_hover_text("Avançar 10 segundos (→)")
                            .clicked()
                        {
                            match player.seek(10.0, "relative") {
                                Ok(()) => {
                                    let next = duration
                                        .map(|dur| (position + 10.0).min(dur))
                                        .unwrap_or(position + 10.0);
                                    manual_seek_recorded = Some(next);
                                }
                                Err(error) => self.message = error,
                            }
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} / {}",
                                format_time(self.seek_drag.unwrap_or(position)),
                                duration.map(format_time).unwrap_or_else(|| "--:--".into())
                            ))
                            .small()
                            .weak(),
                        );
                        if !compact_controls {
                            ui.add_sized(
                                [90.0, 20.0],
                                egui::TextEdit::singleline(&mut self.bookmark_note)
                                    .hint_text("Nota do marcador"),
                            );
                            if ui.small_button("＋ Marcador").clicked() {
                                if let Some(id) = history::youtube_video_id(&video.url) {
                                    let mark = library::Bookmark {
                                        video_id: id,
                                        title: video.title.clone(),
                                        time_ms: (position.max(0.0) * 1000.0).round() as u64,
                                        note: std::mem::take(&mut self.bookmark_note),
                                    };
                                    if let Err(error) = self.library.add_bookmark(mark) {
                                        self.library_error =
                                            Some(format!("Marcador não salvo: {error}"));
                                    } else {
                                        library_changed = true;
                                    }
                                }
                            }
                            if ui
                                .small_button(if self.mini_player {
                                    "Sair do mini"
                                } else {
                                    "Mini player"
                                })
                                .clicked()
                            {
                                if !self.mini_player {
                                    self.mini_player_restore_size = ctx.input(|input| {
                                        input.viewport().inner_rect.map(|rect| rect.size())
                                    });
                                }
                                self.mini_player = !self.mini_player;
                                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                    if self.mini_player {
                                        egui::WindowLevel::AlwaysOnTop
                                    } else {
                                        egui::WindowLevel::Normal
                                    },
                                ));
                                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                    if self.mini_player {
                                        egui::vec2(720.0, 450.0)
                                    } else {
                                        self.mini_player_restore_size
                                            .take()
                                            .unwrap_or_else(|| egui::vec2(1280.0, 800.0))
                                    },
                                ));
                            }
                        }
                        ui.add_space(6.0);
                        let sound_icon = if muted { Icon::Muted } else { Icon::Volume };
                        if ui
                            .add(egui::Button::image(sound_icon.image(palette.icon_fg, 17.0)))
                            .on_hover_text("Silenciar ou ativar som (M)")
                            .clicked()
                        {
                            if let Err(error) = player.set("mute", if muted { "no" } else { "yes" })
                            {
                                self.message = error;
                            }
                        }
                        let mut volume = player.number("volume").unwrap_or(100.0).clamp(0.0, 100.0);
                        if ui
                            .add_sized(
                                [76.0, 20.0],
                                egui::Slider::new(&mut volume, 0.0..=100.0).show_value(false),
                            )
                            .on_hover_text(format!("Volume: {:.0}%", volume))
                            .changed()
                        {
                            if let Err(error) = player.set("volume", &format!("{volume:.0}")) {
                                self.message = error;
                            }
                        }
                        ui.separator();
                        let mut speed = player.number("speed").unwrap_or(1.0);
                        egui::ComboBox::from_id_salt("speed")
                            .selected_text(if compact_controls {
                                format!("{speed:.2}x")
                            } else {
                                format!("Velocidade {speed:.2}x")
                            })
                            .show_ui(ui, |ui| {
                                for choice in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
                                    if ui
                                        .selectable_value(
                                            &mut speed,
                                            choice,
                                            format!("{choice:.2}×"),
                                        )
                                        .changed()
                                    {
                                        if let Err(error) =
                                            player.set("speed", &format!("{choice}"))
                                        {
                                            self.message = error;
                                        }
                                    }
                                }
                            })
                            .response
                            .on_hover_text("Velocidade de reprodução");
                        let chapter_count = player
                            .number("chapter-list/count")
                            .unwrap_or(0.0)
                            .clamp(0.0, 500.0) as usize;
                        if chapter_count > 0 {
                            egui::ComboBox::from_id_salt("chapters")
                                .selected_text("Capítulos")
                                .show_ui(ui, |ui| {
                                    for index in 0..chapter_count {
                                        let title = player
                                            .property(&format!("chapter-list/{index}/title"))
                                            .unwrap_or_else(|| format!("Capítulo {}", index + 1));
                                        if ui.selectable_label(false, title).clicked() {
                                            if let Some(time) =
                                                player.number(&format!("chapter-list/{index}/time"))
                                            {
                                                let _ = player.seek(time, "absolute");
                                            }
                                        }
                                    }
                                });
                        }
                        let mut quality = self.quality;
                        let current_height = player.number("height").map(|height| height as u32);
                        let quality_label = match quality {
                            Some(height) if compact_controls => format!("{height}p"),
                            Some(height) => format!("Qualidade: {height}p"),
                            None => {
                                let preferred = self
                                    .playback_preferences
                                    .max_height
                                    .map(|height| format!("até {height}p"))
                                    .unwrap_or_else(|| "automática".into());
                                current_height
                                    .map(|height| {
                                        if compact_controls {
                                            format!("{preferred} ({height}p)")
                                        } else {
                                            format!("Qualidade: {preferred} ({height}p)")
                                        }
                                    })
                                    .unwrap_or_else(|| {
                                        format!("Qualidade: {preferred} (não confirmada)")
                                    })
                            }
                        };
                        let quality_tip = if self.quality_search.is_some() {
                            "Consultando as resoluções disponíveis...".to_owned()
                        } else {
                            self.quality_error.clone().unwrap_or_else(|| {
                                "Escolha a resolução disponível para este vídeo".into()
                            })
                        };
                        ui.add_enabled_ui(!self.qualities.is_empty(), |ui| {
                            egui::ComboBox::from_id_salt("quality")
                                .selected_text(quality_label)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut quality, None, "Automática");
                                    for &height in &self.qualities {
                                        ui.selectable_value(
                                            &mut quality,
                                            Some(height),
                                            format!("{height}p"),
                                        );
                                    }
                                })
                                .response
                                .on_hover_text(quality_tip);
                        });
                        if quality != self.quality {
                            let ready = self
                                .active_playback
                                .as_ref()
                                .is_some_and(|active| active.has_live_sample)
                                && raw_path.as_deref().is_some_and(|path| path == video.url)
                                && raw_position.is_some();
                            let result = if ready {
                                player.set("options/start", "none").and_then(|()| {
                                    player.quality(
                                        &video.url,
                                        quality,
                                        &self.playback_preferences.format(),
                                    )
                                })
                            } else {
                                Err("Aguarde o vídeo carregar antes de alterar a qualidade.".into())
                            };
                            match result {
                                Ok(()) => {
                                    self.quality = quality;
                                    self.message.clear();
                                    if let Some(active) = &mut self.active_playback {
                                        let pos_ms = (position * 1000.0).round() as u64;
                                        if pos_ms > 0 {
                                            active.resume_target_ms = Some(pos_ms);
                                            active.resume_seek_pending = true;
                                            active.start_option_cleared = false;
                                        }
                                    }
                                }
                                Err(error) => self.message = error,
                            }
                        }
                        let count = player.number("track-list/count").unwrap_or(0.0) as usize;
                        let mut subtitle_tracks = Vec::new();
                        for index in 0..count {
                            if player
                                .property(&format!("track-list/{index}/type"))
                                .as_deref()
                                == Some("sub")
                            {
                                if let Some(id) = player.property(&format!("track-list/{index}/id"))
                                {
                                    let label = player
                                        .property(&format!("track-list/{index}/title"))
                                        .filter(|title| !title.is_empty())
                                        .or_else(|| {
                                            player.property(&format!("track-list/{index}/lang"))
                                        })
                                        .unwrap_or_else(|| format!("Faixa {id}"));
                                    subtitle_tracks.push((id, label));
                                }
                            }
                        }
                        if !subtitle_tracks.is_empty() {
                            let current = player.property("sid").unwrap_or_else(|| "no".into());
                            let mut selected = current.clone();
                            let label = subtitle_tracks
                                .iter()
                                .find(|(id, _)| *id == current)
                                .map(|(_, label)| label.as_str())
                                .unwrap_or("Desativadas");
                            egui::ComboBox::from_id_salt("subtitles")
                                .selected_text(if compact_controls {
                                    label.to_owned()
                                } else {
                                    format!("Legendas: {label}")
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut selected,
                                        "no".to_owned(),
                                        "Desativadas",
                                    );
                                    for (id, label) in &subtitle_tracks {
                                        ui.selectable_value(&mut selected, id.clone(), label);
                                    }
                                });
                            if selected != current {
                                if let Err(error) = player
                                    .set("sid", &selected)
                                    .and_then(|()| player.set("sub-visibility", "yes"))
                                {
                                    self.message = error;
                                }
                            }
                        }
                        if ui
                            .button("⚙")
                            .on_hover_text("Abrir ou fechar configurações do player")
                            .clicked()
                        {
                            settings_toggled = true;
                        }
                        let theater_button_label = if compact_controls {
                            if theater_mode { "Normal" } else { "Teatro" }
                        } else if theater_mode {
                            "▭ Normal"
                        } else {
                            "▭ Teatro"
                        };
                        let theater_button = egui::Button::new(
                            RichText::new(theater_button_label).color(if theater_mode {
                                palette.active_button_fg
                            } else {
                                palette.theater_inactive_fg
                            }),
                        )
                        .fill(if theater_mode {
                            palette.active_button_bg
                        } else {
                            palette.theater_inactive_bg
                        });
                        if ui
                            .add_enabled(!fullscreen, theater_button)
                            .on_hover_text(if fullscreen {
                                "Modo teatro indisponível enquanto a tela cheia estiver ativa"
                            } else if theater_mode {
                                "Sair do modo teatro e restaurar visualização padrão (T)"
                            } else {
                                "Ativar modo teatro na janela (T)"
                            })
                            .clicked()
                        {
                            theater_toggled = true;
                        }
                        let screen_icon = if fullscreen {
                            Icon::Minimize
                        } else {
                            Icon::Maximize
                        };
                        if ui
                            .add(egui::Button::image(
                                screen_icon.image(palette.icon_fg, 17.0),
                            ))
                            .on_hover_text("Tela cheia (F ou Esc para sair)")
                            .clicked()
                        {
                            fullscreen_toggled = true;
                        }
                        if ui
                            .add(egui::Button::image(
                                Icon::Close.image(palette.icon_fg, 17.0),
                            ))
                            .on_hover_text("Fechar vídeo")
                            .clicked()
                        {
                            close_clicked = true;
                        }
                    });
                });
        }

        if library_changed {
            self.save_library();
        }

        let overlay_action = self.player_overlay_ui(
            ui, video_area, paused, ended, muted, position, duration, raw_volume, has_queued,
        );
        if let Some(action) = overlay_action {
            self.handle_player_overlay_action(
                action, paused, ended, muted, position, duration, now, ctx,
            );
        }

        if let Some(seek_pos) = manual_seek_recorded {
            self.record_manual_seek(seek_pos, duration, now);
        } else if flush_on_pause {
            self.flush_playback_history_at(now);
        }

        let resolution_badge = if !fullscreen {
            self.player.as_ref().and_then(|player| {
                VideoSize::from_numbers(player.number("width"), player.number("height"))
                    .map(|size| format!("Reproduzindo em {size}"))
            })
        } else {
            None
        };

        if !fullscreen && mode != WatchLayoutMode::Short {
            self.watch_metadata_card_ui(ui, &video, width, resolution_badge.as_deref());
        }

        if settings_toggled {
            self.settings_open = !self.settings_open;
            self.sync_player_window_visibility();
        }
        if theater_toggled {
            self.toggle_theater_mode();
        }
        if fullscreen_toggled {
            self.toggle_fullscreen(ctx);
        }
        if next_clicked {
            self.request_queue_advance();
        }
        if close_clicked {
            self.close_video(ctx);
        }
    }

    fn fullscreen_seek_ui(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        position: f64,
        duration: Option<f64>,
        now: Instant,
    ) {
        let mut seek = self.seek_drag.unwrap_or(position);
        let response = ui
            .scope(|ui| {
                ui.spacing_mut().slider_width = width;
                ui.add_enabled(
                    duration.is_some(),
                    egui::Slider::new(&mut seek, 0.0..=duration.unwrap_or(1.0))
                        .show_value(false)
                        .trailing_fill(true),
                )
            })
            .inner;
        if response.dragged() {
            self.seek_drag = Some(seek);
        }
        if response.drag_stopped() || (response.changed() && !response.dragged()) {
            let seek_res = self
                .player
                .as_ref()
                .map(|player| player.seek(seek, "absolute+exact"))
                .unwrap_or(Ok(()));
            match seek_res {
                Ok(()) => self.record_manual_seek(seek, duration, now),
                Err(error) => self.message = error,
            }
            self.seek_drag = None;
        }
    }

    fn player_overlay_ui(
        &mut self,
        ui: &mut egui::Ui,
        video_area: Option<(egui::Rect, egui::Response)>,
        paused: bool,
        ended: bool,
        muted: bool,
        position: f64,
        duration: Option<f64>,
        raw_volume: f64,
        has_queued: bool,
    ) -> Option<overlay::OverlayAction> {
        let mut overlay_action = None;
        if let Some((rect, response)) = video_area {
            let hovered = response.hovered() || ui.rect_contains_pointer(rect);
            if hovered {
                self.overlay_last_activity = Instant::now();
            }
            let idle_secs = self.overlay_last_activity.elapsed().as_secs_f64();
            let visible = overlay::overlay_visible(hovered, paused, ended, idle_secs);
            let style = self.overlay_style();
            let telemetry = overlay::OverlayTelemetry {
                paused,
                ended,
                muted,
                position_secs: position,
                duration_secs: duration,
                volume_percent: raw_volume as f32,
                has_queued,
            };
            let (action, over_bar) = overlay::show(ui, rect, &telemetry, &style, visible);
            if let Some(action) = action {
                self.overlay_last_activity = Instant::now();
                overlay_action = Some(action);
            } else if response.clicked() && !over_bar {
                overlay_action = Some(overlay::OverlayAction::TogglePlay);
            }
        }
        overlay_action
    }

    fn handle_player_overlay_action(
        &mut self,
        action: overlay::OverlayAction,
        paused: bool,
        ended: bool,
        muted: bool,
        position: f64,
        duration: Option<f64>,
        now: Instant,
        ctx: &egui::Context,
    ) {
        use overlay::OverlayAction;
        match action {
            OverlayAction::TogglePlay => {
                let result = if ended {
                    self.player
                        .as_ref()
                        .map(|player| {
                            player
                                .seek(0.0, "absolute+exact")
                                .and_then(|()| player.set("pause", "no"))
                        })
                        .unwrap_or(Ok(()))
                } else {
                    self.player
                        .as_ref()
                        .map(|player| player.set("pause", if paused { "no" } else { "yes" }))
                        .unwrap_or(Ok(()))
                };
                match result {
                    Ok(()) => {
                        if ended {
                            self.record_manual_seek(0.0, duration, now);
                        } else if !paused {
                            self.flush_playback_history_at(now);
                        }
                    }
                    Err(error) => self.message = error,
                }
            }
            OverlayAction::SeekBack10 => {
                let result = self
                    .player
                    .as_ref()
                    .map(|player| player.seek(-10.0, "relative"))
                    .unwrap_or(Ok(()));
                match result {
                    Ok(()) => self.record_manual_seek((position - 10.0).max(0.0), duration, now),
                    Err(error) => self.message = error,
                }
            }
            OverlayAction::SeekForward10 => {
                let result = self
                    .player
                    .as_ref()
                    .map(|player| player.seek(10.0, "relative"))
                    .unwrap_or(Ok(()));
                match result {
                    Ok(()) => {
                        let target = duration
                            .map(|duration| (position + 10.0).min(duration))
                            .unwrap_or(position + 10.0);
                        self.record_manual_seek(target, duration, now);
                    }
                    Err(error) => self.message = error,
                }
            }
            OverlayAction::SeekTo(target) => {
                let result = self
                    .player
                    .as_ref()
                    .map(|player| player.seek(target, "absolute+exact"))
                    .unwrap_or(Ok(()));
                match result {
                    Ok(()) => self.record_manual_seek(target, duration, now),
                    Err(error) => self.message = error,
                }
            }
            OverlayAction::ToggleMute => {
                let result = self
                    .player
                    .as_ref()
                    .map(|player| player.set("mute", if muted { "no" } else { "yes" }))
                    .unwrap_or(Ok(()));
                if let Err(error) = result {
                    self.message = error;
                }
            }
            OverlayAction::SetVolume(volume) => {
                let result = self
                    .player
                    .as_ref()
                    .map(|player| player.set("volume", &format!("{volume:.0}")))
                    .unwrap_or(Ok(()));
                if let Err(error) = result {
                    self.message = error;
                }
            }
            OverlayAction::Next => self.request_queue_advance(),
            OverlayAction::ToggleSettings => {
                self.settings_open = !self.settings_open;
                self.sync_player_window_visibility();
            }
            OverlayAction::ToggleTheater => self.toggle_theater_mode(),
            OverlayAction::ToggleFullscreen => self.toggle_fullscreen(ctx),
            OverlayAction::Close => self.close_video(ctx),
        }
    }

    fn watch_primary_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        column_width: f32,
        available_height: f32,
        mode: WatchLayoutMode,
    ) {
        self.sync_theme_visuals(ctx);
        self.sync_player_window_visibility();
        #[cfg(windows)]
        if self.player.is_some() {
            self.player_ui(ui, ctx, column_width, available_height, mode);
            return;
        }
        let Some(video) = self.selected.clone() else {
            return;
        };
        let player_size = compute_player_size_with_theater(
            column_width,
            available_height,
            self.fullscreen,
            self.theater_mode,
            mode,
        );
        let (rect, _) = ui.allocate_exact_size(player_size, egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, Color32::BLACK);
        let compact_controls = watch_controls_are_compact(player_size.x);
        self.paint_hidden_video_notice(ui, rect);
        ui.add_space(6.0);
        let mut theater_toggled = false;
        let mut fullscreen_toggled = false;
        let mut settings_toggled = false;
        let mut close_clicked = false;
        let mut next_clicked = false;
        let has_queued = !self.queue.is_empty();
        let fullscreen = self.fullscreen;
        let theater_mode = self.theater_mode;
        let palette = self.palette();
        egui::Frame::new()
            .fill(palette.player_bar_bg)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_max_width(player_size.x);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(has_queued, egui::Button::new("⏭ Próximo"))
                        .on_hover_text("Avançar manualmente para o próximo vídeo da fila")
                        .clicked()
                    {
                        next_clicked = true;
                    }
                    if ui
                        .button("⚙")
                        .on_hover_text("Abrir ou fechar configurações do player")
                        .clicked()
                    {
                        settings_toggled = true;
                    }
                    let theater_button_label = if compact_controls {
                        if theater_mode { "Normal" } else { "Teatro" }
                    } else if theater_mode {
                        "▭ Normal"
                    } else {
                        "▭ Teatro"
                    };
                    let theater_button = egui::Button::new(
                        RichText::new(theater_button_label).color(if theater_mode {
                            palette.active_button_fg
                        } else {
                            palette.theater_inactive_fg
                        }),
                    )
                    .fill(if theater_mode {
                        palette.active_button_bg
                    } else {
                        palette.theater_inactive_bg
                    });
                    if ui
                        .add_enabled(!fullscreen, theater_button)
                        .on_hover_text(if fullscreen {
                            "Modo teatro indisponível enquanto a tela cheia estiver ativa"
                        } else if theater_mode {
                            "Sair do modo teatro e restaurar visualização padrão (T)"
                        } else {
                            "Ativar modo teatro na janela (T)"
                        })
                        .clicked()
                    {
                        theater_toggled = true;
                    }
                    let screen_icon = if fullscreen {
                        Icon::Minimize
                    } else {
                        Icon::Maximize
                    };
                    if ui
                        .add(egui::Button::image(
                            screen_icon.image(palette.icon_fg, 17.0),
                        ))
                        .on_hover_text("Tela cheia (F ou Esc para sair)")
                        .clicked()
                    {
                        fullscreen_toggled = true;
                    }
                    if ui
                        .add(egui::Button::image(
                            Icon::Close.image(palette.icon_fg, 17.0),
                        ))
                        .on_hover_text("Fechar vídeo")
                        .clicked()
                    {
                        close_clicked = true;
                    }
                });
            });
        if !self.fullscreen && mode != WatchLayoutMode::Short {
            self.watch_metadata_card_ui(ui, &video, player_size.x, None);
        }
        if settings_toggled {
            self.settings_open = !self.settings_open;
            self.sync_player_window_visibility();
        }
        if theater_toggled {
            self.toggle_theater_mode();
        }
        if fullscreen_toggled {
            self.toggle_fullscreen(ctx);
        }
        if next_clicked {
            self.request_queue_advance();
        }
        if close_clicked {
            self.close_video(ctx);
        }
    }

    fn format_watch_metadata_stats(video: &Video, metadata: Option<&VideoMetadata>) -> Vec<String> {
        let mut stats = Vec::new();
        if let Some(views) = metadata.and_then(|meta| meta.view_count) {
            stats.push(format_view_count(views));
        }
        if let Some(relative_date) = metadata
            .and_then(|meta| meta.upload_date.as_deref())
            .and_then(|date| recommendations::format_youtube_relative_time(date, None))
        {
            stats.push(relative_date);
        }
        if !video.duration.is_empty() {
            stats.push(format!("Dura\u{00e7}\u{00e3}o {}", video.duration));
        }
        stats
    }

    fn watch_metadata_channel_row_ui(
        &self,
        ui: &mut egui::Ui,
        video: &Video,
        channel_target: Option<&ChannelTarget>,
        content_width: f32,
        stats: &[String],
        technical_resolution: Option<&str>,
        palette: &ThemePalette,
    ) -> Option<ChannelTarget> {
        let channel_label = channel_target
            .map(channel_target_display_label)
            .filter(|label| !label.trim().is_empty())
            .or_else(|| (!video.channel.trim().is_empty()).then_some(video.channel.as_str()))
            .unwrap_or("Canal");
        let avatar = paint_channel_avatar(ui, channel_label, 40.0);
        let mut open_channel_target = None;
        if avatar.clicked() {
            open_channel_target = channel_target.cloned();
        }

        let channel_width = (content_width - 110.0).max(120.0);
        ui.vertical(|ui| {
            ui.set_max_width(channel_width);
            let clean_channel = truncate_text_chars(channel_label, 48);
            let channel_button = egui::Button::new(
                RichText::new(clean_channel)
                    .size(14.0)
                    .strong()
                    .color(palette.channel_fg),
            )
            .frame(false);
            if ui
                .add(channel_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if let Some(target) = channel_target {
                    format!("Abrir canal ({})", target.canonical_url())
                } else {
                    "Abrir detalhes do canal".to_owned()
                })
                .clicked()
            {
                open_channel_target = channel_target.cloned();
            }
            if !stats.is_empty() {
                let stats_line = stats.join("  \u{00b7}  ");
                let response = ui.label(
                    RichText::new(stats_line)
                        .size(12.0)
                        .color(palette.neutral_status_fg),
                );
                if let Some(technical) = technical_resolution {
                    response.on_hover_text(technical);
                }
            }
        });
        open_channel_target
    }

    fn watch_metadata_actions_ui(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        video_id: Option<&str>,
        current_rating: Option<oauth::VideoRating>,
        rating_pending: bool,
        saved_for_later: bool,
        palette: &ThemePalette,
        rating_to_apply: &mut Option<Option<oauth::VideoRating>>,
        enqueue_video: &mut bool,
        toggle_watch_later: &mut bool,
    ) {
        ui.horizontal(|ui| {
            let like_active = current_rating == Some(oauth::VideoRating::Like);
            let like_button =
                egui::Button::new(RichText::new("👍 Gostei").size(12.5).strong().color(
                    if like_active {
                        palette.active_button_fg
                    } else {
                        palette.header_button_fg
                    },
                ))
                .fill(if like_active {
                    palette.active_button_bg
                } else {
                    palette.header_button_bg
                })
                .corner_radius(18);
            let like_clicked = ui
                .add_enabled_ui(!rating_pending && video_id.is_some(), |ui| {
                    ui.add_sized([92.0, 36.0], like_button)
                })
                .inner
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Marcar Gostei no YouTube")
                .clicked();
            if like_clicked {
                *rating_to_apply = Some(if like_active {
                    None
                } else {
                    Some(oauth::VideoRating::Like)
                });
            }

            let dislike_active = current_rating == Some(oauth::VideoRating::Dislike);
            let dislike_button =
                egui::Button::new(RichText::new("👎").size(15.0).color(if dislike_active {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                }))
                .fill(if dislike_active {
                    palette.active_button_bg
                } else {
                    palette.header_button_bg
                })
                .corner_radius(18);
            let dislike_clicked = ui
                .add_enabled_ui(!rating_pending && video_id.is_some(), |ui| {
                    ui.add_sized([42.0, 36.0], dislike_button)
                })
                .inner
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Marcar Não gostei no YouTube")
                .clicked();
            if dislike_clicked {
                *rating_to_apply = Some(if dislike_active {
                    None
                } else {
                    Some(oauth::VideoRating::Dislike)
                });
            }

            let queue_button = egui::Button::new(
                RichText::new("Fila")
                    .size(13.0)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            if ui
                .add_sized([64.0, 36.0], queue_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Adicionar este vídeo à fila")
                .clicked()
            {
                *enqueue_video = true;
            }

            let share_button = egui::Button::new(
                RichText::new("Compartilhar")
                    .size(13.0)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            if ui
                .add_sized([112.0, 36.0], share_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Copiar o endereço deste vídeo")
                .clicked()
            {
                ctx.copy_text(video.url.clone());
            }

            let save_label = if saved_for_later {
                "✓ Salvo"
            } else {
                "Salvar"
            };
            let save_button = egui::Button::new(
                RichText::new(save_label)
                    .size(13.0)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            if ui
                .add_sized([80.0, 36.0], save_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Alternar este vídeo em Assistir depois")
                .clicked()
            {
                *toggle_watch_later = true;
            }
        });
    }

    fn watch_metadata_description_ui(
        &self,
        ui: &mut egui::Ui,
        video_url: &str,
        description: Option<&str>,
        content_width: f32,
        palette: &ThemePalette,
    ) {
        if let Some(description) = clean_description(description) {
            ui.add_space(8.0);
            egui::CollapsingHeader::new(
                RichText::new("Descri\u{00e7}\u{00e3}o").size(14.0).strong(),
            )
            .id_salt(("watch_description", video_url))
            .default_open(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_max_width((content_width - 24.0).max(1.0));
                        ui.label(
                            RichText::new(description)
                                .size(13.0)
                                .color(palette.description_fg),
                        );
                    });
            });
        }
    }

    fn watch_metadata_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        video: &Video,
        width: f32,
        resolution_badge: Option<&str>,
    ) {
        let ctx = ui.ctx().clone();
        let metadata = self.metadata_for(&video.url).cloned();
        let channel_target = self.channel_target_for_video(video);
        let palette = self.palette();
        let content_width = width.min(ui.available_width()).max(1.0);
        let saved_for_later = self
            .library
            .watch_later
            .iter()
            .any(|record| record.video.url == video.url);
        let video_id = history::youtube_video_id(&video.url);
        let current_rating = video_id
            .as_ref()
            .and_then(|id| self.video_ratings.get(id).copied());
        let rating_pending = self.rating_request.is_some();
        let mut rating_to_apply = None;
        let mut toggle_watch_later = false;
        let mut enqueue_video = false;
        let mut open_channel_target = None;
        let stats = Self::format_watch_metadata_stats(video, metadata.as_ref());
        let technical_resolution = resolution_badge.map(str::to_owned);

        ui.add_space(12.0);
        ui.scope(|ui| {
            ui.set_min_width(content_width);
            ui.set_max_width(content_width);
            ui.label(
                RichText::new(&video.title)
                    .size(if content_width < 620.0 { 18.0 } else { 20.0 })
                    .strong()
                    .color(palette.channel_fg),
            );

            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);
                if let Some(target) = self.watch_metadata_channel_row_ui(
                    ui,
                    video,
                    channel_target.as_ref(),
                    content_width,
                    &stats,
                    technical_resolution.as_deref(),
                    &palette,
                ) {
                    open_channel_target = Some(target);
                }

                self.watch_metadata_actions_ui(
                    ui,
                    &ctx,
                    video,
                    video_id.as_deref(),
                    current_rating,
                    rating_pending,
                    saved_for_later,
                    &palette,
                    &mut rating_to_apply,
                    &mut enqueue_video,
                    &mut toggle_watch_later,
                );
            });

            self.watch_metadata_description_ui(
                ui,
                &video.url,
                metadata
                    .as_ref()
                    .and_then(|meta| meta.description.as_deref()),
                content_width,
                &palette,
            );
            ui.add_space(6.0);
        });

        if let (Some(video_id), Some(rating)) = (video_id, rating_to_apply) {
            self.start_video_rating(video_id, rating, &ctx);
        }
        if toggle_watch_later {
            self.library.toggle_watch_later(video.clone());
            self.save_library();
        }
        if enqueue_video {
            let _ = self.enqueue_video(video.clone());
        }
        if let Some(target) = open_channel_target {
            self.open_channel(target, &ctx);
        }
    }

    fn video_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        compact: bool,
        is_playing: bool,
    ) -> bool {
        let mut clicked = false;
        let mut enqueue_clicked = false;
        let mut favorite_clicked = false;
        let mut later_clicked = false;
        let mut open_channel_target = None;
        let is_playlist = self.home_playlist_urls.contains(&video.url);
        let palette = self.palette();
        let metadata = self.metadata_for(&video.url).cloned();
        let channel_target = metadata
            .as_ref()
            .and_then(search::channel_target_for_metadata);
        let meta_line = format_card_metadata_line(&video.duration, metadata.as_ref());
        let history_entry =
            history::youtube_video_id(&video.url).and_then(|id| self.history.get(&id).cloned());
        let snippet = if compact {
            None
        } else {
            description_snippet(
                metadata
                    .as_ref()
                    .and_then(|meta| meta.description.as_deref()),
                160,
            )
        };
        let fill = if is_playing {
            palette.card_playing_bg
        } else {
            palette.card_bg
        };
        let stroke = if is_playing {
            egui::Stroke::new(1.0, palette.card_playing_stroke)
        } else {
            egui::Stroke::new(1.0, palette.card_stroke)
        };
        let margin = if compact {
            egui::Margin::symmetric(8, 6)
        } else {
            egui::Margin::symmetric(10, 8)
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(6)
            .inner_margin(margin)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    if self.thumbnails.show(ui, ctx, &video.thumbnail).clicked() {
                        clicked = true;
                    }
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width().max(60.0));
                        ui.horizontal(|ui| {
                            if is_playing {
                                ui.label(
                                    RichText::new("▶ Em reprodução")
                                        .small()
                                        .strong()
                                        .color(palette.playing_badge_fg),
                                );
                            }
                            if is_playlist {
                                ui.label(RichText::new("Playlist").small().strong());
                            }
                            if !is_playlist {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_sized(
                                                [58.0, 22.0],
                                                egui::Button::new(RichText::new("+ Fila").small()),
                                            )
                                            .on_hover_text(
                                                "Adicionar vídeo ao final da fila de reprodução",
                                            )
                                            .clicked()
                                        {
                                            enqueue_clicked = true;
                                        }
                                        if ui
                                            .small_button(
                                                if self
                                                    .library
                                                    .favorites
                                                    .iter()
                                                    .any(|v| v.video.url == video.url)
                                                {
                                                    "♥"
                                                } else {
                                                    "♡"
                                                },
                                            )
                                            .on_hover_text("Alternar favorito local")
                                            .clicked()
                                        {
                                            favorite_clicked = true;
                                        }
                                        if ui
                                            .small_button(
                                                if self
                                                    .library
                                                    .watch_later
                                                    .iter()
                                                    .any(|v| v.video.url == video.url)
                                                {
                                                    "✓ Depois"
                                                } else {
                                                    "+ Depois"
                                                },
                                            )
                                            .on_hover_text("Salvar em Assistir depois (local)")
                                            .clicked()
                                        {
                                            later_clicked = true;
                                        }
                                    },
                                );
                            }
                        });
                        let title_size = if compact { 13.5 } else { 15.0 };
                        let title_button = egui::Button::new(
                            RichText::new(&video.title).size(title_size).strong(),
                        )
                        .frame(false)
                        .wrap();
                        if ui
                            .add(title_button)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            clicked = true;
                        }
                        if let Some(target) = &channel_target {
                            let channel_label = if video.channel.trim().is_empty() {
                                channel_target_display_label(target)
                            } else {
                                video.channel.as_str()
                            };
                            let button = egui::Button::new(
                                RichText::new(format!("📺 {channel_label}"))
                                    .small()
                                    .strong()
                                    .color(palette.channel_fg),
                            )
                            .fill(palette.header_button_bg);
                            if ui
                                .add(button)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(format!("Abrir canal ({})", target.canonical_url()))
                                .clicked()
                            {
                                open_channel_target = Some(target.clone());
                            }
                        } else if !video.channel.is_empty() {
                            ui.label(RichText::new(&video.channel).small().weak());
                        }
                        if let Some(line) = &meta_line {
                            ui.label(RichText::new(line).small().weak());
                        }
                        if let Some(entry) = &history_entry {
                            let status = format_history_status(entry);
                            let color = if entry.resume_position_ms().is_some() {
                                palette.resume_status_fg
                            } else {
                                palette.watched_status_fg
                            };
                            ui.label(RichText::new(status).small().color(color));
                        }
                        if let Some(text) = &snippet {
                            ui.add_space(2.0);
                            ui.label(RichText::new(text).small().color(palette.snippet_fg));
                        }
                    });
                });
            });
        if favorite_clicked {
            self.library.toggle_favorite(video.clone());
            self.save_library();
        }
        if later_clicked {
            self.library.toggle_watch_later(video.clone());
            self.save_library();
        }
        if enqueue_clicked {
            let _ = self.enqueue_video(video.clone());
        }
        if let Some(target) = open_channel_target {
            self.open_channel(target, ctx);
        }
        clicked
    }

    fn save_library(&mut self) {
        let result = self
            .library_path
            .as_deref()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "local library path unavailable")
            })
            .and_then(|path| library::save(path, &self.library));
        self.library_error = result
            .err()
            .map(|error| format!("Não foi possível salvar a biblioteca: {error}"));
    }

    fn user_data_paths(&self) -> io::Result<Vec<(&'static str, PathBuf)>> {
        let required = |path: &Option<PathBuf>, name| {
            path.clone().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("caminho de dados indisponível: {name}"),
                )
            })
        };
        Ok(vec![
            ("library", required(&self.library_path, "biblioteca")?),
            ("queue", required(&self.queue_path, "fila")?),
            ("history", required(&self.history_path, "histórico")?),
            (
                "playback",
                required(&self.playback_settings_path, "preferências de reprodução")?,
            ),
            ("theme", required(&self.theme_settings_path, "tema")?),
            ("feed", required(&self.feed_prefs_path, "filtros do feed")?),
            ("buffer", required(&self.settings_path, "buffer")?),
        ])
    }

    fn export_user_data(&mut self) {
        let path_text = self.backup_path_text.trim();
        if path_text.is_empty() {
            self.backup_status = Some("Informe o caminho do arquivo de backup.".into());
            return;
        }
        let path = PathBuf::from(path_text);
        let history_json = serde_json::json!({
            "version": 1,
            "entries": self.history.entries().iter().map(|entry| serde_json::json!({
                "video_id": entry.video_id,
                "title": entry.title,
                "channel": entry.channel,
                "position_ms": entry.position_ms,
                "duration_ms": entry.duration_ms,
            })).collect::<Vec<_>>()
        });
        let backup = backup::Backup {
            library: serde_json::to_value(&self.library).unwrap_or_default(),
            queue: self.queue.to_backup_value(),
            history: history_json,
            preferences: serde_json::json!({
                "speed_quarters": self.playback_preferences.speed_quarters,
                "max_height": self.playback_preferences.max_height,
                "theme": if self.theme == Theme::Dark { "dark" } else { "light" },
                "buffer_max_mib": self.buffer_profile.values().max_mib,
                "buffer_back_mib": self.buffer_profile.values().back_mib,
                "buffer_cache_seconds": self.buffer_profile.values().cache_seconds,
                "feed_hide_watched": self.feed_hide_watched,
                "feed_hide_shorts": self.feed_hide_shorts,
                "feed_hide_lives": self.feed_hide_lives,
                "feed_newest_first": self.feed_newest_first,
                "feed_channel": self.feed_channel,
            }),
        };
        self.backup_status = Some(match backup::save(&path, &backup) {
            Ok(()) => "Backup exportado com fila versionada e preferências atuais.".into(),
            Err(error) => format!("Falha ao exportar backup: {error}"),
        });
    }

    fn read_imported_user_data(&self) -> io::Result<ImportedUserData> {
        let path_text = self.backup_path_text.trim();
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
        Ok(ImportedUserData {
            library,
            queue,
            history,
            playback_preferences,
            theme,
            buffer_profile,
            feed,
        })
    }

    fn analyze_import(&mut self) {
        let imported = match self.read_imported_user_data() {
            Ok(imported) => imported,
            Err(error) => {
                self.pending_import = None;
                self.backup_preview = None;
                self.backup_confirm_import = false;
                self.backup_status = Some(format!(
                    "Importação cancelada; dados atuais preservados: {error}"
                ));
                return;
            }
        };
        let (_, library_merge) = self.library.merged(&imported.library);
        let (_, history_merge) = self.history.merged(&imported.history);
        let queue_space = DEFAULT_QUEUE_LIMIT.saturating_sub(self.queue.len());
        let queue_added = imported.queue.len().min(queue_space);
        let queue_skipped = imported.queue.len().saturating_sub(queue_added);
        self.backup_preview = Some(BackupPreview {
            path: PathBuf::from(self.backup_path_text.trim()),
            library_items: imported.library.item_count(),
            favorites: imported.library.favorites.len(),
            watch_later: imported.library.watch_later.len(),
            collections: imported.library.collections.len(),
            bookmarks: imported.library.bookmarks.len(),
            queue_items: imported.queue.len(),
            history_entries: imported.history.entries().len(),
            merge_added: library_merge.added + history_merge.added + queue_added,
            merge_conflicts: library_merge.conflicts + history_merge.conflicts,
            merge_skipped: library_merge.skipped_for_limit + queue_skipped,
            history_progress_updates: history_merge.progress_updated,
        });
        self.pending_import = Some(imported);
        self.backup_import_mode = BackupImportMode::Merge;
        self.backup_confirm_import = true;
        self.backup_status =
            Some("Backup validado. Confira a prévia e escolha como aplicar os dados.".into());
    }

    fn merged_imported_user_data(&self, imported: ImportedUserData) -> ImportedUserData {
        let (library, _) = self.library.merged(&imported.library);
        let (history, _) = self.history.merged(&imported.history);
        let mut queue = Queue::new(DEFAULT_QUEUE_LIMIT);
        for video in self
            .queue
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
            ..imported
        }
    }

    fn import_user_data(&mut self) {
        let Some(imported) = self.pending_import.clone() else {
            self.analyze_import();
            return;
        };
        let target = match self.backup_import_mode {
            BackupImportMode::Replace => imported,
            BackupImportMode::Merge => self.merged_imported_user_data(imported),
        };
        let paths = match self.user_data_paths() {
            Ok(paths) => paths,
            Err(error) => {
                self.backup_status = Some(format!(
                    "Importação cancelada; dados atuais preservados: {error}"
                ));
                return;
            }
        };
        let Some(root) = paths.first().and_then(|(_, path)| path.parent()) else {
            self.backup_status =
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
                self.backup_status = Some(format!(
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
            settings::save_playback(path("playback")?, target.playback_preferences)?;
            settings::save_theme(path("theme")?, target.theme)?;
            feed_prefs::save(path("feed")?, &target.feed)?;
            settings::save(path("buffer")?, target.buffer_profile)?;
            Ok(())
        })();
        if let Err(error) = persist {
            let rollback = restore_point.restore();
            self.backup_status = Some(match rollback {
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
            self.backup_status = Some(format!(
                "Os arquivos foram gravados, mas não foi possível remover o ponto de restauração: {error}. A importação será desfeita na próxima abertura."
            ));
            return;
        }
        self.library = target.library;
        self.queue = target.queue;
        self.history = target.history;
        self.playback_preferences = target.playback_preferences;
        self.theme = target.theme;
        self.applied_theme = None;
        self.buffer_profile = target.buffer_profile;
        self.feed_hide_watched = target.feed.hide_watched;
        self.feed_hide_shorts = target.feed.hide_shorts;
        self.feed_hide_lives = target.feed.hide_lives;
        self.feed_newest_first = target.feed.newest_first;
        self.feed_channel = target.feed.channel;
        self.cancel_pending_queue_video();
        self.invalidate_feed_cache();
        self.pending_import = None;
        self.backup_preview = None;
        self.backup_confirm_import = false;
        self.backup_status = Some(match self.backup_import_mode {
            BackupImportMode::Replace => {
                "Backup aplicado por substituição; os dados anteriores ficaram no ponto de restauração até a conclusão.".into()
            }
            BackupImportMode::Merge => {
                "Backup mesclado: itens ausentes foram adicionados, a biblioteca atual prevaleceu em conflitos e o histórico manteve o maior progresso.".into()
            }
        });
    }

    fn continue_watching_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        let entries: Vec<_> = self
            .history
            .entries()
            .iter()
            .filter(|entry| entry.resume_position_ms().is_some())
            .take(4)
            .cloned()
            .collect();
        if entries.is_empty() {
            return None;
        }
        let palette = self.palette();
        let mut chosen = None;
        let mut completed = None;
        ui.label(RichText::new("Continuar assistindo").size(20.0).strong());
        ui.add_space(10.0);
        let available = ui.available_width().max(220.0);
        let columns = compute_continue_watching_columns(available);
        let gap = 16.0;
        let card_width = ((available - gap * (columns.saturating_sub(1) as f32)) / columns as f32)
            .clamp(180.0, 360.0);
        self.thumbnails.begin_pass();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, 12.0);
            for entry in &entries {
                let Some(video) = self.video_for_history_entry(entry) else {
                    continue;
                };
                ui.push_id(("continue", &entry.video_id), |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(card_width);
                        ui.set_max_width(card_width);
                        let thumb_height = (card_width * 9.0 / 16.0).round();
                        let thumb = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            &video.thumbnail,
                            egui::vec2(card_width, thumb_height),
                            12.0,
                        );
                        let progress = entry.duration_ms.map(|duration| {
                            (entry.position_ms as f32 / duration.max(1) as f32).clamp(0.04, 1.0)
                        });
                        paint_thumbnail_overlays(ui, thumb.rect, &video.duration, progress, false);
                        if thumb.clicked() {
                            chosen = Some(video.clone());
                        }
                        ui.add_space(8.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(truncate_text_chars(&entry.title, 72))
                                        .size(14.5)
                                        .strong()
                                        .color(palette.header_button_fg),
                                )
                                .frame(false)
                                .wrap(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            chosen = Some(video.clone());
                        }
                        if !entry.channel.is_empty() {
                            ui.label(
                                RichText::new(truncate_text_chars(&entry.channel, 38))
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format_history_status(entry))
                                    .size(12.0)
                                    .color(palette.neutral_status_fg),
                            );
                            if ui
                                .small_button("Marcar conclu\u{00ed}do")
                                .on_hover_text("Marcar este v\u{00ed}deo como assistido")
                                .clicked()
                            {
                                completed = Some(entry.clone());
                            }
                        });
                    });
                });
            }
        });
        self.thumbnails.end_pass(ctx);
        if let Some(mut entry) = completed {
            entry.position_ms = entry.duration_ms.unwrap_or(entry.position_ms);
            let _ = self.history.record(entry);
            self.history_dirty = true;
            self.save_history_to_disk(Instant::now());
        }
        chosen
    }

    fn channel_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
    ) -> Option<Video> {
        let palette = self.palette();
        let loading = self.is_channel_loading();
        let loading_target = self.channel_loading_target.clone();
        let error = self.channel_error.clone();
        let page = self.channel_page.clone();
        let retry_target = self.current_channel_target();
        let from_cache = self.channel_from_cache;
        let current_limit = self.channel_video_limit;
        let mut back_to_results = false;
        let mut reload_target = None;
        let mut force_refresh = false;
        let mut new_limit: Option<usize> = None;

        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("← Voltar aos resultados").small())
                .on_hover_text("Fechar página do canal e retornar aos resultados da pesquisa")
                .clicked()
            {
                back_to_results = true;
            }
            if let Some(target) = &retry_target {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !loading,
                            egui::Button::new(RichText::new("↻ Atualizar").small()),
                        )
                        .on_hover_text("Recarregar do YouTube ignorando o cache local")
                        .clicked()
                    {
                        reload_target = Some(target.clone());
                        force_refresh = true;
                    }
                    let mut limit = current_limit;
                    egui::ComboBox::from_id_salt("channel_video_limit")
                        .selected_text(format!("{limit} vídeos"))
                        .show_ui(ui, |ui| {
                            for option in [20usize, 30, 50, search::MAX_CHANNEL_VIDEOS_LIMIT] {
                                if ui
                                    .selectable_value(
                                        &mut limit,
                                        option,
                                        format!("{option} vídeos"),
                                    )
                                    .clicked()
                                {
                                    new_limit = Some(option);
                                }
                            }
                        });
                    ui.label(RichText::new("Limite").small().weak());
                });
            }
        });
        ui.add_space(6.0);

        let (cancel_initial, status_reload_target) = self.channel_load_status_ui(
            ui,
            loading,
            loading_target.as_ref(),
            error.as_deref(),
            retry_target.as_ref(),
            palette,
        );
        if status_reload_target.is_some() {
            reload_target = status_reload_target;
        }

        let mut chosen = None;
        if let Some(page) = &page {
            let card_max_w = max_card_width
                .map(|limit| ui.available_width().min(limit))
                .unwrap_or_else(|| ui.available_width());
            egui::Frame::new()
                .fill(palette.metadata_card_bg)
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.set_max_width(card_max_w);
                    if let Some(banner_url) = page
                        .banner_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|url| !url.is_empty())
                    {
                        let banner_height = if compact { 96.0 } else { 150.0 };
                        self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            banner_url,
                            egui::vec2(card_max_w, banner_height),
                            12.0,
                        );
                        ui.add_space(12.0);
                    }
                    ui.horizontal_top(|ui| {
                        let title = channel_page_display_title(page);
                        if let Some(avatar_url) = page
                            .avatar_url
                            .as_deref()
                            .map(str::trim)
                            .filter(|url| !url.is_empty())
                        {
                            let avatar_size = if compact { 56.0 } else { 78.0 };
                            self.thumbnails.show_rounded(
                                ui,
                                ctx,
                                avatar_url,
                                egui::vec2(avatar_size, avatar_size),
                                avatar_size * 0.5,
                            );
                        } else {
                            let _ =
                                paint_channel_avatar(ui, title, if compact { 56.0 } else { 78.0 });
                        }
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(title)
                                        .size(if compact { 21.0 } else { 28.0 })
                                        .strong(),
                                );
                                if page.verified == Some(true) {
                                    ui.label(
                                        RichText::new("\u{2713}")
                                            .size(15.0)
                                            .color(palette.neutral_status_fg),
                                    )
                                    .on_hover_text("Canal verificado");
                                }
                            });
                            let mut channel_meta = Vec::new();
                            if let Some(handle) = page
                                .handle
                                .as_deref()
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                            {
                                channel_meta.push(handle.to_owned());
                            }
                            if let Some(subs) = page.subscriber_count {
                                channel_meta.push(format_subscriber_count(subs));
                            }
                            channel_meta.push(format!("{} vídeos", page.videos.len()));
                            ui.label(
                                RichText::new(channel_meta.join("  ·  "))
                                    .size(13.0)
                                    .color(palette.neutral_status_fg),
                            );
                            if let Some(description) =
                                clean_description(page.description.as_deref())
                            {
                                ui.add_space(6.0);
                                let desc_text = if compact {
                                    description_snippet(Some(&description), 180)
                                        .unwrap_or(description)
                                } else {
                                    description_snippet(Some(&description), 320)
                                        .unwrap_or(description)
                                };
                                ui.label(
                                    RichText::new(desc_text)
                                        .size(13.0)
                                        .color(palette.neutral_status_fg),
                                );
                            }
                        });
                    });
                });
            ui.add_space(8.0);

            let mut switch_to_tab = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                if youtube_tab_button(
                    ui,
                    "Vídeos",
                    self.channel_tab == ChannelTab::Videos,
                    &palette,
                )
                .clicked()
                {
                    switch_to_tab = Some(ChannelTab::Videos);
                }
                let playlists_label = if self.channel_playlists_loaded {
                    format!("Playlists ({})", self.channel_playlists.len())
                } else {
                    "Playlists".into()
                };
                if youtube_tab_button(
                    ui,
                    &playlists_label,
                    self.channel_tab == ChannelTab::Playlists,
                    &palette,
                )
                .clicked()
                {
                    switch_to_tab = Some(ChannelTab::Playlists);
                }
                if youtube_tab_button(
                    ui,
                    "Sobre",
                    self.channel_tab == ChannelTab::About,
                    &palette,
                )
                .clicked()
                {
                    switch_to_tab = Some(ChannelTab::About);
                }
                if from_cache && self.channel_tab == ChannelTab::Videos {
                    ui.label(
                        RichText::new("• do cache local")
                            .small()
                            .color(palette.info_accent_fg),
                    )
                    .on_hover_text(
                        "Página recuperada do cache em memória (válido por 10 minutos). Use ↻ Atualizar para buscar novamente.",
                    );
                }
            });
            if let Some(tab) = switch_to_tab {
                self.channel_tab = tab;
                match tab {
                    ChannelTab::Videos => {}
                    ChannelTab::Playlists => {
                        if !self.channel_playlists_loaded
                            && !self.is_channel_playlists_loading()
                            && self.channel_playlists_error.is_none()
                        {
                            self.start_channel_playlists(ctx);
                        }
                    }
                    ChannelTab::About => {
                        if !self.channel_about_loaded
                            && !self.is_channel_about_loading()
                            && self.channel_about_error.is_none()
                        {
                            self.start_channel_about(ctx);
                        }
                    }
                }
            }
            ui.separator();
            ui.add_space(10.0);

            match self.channel_tab {
                ChannelTab::Videos => {
                    chosen = self.channel_videos_tab_ui(
                        ui,
                        ctx,
                        page,
                        scroll_id,
                        compact,
                        max_card_width,
                        card_max_w,
                    );
                }
                ChannelTab::Playlists => {
                    chosen = self.channel_playlists_tab_ui(
                        ui,
                        ctx,
                        scroll_id,
                        compact,
                        max_card_width,
                        card_max_w,
                    );
                }
                ChannelTab::About => {
                    self.channel_about_tab_ui(ui, ctx, page, scroll_id, compact, card_max_w);
                }
            }
        }

        if cancel_initial {
            self.cancel_channel_loading();
        }
        if back_to_results {
            self.close_channel_view();
        }
        if let Some(limit) = new_limit {
            self.channel_video_limit = limit;
        }
        if new_limit.is_some() || reload_target.is_some() {
            if let Some(target) = reload_target.or_else(|| self.current_channel_target()) {
                let force = force_refresh && new_limit.is_none();
                self.open_channel_with_options(target, ctx, force);
            }
        }
        chosen
    }

    #[allow(clippy::too_many_arguments)]
    fn channel_videos_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        card_max_w: f32,
    ) -> Option<Video> {
        let palette = self.palette();
        let mut chosen = None;
        let mut load_more_clicked = false;
        let mut cancel_more_clicked = false;
        let more_loading = self.is_channel_more_videos_loading();
        let more_error = self.channel_more_videos_error.clone();
        let has_more = self.channel_videos_has_more;
        let reached_max = self.channel_videos_reached_max
            || page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT;

        if page.videos.is_empty() {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.label(RichText::new("Nenhum vídeo público listado para este canal.").weak());
                });
            return None;
        }

        let current_url = self.selected.as_ref().map(|video| video.url.clone());
        let channel_videos: Vec<Video> =
            page.videos.iter().map(|item| item.video.clone()).collect();
        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let target_width = max_card_width
                    .map(|limit| ui.available_width().min(limit))
                    .unwrap_or_else(|| ui.available_width());
                for (index, video) in channel_videos.iter().enumerate() {
                    let is_playing = current_url.as_deref() == Some(video.url.as_str());
                    ui.push_id(index, |ui| {
                        ui.scope(|ui| {
                            ui.set_max_width(target_width);
                            if self.video_card_ui(ui, ctx, video, compact, is_playing) {
                                chosen = Some(video.clone());
                            }
                        });
                    });
                    ui.add_space(6.0);
                }

                if let Some(err) = &more_error {
                    egui::Frame::new()
                        .fill(palette.error_bg)
                        .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_max_width(target_width);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!("Erro ao carregar mais vídeos: {err}"))
                                        .small()
                                        .color(palette.error_banner_fg),
                                );
                                if ui
                                    .button(RichText::new("↻ Tentar novamente").small())
                                    .clicked()
                                {
                                    load_more_clicked = true;
                                }
                            });
                        });
                    ui.add_space(6.0);
                }

                if more_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Carregando mais vídeos do canal...")
                                .small()
                                .color(palette.info_accent_fg),
                        );
                        if ui
                            .button(RichText::new("Cancelar").small())
                            .on_hover_text("Cancelar carregamento adicional de vídeos")
                            .clicked()
                        {
                            cancel_more_clicked = true;
                        }
                    });
                } else if reached_max {
                    ui.label(
                        RichText::new(format!(
                            "Limite máximo de {} vídeos por canal atingido.",
                            search::MAX_CHANNEL_VIDEOS_LIMIT
                        ))
                        .small()
                        .weak(),
                    );
                } else if has_more {
                    let remaining =
                        search::MAX_CHANNEL_VIDEOS_LIMIT.saturating_sub(channel_videos.len());
                    let step = remaining.min(search::CHANNEL_VIDEOS_PAGE_STEP);
                    if ui
                        .button(RichText::new(format!(
                            "Carregar mais vídeos (+{step}, até {})",
                            search::MAX_CHANNEL_VIDEOS_LIMIT
                        )))
                        .on_hover_text(
                            "Buscar próximo lote de vídeos públicos do canal sob demanda",
                        )
                        .clicked()
                    {
                        load_more_clicked = true;
                    }
                }
            });
        self.thumbnails.end_pass(ctx);

        if cancel_more_clicked {
            self.cancel_channel_more_videos();
        } else if load_more_clicked {
            self.load_more_channel_videos(ctx);
        }

        chosen
    }

    fn channel_playlists_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        card_max_w: f32,
    ) -> Option<Video> {
        let palette = self.palette();

        if let Some(selected_playlist) = self.channel_selected_playlist.clone() {
            return self.channel_playlist_detail_ui(
                ui,
                ctx,
                scroll_id,
                compact,
                max_card_width,
                card_max_w,
                selected_playlist,
                palette,
            );
        }

        self.channel_playlists_list_ui(
            ui,
            ctx,
            scroll_id,
            compact,
            max_card_width,
            card_max_w,
            palette,
        )
    }

    fn channel_playlist_detail_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        card_max_w: f32,
        selected_playlist: search::ChannelPlaylist,
        palette: ThemePalette,
    ) -> Option<Video> {
        let mut chosen = None;
        let mut back_to_playlists = false;
        let mut reload_playlist = false;
        let mut cancel_playlist_videos = false;
        let loading_videos = self.is_channel_playlist_videos_loading();
        let videos_error = self.channel_playlist_videos_error.clone();
        let videos_loaded = self.channel_playlist_videos_loaded;

        ui.horizontal_wrapped(|ui| {
            if ui
                .button(RichText::new("← Voltar às playlists do canal").small())
                .clicked()
            {
                back_to_playlists = true;
            }
            ui.label(RichText::new(&selected_playlist.title).strong());
            if let Some(count) = selected_playlist.item_count {
                ui.label(
                    RichText::new(format!("({count} vídeos na playlist)"))
                        .small()
                        .weak(),
                );
            }
            if ui
                .add_enabled(
                    !loading_videos,
                    egui::Button::new(RichText::new("↻ Atualizar playlist").small()),
                )
                .clicked()
            {
                reload_playlist = true;
            }
        });
        ui.add_space(6.0);

        if loading_videos {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Carregando vídeos da playlist pública...")
                                .color(palette.info_accent_fg),
                        );
                        if ui.button(RichText::new("Cancelar").small()).clicked() {
                            cancel_playlist_videos = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if let Some(err) = &videos_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar vídeos da playlist: {err}"))
                                .color(palette.error_banner_fg),
                        );
                        if ui
                            .button(RichText::new("↻ Tentar novamente").small())
                            .clicked()
                        {
                            reload_playlist = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if videos_loaded && self.channel_playlist_videos.is_empty() && videos_error.is_none() {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.label(
                        RichText::new("Nenhum vídeo público disponível nesta playlist.").weak(),
                    );
                });
        } else if !self.channel_playlist_videos.is_empty() {
            let current_url = self.selected.as_ref().map(|video| video.url.clone());
            let playlist_videos: Vec<Video> = self
                .channel_playlist_videos
                .iter()
                .map(|item| item.video.clone())
                .collect();
            self.thumbnails.begin_pass();
            egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_playlist_videos"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let target_width = max_card_width
                        .map(|limit| ui.available_width().min(limit))
                        .unwrap_or_else(|| ui.available_width());
                    for (index, video) in playlist_videos.iter().enumerate() {
                        let is_playing = current_url.as_deref() == Some(video.url.as_str());
                        ui.push_id(("chan_pl_vid", index), |ui| {
                            ui.scope(|ui| {
                                ui.set_max_width(target_width);
                                if self.video_card_ui(ui, ctx, video, compact, is_playing) {
                                    chosen = Some(video.clone());
                                }
                            });
                        });
                        ui.add_space(6.0);
                    }
                    if playlist_videos.len() >= search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT {
                        ui.label(
                            RichText::new(format!(
                                "Exibindo até o limite de {} vídeos por playlist pública.",
                                search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT
                            ))
                            .small()
                            .weak(),
                        );
                    }
                });
            self.thumbnails.end_pass(ctx);
        }

        if back_to_playlists {
            self.close_channel_public_playlist(ctx);
        } else if cancel_playlist_videos {
            self.cancel_channel_playlist_videos();
        } else if reload_playlist {
            self.open_channel_public_playlist(selected_playlist, ctx);
        }
        return chosen;
    }

    fn channel_playlists_list_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> Option<Video> {
        let mut reload_playlists = false;
        let mut cancel_playlists = false;
        let mut open_playlist = None;
        let loading_playlists = self.is_channel_playlists_loading();
        let playlists_error = self.channel_playlists_error.clone();
        let playlists_loaded = self.channel_playlists_loaded;
        let playlists = self.channel_playlists.clone();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Playlists públicas do canal (limite de até {})",
                    search::DEFAULT_CHANNEL_PLAYLISTS_LIMIT
                ))
                .small()
                .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !loading_playlists,
                        egui::Button::new(RichText::new("↻ Atualizar playlists").small()),
                    )
                    .clicked()
                {
                    reload_playlists = true;
                }
            });
        });
        ui.add_space(6.0);

        if loading_playlists {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Consultando playlists públicas do canal...")
                                .color(palette.info_accent_fg),
                        );
                        if ui.button(RichText::new("Cancelar").small()).clicked() {
                            cancel_playlists = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if let Some(err) = &playlists_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar playlists públicas: {err}"))
                                .color(palette.error_banner_fg),
                        );
                        if ui
                            .button(RichText::new("↻ Tentar novamente").small())
                            .clicked()
                        {
                            reload_playlists = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if playlists_loaded && playlists.is_empty() && playlists_error.is_none() {
            Self::channel_playlists_empty_state_ui(ui, card_max_w, palette);
        } else if !playlists.is_empty() {
            self.thumbnails.begin_pass();
            egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_playlists"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let target_width = max_card_width
                        .map(|limit| ui.available_width().min(limit))
                        .unwrap_or_else(|| ui.available_width());
                    for (index, playlist) in playlists.iter().enumerate() {
                        ui.push_id(("chan_pl", index), |ui| {
                            ui.scope(|ui| {
                                ui.set_max_width(target_width);
                                egui::Frame::new()
                                    .fill(palette.card_bg)
                                    .stroke(egui::Stroke::new(1.0, palette.card_stroke))
                                    .corner_radius(6)
                                    .inner_margin(if compact {
                                        egui::Margin::symmetric(8, 6)
                                    } else {
                                        egui::Margin::symmetric(10, 8)
                                    })
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal_top(|ui| {
                                            let thumb_url = &playlist.thumbnail;
                                            if self.thumbnails.show(ui, ctx, thumb_url).clicked() {
                                                open_playlist = Some(playlist.clone());
                                            }
                                            ui.add_space(8.0);
                                            ui.vertical(|ui| {
                                                ui.set_width(ui.available_width().max(60.0));
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new("Playlist pública")
                                                            .small()
                                                            .strong()
                                                            .color(palette.info_accent_fg),
                                                    );
                                                    if let Some(count) = playlist.item_count {
                                                        ui.label(
                                                            RichText::new(if count == 1 {
                                                                "• 1 vídeo".into()
                                                            } else {
                                                                format!(
                                                                    "• {} vídeos",
                                                                    format_thousands(count)
                                                                )
                                                            })
                                                            .small()
                                                            .weak(),
                                                        );
                                                    }
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if ui
                                                                .button(
                                                                    RichText::new("Abrir vídeos")
                                                                        .small(),
                                                                )
                                                                .clicked()
                                                            {
                                                                open_playlist =
                                                                    Some(playlist.clone());
                                                            }
                                                        },
                                                    );
                                                });
                                                let title_size = if compact { 13.5 } else { 15.0 };
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(&playlist.title)
                                                                .size(title_size)
                                                                .strong(),
                                                        )
                                                        .frame(false)
                                                        .wrap(),
                                                    )
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    open_playlist = Some(playlist.clone());
                                                }
                                                ui.label(
                                                    RichText::new(&playlist.url).small().weak(),
                                                );
                                                if let Some(desc) = description_snippet(
                                                    playlist.description.as_deref(),
                                                    160,
                                                ) {
                                                    ui.add_space(2.0);
                                                    ui.label(
                                                        RichText::new(desc)
                                                            .small()
                                                            .color(palette.snippet_fg),
                                                    );
                                                }
                                            });
                                        });
                                    });
                            });
                        });
                        ui.add_space(6.0);
                    }
                });
            self.thumbnails.end_pass(ctx);
        }

        if cancel_playlists {
            self.cancel_channel_playlists();
        } else if reload_playlists {
            self.start_channel_playlists(ctx);
        } else if let Some(pl) = open_playlist {
            self.open_channel_public_playlist(pl, ctx);
        }
        None
    }

    fn channel_about_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        scroll_id: &str,
        compact: bool,
        card_max_w: f32,
    ) {
        let palette = self.palette();
        let loading_about = self.is_channel_about_loading();
        let about_error = self.channel_about_error.clone();
        let about = self.channel_about.clone();
        let mut reload_about = false;
        let mut cancel_about = false;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Informações e metadados públicos do canal")
                    .small()
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !loading_about,
                        egui::Button::new(RichText::new("↻ Atualizar informações").small()),
                    )
                    .clicked()
                {
                    reload_about = true;
                }
            });
        });
        ui.add_space(6.0);

        if loading_about {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Consultando aba Sobre do canal...")
                                .color(palette.info_accent_fg),
                        );
                        if ui.button(RichText::new("Cancelar").small()).clicked() {
                            cancel_about = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if let Some(err) = &about_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Erro ao consultar detalhes da aba Sobre: {err}"
                            ))
                            .color(palette.error_banner_fg),
                        );
                        if ui
                            .button(RichText::new("↻ Tentar novamente").small())
                            .clicked()
                        {
                            reload_about = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        egui::ScrollArea::vertical()
            .id_salt(format!("{scroll_id}_about"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_stroke))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_min_width(card_max_w);
                        ui.set_max_width(card_max_w);
                        let title = channel_page_display_title(page);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(title)
                                    .size(if compact { 15.5 } else { 17.5 })
                                    .strong(),
                            );
                            if page.verified == Some(true) {
                                ui.label(
                                    RichText::new("✔ Canal verificado")
                                        .small()
                                        .strong()
                                        .color(palette.info_accent_fg),
                                );
                            }
                        });
                        ui.add_space(6.0);

                        if let Some(handle) = page
                            .handle
                            .as_deref()
                            .or_else(|| about.as_ref().and_then(|a| a.handle.as_deref()))
                        {
                            ui.label(RichText::new(format!("Handle: {handle}")).small());
                        }
                        if let Some(channel_id) = page
                            .channel_id
                            .as_deref()
                            .or_else(|| about.as_ref().and_then(|a| a.channel_id.as_deref()))
                        {
                            ui.label(RichText::new(format!("ID do canal: {channel_id}")).small());
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("URL canônica: {}", page.canonical_url))
                                    .small(),
                            );
                            if ui
                                .button(RichText::new("Copiar link").small())
                                .on_hover_text(
                                    "Copiar URL canônica do canal para a área de transferência",
                                )
                                .clicked()
                            {
                                ctx.copy_text(page.canonical_url.clone());
                            }
                        });
                        if let Some(subs) = page
                            .subscriber_count
                            .or_else(|| about.as_ref().and_then(|a| a.subscriber_count))
                        {
                            ui.label(
                                RichText::new(format!(
                                    "Inscritos: {}",
                                    format_subscriber_count(subs)
                                ))
                                .small(),
                            );
                        }
                        if let Some(views) = page
                            .view_count
                            .or_else(|| about.as_ref().and_then(|a| a.view_count))
                        {
                            ui.label(
                                RichText::new(format!(
                                    "Visualizações totais reportadas: {}",
                                    format_view_count(views)
                                ))
                                .small(),
                            );
                        }
                        ui.label(
                            RichText::new(format!(
                                "Vídeos carregados na sessão: {}",
                                page.videos.len()
                            ))
                            .small(),
                        );

                        if let Some(media_line) = format_channel_media_line(page) {
                            ui.label(RichText::new(format!("Mídia: {media_line}")).small().weak());
                        }

                        let tags = if !page.tags.is_empty() {
                            &page.tags
                        } else if let Some(about_info) = &about {
                            &about_info.tags
                        } else {
                            &page.tags
                        };
                        if !tags.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!("Tags do canal ({}):", tags.len()))
                                    .small()
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(tags.join(", "))
                                    .small()
                                    .color(palette.snippet_fg),
                            );
                        }

                        Self::channel_about_description_ui(ui, page, about.as_ref(), palette);
                    });
            });

        if cancel_about {
            self.cancel_channel_about();
        } else if reload_about {
            self.start_channel_about(ctx);
        }
    }

    fn channel_load_status_ui(
        &self,
        ui: &mut egui::Ui,
        loading: bool,
        loading_target: Option<&ChannelTarget>,
        error: Option<&str>,
        retry_target: Option<&ChannelTarget>,
        palette: ThemePalette,
    ) -> (bool, Option<ChannelTarget>) {
        let mut cancel_initial = false;
        let mut reload_target = None;

        if loading {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let target_text = loading_target
                            .map(channel_target_display_label)
                            .unwrap_or_else(|| "canal".into());
                        ui.label(
                            RichText::new(format!("Carregando canal {target_text}..."))
                                .color(palette.info_accent_fg),
                        );
                        if ui
                            .button(RichText::new("Cancelar").small())
                            .on_hover_text("Cancelar o carregamento em andamento do canal")
                            .clicked()
                        {
                            cancel_initial = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if let Some(error_msg) = error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar canal: {error_msg}"))
                                .color(palette.error_banner_fg),
                        );
                        if let Some(target) = retry_target {
                            if ui
                                .button(RichText::new("↻ Tentar novamente").small())
                                .on_hover_text("Tentar buscar novamente os dados do canal")
                                .clicked()
                            {
                                reload_target = Some(target.clone());
                            }
                        }
                    });
                });
            ui.add_space(6.0);
        }

        (cancel_initial, reload_target)
    }

    fn channel_playlists_empty_state_ui(ui: &mut egui::Ui, card_max_w: f32, palette: ThemePalette) {
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(card_max_w);
                ui.label(RichText::new("Nenhuma playlist pública listada para este canal.").weak());
            });
    }

    fn channel_about_description_ui(
        ui: &mut egui::Ui,
        page: &ChannelPage,
        about: Option<&search::ChannelAboutInfo>,
        palette: ThemePalette,
    ) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new("Descrição do canal").small().strong());
        let full_desc = about
            .and_then(|about| clean_description(about.description.as_deref()))
            .or_else(|| clean_description(page.description.as_deref()));
        if let Some(desc) = full_desc {
            ui.add_space(4.0);
            ui.label(RichText::new(desc).color(palette.description_fg));
        } else {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Nenhuma descrição pública informada nos metadados do canal.")
                    .small()
                    .weak(),
            );
        }
    }

    fn queue_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        max_height: Option<f32>,
    ) {
        let queued = self.queue.videos().to_vec();
        let len = self.queue.len();
        let limit = self.queue.limit();
        let palette = self.palette();
        let mut advance_clicked = false;
        ui.horizontal(|ui| {
            let heading_size = if compact { 15.0 } else { 17.0 };
            ui.label(
                RichText::new("Fila de reprodução")
                    .size(heading_size)
                    .strong(),
            );
            ui.label(RichText::new(format!("({len}/{limit})")).small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !queued.is_empty(),
                        egui::Button::new(RichText::new("⏭ Próximo").small()),
                    )
                    .on_hover_text("Avançar manualmente para o próximo vídeo da fila")
                    .clicked()
                {
                    advance_clicked = true;
                }
            });
        });
        if let Some(error) = &self.queue_persistence_error {
            ui.colored_label(Color32::RED, error);
        }
        if let Some(error) = &self.queue_error {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(RichText::new(error).small().color(palette.error_banner_fg));
                });
        }
        if let Some(notice) = &self.queue_notice {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(palette.notice_bg)
                .stroke(egui::Stroke::new(1.0, palette.notice_stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(RichText::new(notice).small().color(palette.notice_fg));
                });
        }
        ui.add_space(4.0);
        if queued.is_empty() {
            ui.label(
                RichText::new(
                    "Fila vazia. Adicione resultados com '+ Fila' e avance manualmente com '⏭ Próximo'.",
                )
                .small()
                .weak(),
            );
            if advance_clicked {
                self.request_queue_advance();
            }
            return;
        }
        let mut reorder_action: Option<(usize, usize)> = None;
        let mut remove_action: Option<usize> = None;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, true]);
        if let Some(limit_h) = max_height {
            scroll = scroll.max_height(limit_h);
        }
        scroll.show(ui, |ui| {
            let target_width = max_card_width
                .map(|limit_w| ui.available_width().min(limit_w))
                .unwrap_or_else(|| ui.available_width());
            for (index, item) in queued.iter().enumerate() {
                ui.push_id(index, |ui| {
                    ui.scope(|ui| {
                        ui.set_max_width(target_width);
                        let is_next = index == 0;
                        let fill = if is_next {
                            palette.queue_next_bg
                        } else {
                            palette.card_alt_bg
                        };
                        egui::Frame::new()
                            .fill(fill)
                            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                            .corner_radius(6)
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let badge = if is_next {
                                        format!("Próximo #{}", index + 1)
                                    } else {
                                        format!("#{}", index + 1)
                                    };
                                    ui.label(RichText::new(badge).small().strong());
                                    ui.vertical(|ui| {
                                        let text_w = (ui.available_width() - 88.0).max(50.0);
                                        ui.set_max_width(text_w);
                                        ui.label(RichText::new(&item.title).small().strong());
                                        if !item.channel.is_empty() || !item.duration.is_empty() {
                                            let sub = if item.channel.is_empty() {
                                                item.duration.clone()
                                            } else if item.duration.is_empty() {
                                                item.channel.clone()
                                            } else {
                                                format!("{} • {}", item.channel, item.duration)
                                            };
                                            ui.label(RichText::new(sub).small().weak());
                                        }
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add_sized(
                                                    [24.0, 22.0],
                                                    egui::Button::new(RichText::new("✕").small()),
                                                )
                                                .on_hover_text("Remover da fila")
                                                .clicked()
                                            {
                                                remove_action = Some(index);
                                            }
                                            if ui
                                                .add_enabled(
                                                    index + 1 < queued.len(),
                                                    egui::Button::new(RichText::new("↓").small())
                                                        .min_size(egui::vec2(24.0, 22.0)),
                                                )
                                                .on_hover_text("Mover para baixo na fila")
                                                .clicked()
                                            {
                                                reorder_action = Some((index, index + 1));
                                            }
                                            if ui
                                                .add_enabled(
                                                    index > 0,
                                                    egui::Button::new(RichText::new("↑").small())
                                                        .min_size(egui::vec2(24.0, 22.0)),
                                                )
                                                .on_hover_text("Mover para cima na fila")
                                                .clicked()
                                            {
                                                reorder_action = Some((index, index - 1));
                                            }
                                        },
                                    );
                                });
                            });
                    });
                });
                ui.add_space(4.0);
            }
        });
        if let Some((from, to)) = reorder_action {
            self.reorder_queue(from, to);
        }
        if let Some(index) = remove_action {
            self.remove_from_queue(index);
        }
        if advance_clicked {
            self.request_queue_advance();
        }
    }

    fn history_entry_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        entry: &HistoryEntry,
        compact: bool,
        is_playing: bool,
    ) -> (bool, bool, bool) {
        let mut play_clicked = false;
        let mut remove_clicked = false;
        let mut enqueue_clicked = false;
        let palette = self.palette();
        let fill = if is_playing {
            palette.card_playing_bg
        } else {
            palette.card_alt_bg
        };
        let stroke = if is_playing {
            egui::Stroke::new(1.0, palette.card_playing_stroke)
        } else {
            egui::Stroke::new(1.0, palette.card_alt_stroke)
        };
        let margin = if compact {
            egui::Margin::symmetric(8, 6)
        } else {
            egui::Margin::symmetric(10, 8)
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(6)
            .inner_margin(margin)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    let play_label = if entry.resume_position_ms().is_some() {
                        "▶ Retomar"
                    } else {
                        "▶ Abrir"
                    };
                    if ui
                        .add_sized(
                            [74.0, 26.0],
                            egui::Button::new(RichText::new(play_label).small()),
                        )
                        .on_hover_text("Reproduzir vídeo a partir do histórico")
                        .clicked()
                    {
                        play_clicked = true;
                    }
                    ui.add_space(6.0);
                    let content_width = (ui.available_width() - 34.0).max(60.0);
                    ui.vertical(|ui| {
                        ui.set_width(content_width);
                        let title_size = if compact { 13.5 } else { 14.5 };
                        let title_button = egui::Button::new(
                            RichText::new(&entry.title).size(title_size).strong(),
                        )
                        .frame(false)
                        .wrap();
                        if ui
                            .add(title_button)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            play_clicked = true;
                        }
                        ui.horizontal_wrapped(|ui| {
                            if !entry.channel.is_empty() {
                                ui.label(RichText::new(&entry.channel).small().weak());
                                ui.separator();
                            }
                            let status = format_history_status(entry);
                            let status_color = if entry.resume_position_ms().is_some() {
                                palette.resume_status_fg
                            } else {
                                palette.neutral_status_fg
                            };
                            ui.label(RichText::new(status).small().color(status_color));
                            if ui
                                .small_button("+ Fila")
                                .on_hover_text("Adicionar vídeo à fila")
                                .clicked()
                            {
                                enqueue_clicked = true;
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if ui
                            .add_sized([24.0, 24.0], egui::Button::new(RichText::new("✕").small()))
                            .on_hover_text("Remover este vídeo do histórico")
                            .clicked()
                        {
                            remove_clicked = true;
                        }
                    });
                });
            });
        (play_clicked, remove_clicked, enqueue_clicked)
    }

    fn history_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        max_height: Option<f32>,
    ) -> Option<Video> {
        self.ensure_history_synchronized();
        if self.history.entries().is_empty() {
            return None;
        }
        let entries: Vec<HistoryEntry> = self.history.entries().to_vec();
        let current_video_id = self
            .selected
            .as_ref()
            .and_then(|video| history::youtube_video_id(&video.url));

        ui.horizontal(|ui| {
            let heading_size = if compact { 15.0 } else { 17.0 };
            ui.label(RichText::new("Histórico local").size(heading_size).strong());
            ui.label(
                RichText::new(format!("({} vídeos)", entries.len()))
                    .small()
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new("Limpar histórico").small())
                    .on_hover_text("Apagar todos os itens do histórico local")
                    .clicked()
                {
                    self.clear_history();
                }
            });
        });
        ui.add_space(6.0);

        let mut chosen = None;
        let mut to_remove = None;
        let mut to_enqueue = None;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, true]);
        if let Some(limit) = max_height {
            scroll = scroll.max_height(limit);
        }
        scroll.show(ui, |ui| {
            let target_width = max_card_width
                .map(|limit| ui.available_width().min(limit))
                .unwrap_or_else(|| ui.available_width());
            for entry in &entries {
                let is_playing = current_video_id.as_deref() == Some(entry.video_id.as_str());
                ui.scope(|ui| {
                    ui.set_max_width(target_width);
                    let (play_clicked, remove_clicked, enqueue_clicked) =
                        self.history_entry_card_ui(ui, entry, compact, is_playing);
                    if play_clicked {
                        chosen = self.video_for_history_entry(entry);
                    }
                    if enqueue_clicked {
                        to_enqueue = self.video_for_history_entry(entry);
                    }
                    if remove_clicked {
                        to_remove = Some(entry.video_id.clone());
                    }
                });
                ui.add_space(6.0);
            }
        });
        if let Some(video) = to_enqueue {
            let _ = self.enqueue_video(video);
        }
        if let Some(video_id) = to_remove {
            self.remove_history_entry(&video_id);
        }
        chosen
    }

    fn watch_recommendation_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        compact: bool,
    ) -> bool {
        let mut clicked = false;
        let mut open_channel_target = None;
        let palette = self.palette();
        let metadata = self.metadata_for(&video.url).cloned();
        let channel_target = self.channel_target_for_video(video);
        let history_entry =
            history::youtube_video_id(&video.url).and_then(|id| self.history.get(&id).cloned());
        let progress_ratio = history_entry.as_ref().and_then(|entry| {
            let duration_ms = entry.duration_ms? as f32;
            if duration_ms <= 0.0 || entry.position_ms == 0 {
                None
            } else {
                Some((entry.position_ms as f32 / duration_ms).clamp(0.04, 1.0))
            }
        });
        let panel_width = ui.available_width().max(160.0);
        let thumb_width = if compact {
            (panel_width * 0.38).clamp(132.0, 168.0)
        } else {
            (panel_width * 0.34).clamp(168.0, 220.0)
        };
        let thumb_size = egui::vec2(thumb_width, thumb_width * 9.0 / 16.0);

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let thumb = self
                .thumbnails
                .show_rounded(ui, ctx, &video.thumbnail, thumb_size, 8.0);
            paint_thumbnail_overlays(ui, thumb.rect, &video.duration, progress_ratio, false);
            if thumb.clicked() {
                clicked = true;
            }

            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width().max(60.0));
                let title_button = egui::Button::new(
                    RichText::new(truncate_text_chars(&video.title, 88))
                        .size(14.0)
                        .strong()
                        .color(palette.header_button_fg),
                )
                .frame(false)
                .wrap();
                if ui
                    .add(title_button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(&video.title)
                    .clicked()
                {
                    clicked = true;
                }

                if !video.channel.is_empty() {
                    let channel_button = egui::Button::new(
                        RichText::new(truncate_text_chars(&video.channel, 42))
                            .size(12.0)
                            .color(palette.neutral_status_fg),
                    )
                    .frame(false);
                    if ui
                        .add(channel_button)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Abrir canal")
                        .clicked()
                    {
                        open_channel_target = channel_target.clone();
                    }
                }

                if let Some(subline) =
                    format_youtube_card_subline(&video.duration, metadata.as_ref())
                {
                    ui.label(
                        RichText::new(subline)
                            .size(12.0)
                            .color(palette.neutral_status_fg),
                    );
                }

                if history_entry.is_some() {
                    ui.label(
                        RichText::new("Continuar assistindo")
                            .size(11.5)
                            .color(palette.resume_status_fg),
                    );
                }
            });
        });

        if let Some(target) = open_channel_target {
            self.open_channel(target, ctx);
        }
        clicked
    }

    fn results_feed_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        _scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
    ) -> Option<Video> {
        let mut chosen = None;
        let current_url = self.selected.as_ref().map(|video| video.url.clone());
        let feed_generation = self.feed_data_generation;
        let videos = std::mem::take(&mut self.videos);
        self.thumbnails.begin_pass();
        let target_width = max_card_width
            .map(|limit| ui.available_width().min(limit))
            .unwrap_or_else(|| ui.available_width());
        for (index, video) in videos.iter().enumerate() {
            let is_playing = current_url.as_deref() == Some(video.url.as_str());
            ui.push_id(index, |ui| {
                ui.scope(|ui| {
                    ui.set_max_width(target_width);
                    if self.watch_recommendation_card_ui(ui, ctx, video, compact) {
                        if self.home_playlist_urls.contains(&video.url) {
                            self.start_browser_feed(video.url.clone(), false, ctx);
                        } else {
                            chosen = Some(video.clone());
                        }
                    }
                });
            });
            ui.add_space(6.0);
        }
        self.thumbnails.end_pass(ctx);
        if self.feed_data_generation == feed_generation {
            self.videos = videos;
        }
        chosen
    }

    fn filter_chips_bar_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        profile: &UserProfile,
    ) {
        let palette = self.palette();
        self.ensure_feed_cache();
        let chips = self.feed_cache.chips.clone();
        let mut selected_chip: Option<FeedFilter> = None;
        let mut clear_channel_filter = false;
        let mut refresh_home = false;
        let mut cancel_home = false;

        egui::ScrollArea::horizontal()
            .id_salt("youtube_filter_chips")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

                    if let FeedFilter::Channel(channel_name) = &self.feed_filter {
                        let chip_btn = egui::Button::new(
                            RichText::new(format!("📺 {channel_name}  ✕"))
                                .size(13.0)
                                .strong()
                                .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .corner_radius(8);
                        if ui
                            .add_sized([0.0, 32.0], chip_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Remover filtro de canal e voltar para Tudo")
                            .clicked()
                        {
                            clear_channel_filter = true;
                        }
                    }

                    for chip in chips {
                        let is_active = self.feed_filter == chip
                            && self.browse_section != BrowseSection::History
                            && self.browse_section != BrowseSection::Queue;
                        let (bg, fg) = if is_active {
                            (palette.active_button_bg, palette.active_button_fg)
                        } else {
                            (palette.header_button_bg, palette.header_button_fg)
                        };
                        let label = chip.label();
                        let text = if is_active {
                            RichText::new(&label).size(13.0).strong().color(fg)
                        } else {
                            RichText::new(&label).size(13.0).color(fg)
                        };
                        let btn = egui::Button::new(text).fill(bg).corner_radius(8);
                        if ui
                            .add_sized([0.0, 32.0], btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            selected_chip = Some(chip);
                        }
                    }

                    if self.home_search.is_some() {
                        let cancel_btn = egui::Button::new(
                            RichText::new("✕ Cancelar carregamento")
                                .size(12.5)
                                .color(palette.header_button_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(8);
                        if ui
                            .add_sized([0.0, 30.0], cancel_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            cancel_home = true;
                        }
                    } else if self.home_title.is_some() {
                        let refresh_btn = egui::Button::new(
                            RichText::new("↻")
                                .size(13.5)
                                .color(palette.header_button_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(8);
                        if ui
                            .add_sized([32.0, 30.0], refresh_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Atualizar envios das inscrições")
                            .clicked()
                        {
                            refresh_home = true;
                        }
                    }
                });
            });

        if clear_channel_filter {
            self.feed_filter = FeedFilter::All;
            self.browse_section = BrowseSection::Home;
            self.invalidate_feed_cache();
        }
        if let Some(chip) = selected_chip {
            self.search_results_mode = false;
            self.browse_section = if matches!(chip, FeedFilter::Shorts) {
                BrowseSection::Shorts
            } else {
                BrowseSection::Home
            };
            self.feed_filter = chip.clone();
            self.invalidate_feed_cache();
            if self.search.is_none() && self.home_search.is_none() {
                self.query = RecommendationEngine::build_discovery_query(profile, &chip);
                self.start_search(ctx);
            }
        }
        if cancel_home {
            self.cancel_home_feed(ctx);
        } else if refresh_home {
            self.start_browser_feed(":ytrec".into(), false, ctx);
        }
    }

    fn youtube_grid_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scored: &ScoredVideo,
        width: f32,
    ) -> (bool, bool, Option<ChannelTarget>, Option<String>) {
        let video = &scored.video;
        let palette = self.palette();
        let is_playlist = self.home_playlist_urls.contains(&video.url);
        let metadata = self.metadata_for(&video.url).cloned();
        let channel_target = self.channel_target_for_video(video);
        let history_entry =
            history::youtube_video_id(&video.url).and_then(|id| self.history.get(&id).cloned());
        let progress_ratio = history_entry.as_ref().and_then(|entry| {
            let dur = entry.duration_ms? as f32;
            if dur <= 0.0 || entry.position_ms == 0 {
                None
            } else {
                Some((entry.position_ms as f32 / dur).clamp(0.04, 1.0))
            }
        });

        let mut open_clicked = false;
        let mut enqueue_clicked = false;
        let mut open_channel = None;
        let mut filter_channel = None;

        ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let thumb_height = (width * 9.0 / 16.0).round();
            let thumb_resp = self.thumbnails.show_rounded(
                ui,
                ctx,
                &video.thumbnail,
                egui::vec2(width, thumb_height),
                12.0,
            );
            let is_music = recommendations::is_music_video(video, metadata.as_ref());
            let overlay_badge = if is_playlist {
                "☰ Playlist"
            } else {
                video.duration.as_str()
            };
            paint_thumbnail_overlays(
                ui,
                thumb_resp.rect,
                overlay_badge,
                progress_ratio,
                is_music && !is_playlist,
            );
            if thumb_resp.clicked() {
                open_clicked = true;
            }

            ui.add_space(10.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let avatar_label = if video.channel.trim().is_empty() {
                    "FastTube"
                } else {
                    video.channel.as_str()
                };
                let avatar_resp = paint_channel_avatar(ui, avatar_label, 36.0);
                if avatar_resp.clicked() {
                    if let Some(target) = &channel_target {
                        open_channel = Some(target.clone());
                    } else if !video.channel.trim().is_empty() {
                        filter_channel = Some(video.channel.clone());
                    }
                }

                let text_col_w = (ui.available_width() - 48.0).max(80.0);
                ui.vertical(|ui| {
                    ui.set_width(text_col_w);
                    ui.set_max_width(text_col_w);

                    let title_limit = if width < 220.0 {
                        44
                    } else if width < 280.0 {
                        58
                    } else {
                        72
                    };
                    let clean_title = truncate_text_chars(&video.title, title_limit);
                    let title_btn = egui::Button::new(
                        RichText::new(clean_title)
                            .size(16.0)
                            .strong()
                            .color(palette.channel_fg),
                    )
                    .frame(false)
                    .wrap();
                    if ui
                        .add(title_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(&video.title)
                        .clicked()
                    {
                        open_clicked = true;
                    }

                    ui.add_space(2.0);
                    if !video.channel.is_empty() {
                        let verified_suffix = if channel_target.is_some() { " ✔" } else { "" };
                        let ch_btn = egui::Button::new(
                            RichText::new(format!("{}{}", video.channel, verified_suffix))
                                .size(13.5)
                                .color(palette.neutral_status_fg),
                        )
                        .frame(false);
                        if ui
                            .add(ch_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            if let Some(target) = &channel_target {
                                open_channel = Some(target.clone());
                            } else {
                                filter_channel = Some(video.channel.clone());
                            }
                        }
                    }

                    if let Some(subline) =
                        format_youtube_card_subline(&video.duration, metadata.as_ref())
                    {
                        ui.label(
                            RichText::new(subline)
                                .size(13.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if !is_playlist {
                        let queue_btn = egui::Button::new(
                            RichText::new("Fila")
                                .size(11.5)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false);
                        if ui
                            .add_sized([38.0, 24.0], queue_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Adicionar vídeo à fila de reprodução")
                            .clicked()
                        {
                            enqueue_clicked = true;
                        }
                    }
                });
            });
        });

        (open_clicked, enqueue_clicked, open_channel, filter_channel)
    }

    fn shorts_shelf_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        shorts: &[ScoredVideo],
        available_width: f32,
    ) -> (Option<Video>, Option<Video>) {
        if shorts.is_empty() {
            return (None, None);
        }
        let palette = self.palette();
        let mut selected = None;
        let enqueue = None;

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // Ícone vermelho do Shorts + título "Shorts" igual ao YouTube
            let (badge_rect, _) =
                ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(badge_rect, 6.0, Color32::from_rgb(255, 0, 51));
            ui.painter().text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                "⚡",
                egui::FontId::proportional(13.0),
                Color32::WHITE,
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Shorts")
                    .size(19.0)
                    .strong()
                    .color(palette.channel_fg),
            );
        });
        ui.add_space(10.0);

        let columns = ((available_width + 14.0) / 195.0).floor().clamp(2.0, 5.0) as usize;
        let card_w = ((available_width - 14.0 * (columns.saturating_sub(1) as f32))
            / columns as f32)
            .max(130.0);
        let card_h = (card_w * 16.0 / 9.0).round();

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            for scored in shorts.iter().take(columns) {
                let video = &scored.video;
                ui.push_id(("short_shelf", &video.url), |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        ui.set_max_width(card_w);
                        let thumb = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            &video.thumbnail,
                            egui::vec2(card_w, card_h),
                            12.0,
                        );
                        paint_thumbnail_overlays(ui, thumb.rect, &video.duration, None, false);
                        ui.add_space(6.0);
                        let short_title = truncate_text_chars(&video.title, 48);
                        let title_resp = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(short_title)
                                        .size(13.5)
                                        .strong()
                                        .color(palette.channel_fg),
                                )
                                .frame(false)
                                .wrap(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&video.title);
                        if thumb.clicked() || title_resp.clicked() {
                            selected = Some(video.clone());
                        }
                        let meta = self.metadata_for(&video.url);
                        let views_line = meta
                            .and_then(|m| m.view_count)
                            .map(recommendations::format_youtube_compact_views)
                            .or_else(|| (!video.channel.is_empty()).then(|| video.channel.clone()));
                        if let Some(sub) = views_line {
                            ui.label(
                                RichText::new(sub)
                                    .size(12.0)
                                    .color(palette.neutral_status_fg),
                            );
                        }
                    });
                });
            }
        });
        ui.add_space(20.0);
        (selected, enqueue)
    }

    fn home_grid_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        self.ensure_feed_cache();
        let mut ranked = std::mem::take(&mut self.feed_cache.ranked);
        let is_subscription_feed = self
            .home_title
            .as_deref()
            .is_some_and(|title| title.contains("envios recentes"));
        // Os filtros da página de inscrições alteram apenas a visualização. Preserva
        // a classificação base para que desmarcar um filtro não elimine candidatos.
        let ranked_base = is_subscription_feed.then(|| ranked.clone());
        if is_subscription_feed {
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                changed |= ui
                    .checkbox(&mut self.feed_hide_watched, "Ocultar assistidos")
                    .changed();
                changed |= ui
                    .checkbox(&mut self.feed_hide_shorts, "Ocultar Shorts")
                    .changed();
                changed |= ui
                    .checkbox(&mut self.feed_hide_lives, "Ocultar lives")
                    .changed();
                changed |= ui
                    .checkbox(&mut self.feed_newest_first, "Mais recentes")
                    .changed();
                let channels: Vec<String> = self
                    .videos
                    .iter()
                    .map(|video| video.channel.clone())
                    .filter(|name| !name.is_empty())
                    .collect();
                egui::ComboBox::from_id_salt("subscription_channel_filter")
                    .selected_text(self.feed_channel.as_deref().unwrap_or("Todos os canais"))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.feed_channel.is_none(), "Todos os canais")
                            .clicked()
                        {
                            self.feed_channel = None;
                            changed = true;
                        }
                        for channel in channels {
                            if ui
                                .selectable_label(
                                    self.feed_channel.as_deref() == Some(&channel),
                                    &channel,
                                )
                                .clicked()
                            {
                                self.feed_channel = Some(channel);
                                changed = true;
                            }
                        }
                    });
            });
            if changed {
                if let Some(path) = &self.feed_prefs_path {
                    let pref = feed_prefs::FeedPreferences {
                        hide_watched: self.feed_hide_watched,
                        hide_shorts: self.feed_hide_shorts,
                        hide_lives: self.feed_hide_lives,
                        newest_first: self.feed_newest_first,
                        channel: self.feed_channel.clone(),
                    };
                    if let Err(error) = feed_prefs::save(path, &pref) {
                        self.message = format!("Filtros não salvos: {error}");
                    }
                }
            }
            let filters = feed_prefs::FeedPreferences {
                hide_watched: self.feed_hide_watched,
                hide_shorts: self.feed_hide_shorts,
                hide_lives: self.feed_hide_lives,
                newest_first: self.feed_newest_first,
                channel: self.feed_channel.clone(),
            };
            ranked.retain(|item| {
                let viewed = history::youtube_video_id(&item.video.url)
                    .and_then(|id| self.history.get(&id))
                    .is_some_and(|entry| entry.is_near_end());
                let short = recommendations::is_short_video(&item.video, Some(&item.metadata));
                let live = recommendations::is_live_video(&item.video, Some(&item.metadata));
                filters.include(viewed, short, live, &item.video.channel)
            });
            if self.feed_newest_first {
                ranked.sort_by_key(|item| {
                    std::cmp::Reverse(item.metadata.upload_date.clone().unwrap_or_default())
                });
            }
        }

        let show_shorts_only = matches!(self.feed_filter, FeedFilter::Shorts)
            || self.browse_section == BrowseSection::Shorts;
        let (shorts_list, regular_list): (Vec<ScoredVideo>, Vec<ScoredVideo>) = if show_shorts_only
        {
            (Vec::new(), ranked)
        } else {
            ranked.into_iter().partition(|item| item.is_short)
        };
        // Se todos os vídeos forem curtos ou não houver vídeos regulares suficientes, exibe todos na grade principal
        let (main_videos, shelf_shorts) = if regular_list.is_empty() {
            (shorts_list, Vec::new())
        } else {
            (regular_list, shorts_list)
        };

        let mut selected = None;
        let mut opened_short = false;
        let mut playlist = None;
        let mut enqueue = None;
        let mut open_channel = None;
        let mut filter_channel = None;

        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt("home_grid")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available = ui.available_width().max(220.0);
                let gap = 16.0;
                let columns = compute_home_grid_columns(available);
                let width = ((available - gap * (columns.saturating_sub(1) as f32))
                    / columns as f32)
                    .max(160.0);
                let row_height = (width * 9.0 / 16.0).round() + 10.0 + 96.0;
                let expanded_clip = ui.clip_rect().expand2(egui::vec2(0.0, 450.0));

                for (row_index, row) in main_videos.chunks(columns).enumerate() {
                    let cursor = ui.cursor();
                    let row_top = cursor.min.y;
                    let row_rect = egui::Rect::from_min_size(
                        egui::pos2(cursor.min.x, row_top),
                        egui::vec2(available, row_height),
                    );
                    if row_index > 0 && !row_rect.intersects(expanded_clip) {
                        ui.allocate_exact_size(
                            egui::vec2(available, row_height + 24.0),
                            egui::Sense::hover(),
                        );
                        continue;
                    }
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for scored in row {
                            let video = &scored.video;
                            ui.push_id(&video.url, |ui| {
                                let is_playlist = self.home_playlist_urls.contains(&video.url);
                                let (open_card, enqueue_card, ch_target, ch_filter) =
                                    self.youtube_grid_card_ui(ui, ctx, scored, width);
                                if open_card {
                                    if is_playlist {
                                        playlist = Some(video.url.clone());
                                    } else {
                                        selected = Some(video.clone());
                                    }
                                }
                                if enqueue_card {
                                    enqueue = Some(video.clone());
                                }
                                if ch_target.is_some() {
                                    open_channel = ch_target;
                                }
                                if ch_filter.is_some() {
                                    filter_channel = ch_filter;
                                }
                            });
                        }
                    });
                    ui.add_space(24.0);

                    // Após a primeira linha de vídeos 16:9, exibe a prateleira "Shorts" igual ao YouTube
                    if row_index == 0 && !shelf_shorts.is_empty() {
                        let (short_sel, short_enq) =
                            self.shorts_shelf_ui(ui, ctx, &shelf_shorts, available);
                        if short_sel.is_some() {
                            opened_short = true;
                            selected = short_sel;
                        }
                        if short_enq.is_some() {
                            enqueue = short_enq;
                        }
                    }
                }

                if main_videos.is_empty() && !shelf_shorts.is_empty() {
                    let (short_sel, short_enq) =
                        self.shorts_shelf_ui(ui, ctx, &shelf_shorts, available);
                    if short_sel.is_some() {
                        opened_short = true;
                        selected = short_sel;
                    }
                    if short_enq.is_some() {
                        enqueue = short_enq;
                    }
                }
            });
        self.feed_cache.ranked =
            ranked_base.unwrap_or_else(|| main_videos.into_iter().chain(shelf_shorts).collect());
        self.thumbnails.end_pass(ctx);

        if opened_short {
            self.browse_section = BrowseSection::Shorts;
            self.feed_filter = FeedFilter::Shorts;
        }
        if let Some(video) = enqueue {
            let _ = self.enqueue_video(video);
        }
        if let Some(target) = playlist {
            self.start_browser_feed(target, false, ctx);
        }
        if let Some(target) = open_channel {
            self.open_channel(target, ctx);
        } else if let Some(ch_name) = filter_channel {
            self.feed_filter = FeedFilter::Channel(ch_name);
            self.invalidate_feed_cache();
        }
        selected
    }

    fn youtube_search_result_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
    ) -> (bool, bool, Option<ChannelTarget>, Option<String>) {
        let palette = self.palette();
        let metadata = self.metadata_for(&video.url).cloned();
        let channel_target = self.channel_target_for_video(video);
        let history_entry =
            history::youtube_video_id(&video.url).and_then(|id| self.history.get(&id).cloned());
        let progress_ratio = history_entry.as_ref().and_then(|entry| {
            let duration_ms = entry.duration_ms? as f32;
            if duration_ms <= 0.0 || entry.position_ms == 0 {
                None
            } else {
                Some((entry.position_ms as f32 / duration_ms).clamp(0.04, 1.0))
            }
        });
        let description = description_snippet(
            metadata
                .as_ref()
                .and_then(|meta| meta.description.as_deref()),
            240,
        );
        let subline = format_youtube_card_subline(&video.duration, metadata.as_ref());
        let is_playlist = self.home_playlist_urls.contains(&video.url);
        let overlay_badge = if is_playlist {
            "Playlist"
        } else {
            video.duration.as_str()
        };
        let panel_width = ui.available_width().max(220.0);
        let wide = panel_width >= 700.0;
        let thumb_width = if wide {
            (panel_width * 0.36).clamp(280.0, 420.0)
        } else {
            panel_width
        };
        let thumb_size = egui::vec2(thumb_width, (thumb_width * 9.0 / 16.0).round());

        let open_clicked = Cell::new(false);
        let mut enqueue_clicked = false;
        let mut open_channel = None;
        let mut filter_channel = None;

        let mut show_details = |ui: &mut egui::Ui| {
            ui.set_width(ui.available_width().max(120.0));
            ui.horizontal_top(|ui| {
                let title_width = (ui.available_width() - 42.0).max(100.0);
                ui.vertical(|ui| {
                    ui.set_max_width(title_width);
                    let title = if is_playlist {
                        format!("{} \u{00b7} Playlist", video.title)
                    } else {
                        video.title.clone()
                    };
                    let title_response = ui.add(
                        egui::Button::new(
                            RichText::new(title)
                                .size(if wide { 19.0 } else { 17.0 })
                                .strong()
                                .color(palette.header_button_fg),
                        )
                        .frame(false)
                        .wrap(),
                    );
                    if title_response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        open_clicked.set(true);
                    }

                    if let Some(line) = &subline {
                        ui.label(
                            RichText::new(line)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let channel_label = if video.channel.trim().is_empty() {
                            channel_target
                                .as_ref()
                                .map(channel_target_display_label)
                                .unwrap_or("Canal")
                        } else {
                            video.channel.as_str()
                        };
                        if paint_channel_avatar(ui, channel_label, 26.0).clicked() {
                            if let Some(target) = &channel_target {
                                open_channel = Some(target.clone());
                            } else if !video.channel.trim().is_empty() {
                                filter_channel = Some(video.channel.clone());
                            }
                        }
                        let channel_button = egui::Button::new(
                            RichText::new(channel_label)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        )
                        .frame(false);
                        if ui
                            .add(channel_button)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            if let Some(target) = &channel_target {
                                open_channel = Some(target.clone());
                            } else if !video.channel.trim().is_empty() {
                                filter_channel = Some(video.channel.clone());
                            }
                        }
                    });

                    if let Some(text) = description.as_ref() {
                        ui.add_space(5.0);
                        ui.label(RichText::new(text).size(12.5).color(palette.snippet_fg));
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if !is_playlist
                        && ui
                            .small_button("+ Fila")
                            .on_hover_text("Adicionar \u{00e0} fila de reprodu\u{00e7}\u{00e3}o")
                            .clicked()
                    {
                        enqueue_clicked = true;
                    }
                });
            });
        };

        if wide {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let thumb =
                    self.thumbnails
                        .show_rounded(ui, ctx, &video.thumbnail, thumb_size, 12.0);
                paint_thumbnail_overlays(ui, thumb.rect, overlay_badge, progress_ratio, false);
                if thumb.clicked() {
                    open_clicked.set(true);
                }
                show_details(ui);
            });
        } else {
            let thumb = self
                .thumbnails
                .show_rounded(ui, ctx, &video.thumbnail, thumb_size, 12.0);
            paint_thumbnail_overlays(ui, thumb.rect, overlay_badge, progress_ratio, false);
            if thumb.clicked() {
                open_clicked.set(true);
            }
            ui.add_space(10.0);
            show_details(ui);
        }

        (
            open_clicked.get(),
            enqueue_clicked,
            open_channel,
            filter_channel,
        )
    }

    fn search_results_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        let palette = self.palette();
        if self.videos.is_empty() {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                if self.search.is_some() {
                    ui.spinner();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Pesquisando no YouTube...")
                            .size(15.0)
                            .color(palette.neutral_status_fg),
                    );
                } else {
                    ui.label(
                        RichText::new("Nenhum resultado encontrado.")
                            .size(18.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Tente outra pesquisa ou remova os filtros.")
                            .size(13.0)
                            .color(palette.neutral_status_fg),
                    );
                }
            });
            return None;
        }

        let mut chosen = None;
        let mut enqueue = None;
        let mut open_channel = None;
        let mut filter_channel = None;
        let generation = self.feed_data_generation;
        let videos = std::mem::take(&mut self.videos);
        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt("youtube_search_results")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 18.0;
                for (index, video) in videos.iter().enumerate() {
                    let is_playlist = self.home_playlist_urls.contains(&video.url);
                    let (open, queued, channel, channel_filter) =
                        self.youtube_search_result_card_ui(ui, ctx, video);
                    if open {
                        if is_playlist {
                            self.start_browser_feed(video.url.clone(), false, ctx);
                        } else {
                            chosen = Some(video.clone());
                        }
                    }
                    if queued {
                        enqueue = Some(video.clone());
                    }
                    if channel.is_some() {
                        open_channel = channel;
                    }
                    if channel_filter.is_some() {
                        filter_channel = channel_filter;
                    }
                    if index + 1 < videos.len() {
                        ui.separator();
                    }
                }
            });
        self.thumbnails.end_pass(ctx);
        if self.feed_data_generation == generation {
            self.videos = videos;
        }
        if let Some(video) = enqueue {
            let _ = self.enqueue_video(video);
        }
        if let Some(target) = open_channel {
            self.open_channel(target, ctx);
        } else if let Some(channel) = filter_channel {
            self.search_results_mode = true;
            self.feed_filter = FeedFilter::Channel(channel);
            self.invalidate_feed_cache();
        }
        chosen
    }

    fn watch_sidebar_tabs_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &ThemePalette,
    ) {
        let tab_button = |ui: &mut egui::Ui, label: &str, selected: bool| {
            let response = ui.add(
                egui::Button::new(RichText::new(label).size(12.5).strong().color(if selected {
                    palette.header_button_fg
                } else {
                    palette.neutral_status_fg
                }))
                .fill(Color32::TRANSPARENT)
                .frame(false)
                .min_size(egui::vec2(0.0, 34.0)),
            );
            if selected {
                let y = response.rect.bottom() - 1.0;
                ui.painter().line_segment(
                    [
                        egui::pos2(response.rect.left() + 8.0, y),
                        egui::pos2(response.rect.right() - 8.0, y),
                    ],
                    egui::Stroke::new(2.0, Color32::from_rgb(255, 0, 51)),
                );
            }
            response
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 4.0);
            if tab_button(
                ui,
                "Em seguida",
                self.watch_extras.tab == watch_extras::Tab::Results,
            )
            .clicked()
            {
                self.watch_extras.tab = watch_extras::Tab::Results;
            }
            if tab_button(
                ui,
                "Coment\u{00e1}rios",
                self.watch_extras.tab == watch_extras::Tab::Comments,
            )
            .clicked()
            {
                self.watch_extras.tab = watch_extras::Tab::Comments;
                if !self.watch_extras.comments_loaded {
                    self.watch_extras
                        .start_comments(self.oauth_session.as_ref(), false, ctx);
                }
            }
            if tab_button(
                ui,
                "Legendas",
                self.watch_extras.tab == watch_extras::Tab::Subtitles,
            )
            .clicked()
            {
                self.watch_extras.tab = watch_extras::Tab::Subtitles;
                if !self.watch_extras.subtitles_loaded {
                    self.watch_extras.start_subtitles(ctx);
                }
            }
            if tab_button(
                ui,
                "Marcadores",
                self.watch_extras.tab == watch_extras::Tab::Markers,
            )
            .clicked()
            {
                self.watch_extras.tab = watch_extras::Tab::Markers;
            }
        });
    }

    fn watch_sidebar_markers_ui(&self, ui: &mut egui::Ui) {
        let id = self
            .selected
            .as_ref()
            .and_then(|v| history::youtube_video_id(&v.url));
        if let Some(id) = id {
            let marks: Vec<_> = self
                .library
                .bookmarks
                .iter()
                .filter(|m| m.video_id == id)
                .cloned()
                .collect();
            if marks.is_empty() {
                ui.label("Nenhum marcador para este vídeo.");
            }
            for mark in marks {
                if ui
                    .link(format!(
                        "{} — {}",
                        format_time(mark.time_ms as f64 / 1000.0),
                        mark.note
                    ))
                    .clicked()
                {
                    #[cfg(windows)]
                    if let Some(player) = &self.player {
                        let _ = player.seek(mark.time_ms as f64 / 1000.0, "absolute");
                    }
                }
            }
        } else {
            ui.label("Marcadores disponíveis para vídeos do YouTube.");
        }
    }

    fn watch_sidebar_queue_section_ui(&mut self, ui: &mut egui::Ui, scroll_id: &str) {
        if !self.queue.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new(format!(
                    "Fila de reprodu\u{00e7}\u{00e3}o ({})",
                    self.queue.len()
                ))
                .size(14.0)
                .strong(),
            )
            .id_salt(("watch_queue", scroll_id))
            .default_open(false)
            .show(ui, |ui| {
                self.queue_section_ui(ui, &format!("{scroll_id}_queue"), true, None, Some(180.0));
            });
            ui.add_space(8.0);
        }
    }

    fn watch_sidebar_feed_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
    ) -> Option<Video> {
        ui.label(RichText::new("Em seguida").size(16.0).strong());
        ui.add_space(6.0);
        if self.videos.is_empty() {
            let text = if self.search.is_some() {
                "Pesquisando v\u{00ed}deos..."
            } else {
                "Nenhuma recomenda\u{00e7}\u{00e3}o no momento."
            };
            ui.label(RichText::new(text).weak());
            if !self.history.entries().is_empty() {
                ui.add_space(10.0);
                return self.history_section_ui(
                    ui,
                    &format!("{scroll_id}_history"),
                    true,
                    None,
                    None,
                );
            }
            return None;
        }
        self.results_feed_ui(ui, ctx, scroll_id, true, None)
    }

    fn watch_sidebar_results_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
    ) -> Option<Video> {
        self.sync_theme_visuals(ctx);
        self.ensure_history_synchronized();
        let palette = self.palette();
        self.watch_sidebar_tabs_ui(ui, ctx, &palette);
        ui.add_space(6.0);
        if self.watch_extras.tab == watch_extras::Tab::Comments {
            self.comments_ui(ui, ctx, scroll_id);
            return None;
        }
        if self.watch_extras.tab == watch_extras::Tab::Subtitles {
            self.subtitles_ui(ui, ctx, scroll_id);
            return None;
        }
        if self.watch_extras.tab == watch_extras::Tab::Markers {
            self.watch_sidebar_markers_ui(ui);
            return None;
        }
        self.watch_sidebar_queue_section_ui(ui, scroll_id);
        if self.channel_view_open {
            return self.channel_section_ui(ui, ctx, &format!("{scroll_id}_channel"), true, None);
        }
        self.watch_sidebar_feed_ui(ui, ctx, scroll_id)
    }

    fn comments_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, scroll_id: &str) {
        let needs_reauth = self
            .oauth_session
            .as_ref()
            .is_some_and(|s| !s.has_comment_scope());
        ui.horizontal(|ui| {
            ui.heading("Coment\u{00e1}rios");
            if ui
                .add_enabled(
                    self.watch_extras.comments_loading.is_none(),
                    egui::Button::new("\u{21bb} Atualizar"),
                )
                .clicked()
            {
                self.watch_extras
                    .start_comments(self.oauth_session.as_ref(), false, ctx);
            }
        });
        ui.label(
            RichText::new(format!(
                "{} de 100 coment\u{00e1}rios \u{2022} somente leitura",
                self.watch_extras.comments.len()
            ))
            .small()
            .weak(),
        );
        if needs_reauth {
            if self.reauth.is_some() {
                ui.spinner();
                ui.label("Aguardando autoriza\u{00e7}\u{00e3}o no navegador...");
                if ui.button("Cancelar").clicked() {
                    self.cancel_reauth();
                }
            } else if ui.button("Autorizar coment\u{00e1}rios").clicked() {
                self.start_reauth(ctx);
            }
        }
        if self.watch_extras.comments_loading.is_some() {
            ui.spinner();
        }
        if let Some(error) = &self.watch_extras.comments_error {
            ui.label(error);
        }
        if self.watch_extras.comments_loaded
            && self.watch_extras.comments.is_empty()
            && self.watch_extras.comments_error.is_none()
        {
            ui.label("Nenhum comentário disponível.");
        }
        egui::ScrollArea::vertical()
            .id_salt(format!("{scroll_id}_comments"))
            .show(ui, |ui| {
                let comments = self.watch_extras.comments.clone();
                for comment in &comments {
                    ui.separator();
                    ui.label(RichText::new(&comment.author).strong());
                    ui.label(&comment.text);
                    ui.label(
                        RichText::new(format!(
                            "{}  •  {} curtidas  •  {} respostas",
                            comment.published, comment.likes, comment.replies
                        ))
                        .small()
                        .weak(),
                    );
                    if comment.replies > 0 {
                        let state = self.watch_extras.comment_replies.get(&comment.id);
                        if state.is_none_or(|state| !state.loaded) {
                            let loading = state.is_some_and(|state| state.is_loading());
                            if ui
                                .add_enabled(
                                    !loading,
                                    egui::Button::new(format!(
                                        "Carregar respostas ({})",
                                        comment.replies
                                    )),
                                )
                                .clicked()
                            {
                                self.watch_extras.start_replies(
                                    &comment.id,
                                    false,
                                    self.oauth_session.as_ref(),
                                    ctx,
                                );
                            }
                        }
                        let (loaded, loading, error, replies, has_next) = self
                            .watch_extras
                            .comment_replies
                            .get(&comment.id)
                            .map(|state| {
                                (
                                    state.loaded,
                                    state.is_loading(),
                                    state.error.clone(),
                                    state.comments.clone(),
                                    state.next.is_some(),
                                )
                            })
                            .unwrap_or_default();
                        if loading {
                            ui.spinner();
                        }
                        if let Some(error) = error {
                            ui.label(error);
                            if ui.button("Tentar novamente").clicked() {
                                let retry_next = self
                                    .watch_extras
                                    .comment_replies
                                    .get(&comment.id)
                                    .is_some_and(
                                        watch_extras::CommentReplies::retry_uses_next_page,
                                    );
                                self.watch_extras.start_replies(
                                    &comment.id,
                                    retry_next,
                                    self.oauth_session.as_ref(),
                                    ctx,
                                );
                            }
                        }
                        if loaded {
                            ui.label(
                                RichText::new(format!(
                                    "{} de 100 respostas carregadas",
                                    replies.len()
                                ))
                                .small()
                                .weak(),
                            );
                        }
                        for reply in &replies {
                            ui.indent(&reply.id, |ui| {
                                ui.label(RichText::new(&reply.author).strong());
                                ui.label(&reply.text);
                                ui.label(RichText::new(&reply.published).small().weak());
                            });
                        }
                        if has_next && ui.button("Carregar mais respostas").clicked() {
                            self.watch_extras.start_replies(
                                &comment.id,
                                true,
                                self.oauth_session.as_ref(),
                                ctx,
                            );
                        }
                    }
                }
                if self.watch_extras.comments_next.is_some()
                    && ui
                        .add_enabled(
                            self.watch_extras.comments_loading.is_none(),
                            egui::Button::new("Carregar mais comentários"),
                        )
                        .clicked()
                {
                    self.watch_extras
                        .start_comments(self.oauth_session.as_ref(), true, ctx);
                }
            });
    }

    fn subtitles_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, scroll_id: &str) {
        ui.horizontal(|ui| {
            ui.heading("Legendas");
            if ui
                .add_enabled(
                    self.watch_extras.subtitles_loading.is_none(),
                    egui::Button::new("↻ Buscar novamente"),
                )
                .clicked()
            {
                self.watch_extras.start_subtitles(ctx);
            }
        });
        ui.label(
            RichText::new("Faixas do YouTube obtidas pelo yt-dlp; disponibilidade pode variar.")
                .small()
                .weak(),
        );
        if self.watch_extras.subtitles_loading.is_some() {
            ui.spinner();
        }
        if let Some(error) = &self.watch_extras.subtitles_error {
            ui.colored_label(self.palette().warning_fg, error);
        }
        if self.watch_extras.subtitles_loaded
            && self.watch_extras.subtitles.is_empty()
            && self.watch_extras.subtitles_error.is_none()
        {
            ui.label("Nenhuma legenda compatível encontrada.");
        }
        let mut chosen = None;
        let mut transcript_track = None;
        egui::ScrollArea::vertical()
            .id_salt(format!("{scroll_id}_subtitles"))
            .max_height(150.0)
            .show(ui, |ui| {
                for track in &self.watch_extras.subtitles {
                    let label = format!(
                        "{}{}",
                        track.language,
                        if track.automatic {
                            " (automática)"
                        } else {
                            ""
                        }
                    );
                    ui.horizontal(|ui| {
                        if ui.button(&label).clicked() {
                            chosen = Some((track.clone(), label.clone()));
                        }
                        if ui
                            .small_button("Transcrição")
                            .on_hover_text("Baixar, pesquisar e acompanhar o WebVTT desta faixa")
                            .clicked()
                        {
                            transcript_track = Some(track.clone());
                        }
                    });
                }
            });
        if let Some((track, label)) = chosen {
            #[cfg(windows)]
            {
                let result = self
                    .player
                    .as_ref()
                    .ok_or("O player não está disponível.".to_owned())
                    .and_then(|player| player.add_subtitle(&track.url, &label, &track.language));
                self.watch_extras.subtitles_error = result.err();
            }
            #[cfg(not(windows))]
            let _ = (track, label);
        }
        if let Some(track) = transcript_track {
            self.watch_extras.start_transcript(&track, ctx);
        }
        ui.separator();
        ui.heading("Transcrição acessível");
        ui.label(
            RichText::new(
                "Use a busca para navegar ou acompanhe o trecho ativo sem tirar os olhos do vídeo.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            ui.label("Buscar:");
            ui.text_edit_singleline(&mut self.watch_extras.transcript_query);
            if !self.watch_extras.transcript_query.is_empty() && ui.small_button("Limpar").clicked()
            {
                self.watch_extras.transcript_query.clear();
            }
            if self.watch_extras.transcript_loading {
                ui.spinner();
            }
        });
        let mut follow = self.watch_extras.transcript_follow;
        let mut font_size = self.watch_extras.transcript_font_size.clamp(13.0, 24.0);
        if self.watch_extras.transcript_font_size <= 0.0 {
            font_size = 16.0;
        }
        let mut high_contrast = self.watch_extras.transcript_high_contrast;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut follow, "Acompanhar reprodução");
            ui.checkbox(&mut high_contrast, "Alto contraste");
            ui.add(
                egui::Slider::new(&mut font_size, 13.0..=24.0)
                    .integer()
                    .text("Tamanho"),
            );
        });
        self.watch_extras.transcript_follow = follow;
        self.watch_extras.transcript_font_size = font_size;
        self.watch_extras.transcript_high_contrast = high_contrast;
        if let Some(error) = &self.watch_extras.transcript_error {
            ui.colored_label(self.palette().warning_fg, error);
        }
        if !self.watch_extras.transcript_loading
            && self.watch_extras.transcript.is_empty()
            && self.watch_extras.transcript_error.is_none()
        {
            ui.label("Escolha Transcrição em uma faixa acima para carregar o texto.");
        }
        let position_ms = self.current_player_live_pos_ms();
        let active_index = position_ms.and_then(|position| {
            transcript::active_cue_index(&self.watch_extras.transcript, position)
        });
        let mut seek_to_ms = None;
        if let Some(index) = active_index {
            let cue = &self.watch_extras.transcript[index];
            let (fill, stroke, text_color) = if high_contrast {
                if self.theme == Theme::Dark {
                    (
                        Color32::from_rgb(0, 0, 0),
                        Color32::from_rgb(255, 220, 80),
                        Color32::WHITE,
                    )
                } else {
                    (
                        Color32::from_rgb(255, 246, 184),
                        Color32::from_rgb(74, 57, 0),
                        Color32::from_rgb(20, 16, 0),
                    )
                }
            } else {
                (
                    self.palette().notice_bg,
                    self.palette().notice_stroke,
                    self.palette().notice_fg,
                )
            };
            egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, stroke))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Trecho atual")
                            .small()
                            .strong()
                            .color(text_color),
                    );
                    let text = format!(
                        "{}  {}",
                        format_time(cue.start_ms as f64 / 1000.0),
                        cue.text
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(text)
                                    .size(font_size)
                                    .color(text_color)
                                    .strong(),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .wrap(),
                        )
                        .on_hover_text("Voltar para o início deste trecho")
                        .clicked()
                    {
                        seek_to_ms = Some(cue.start_ms);
                    }
                });
        }
        let query = self.watch_extras.transcript_query.trim().to_owned();
        if !query.is_empty() {
            let matches = transcript::search(&self.watch_extras.transcript, &query);
            if matches.is_empty() {
                ui.label("Nenhum trecho corresponde à busca.");
            } else {
                ui.label(
                    RichText::new(format!(
                        "{} ocorrência(s); exibindo até 200.",
                        matches.len()
                    ))
                    .small()
                    .weak(),
                );
                egui::ScrollArea::vertical()
                    .id_salt(format!("{scroll_id}_transcript_matches"))
                    .max_height(360.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for cue in matches.into_iter().take(200) {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!(
                                            "{}  {}",
                                            format_time(cue.start_ms as f64 / 1000.0),
                                            cue.text
                                        ))
                                        .size(font_size),
                                    )
                                    .frame(false)
                                    .wrap(),
                                )
                                .clicked()
                            {
                                seek_to_ms = Some(cue.start_ms);
                            }
                        }
                    });
            }
        } else if !self.watch_extras.transcript.is_empty() {
            let palette = self.palette();
            let row_height = (font_size * 2.4).max(36.0);
            let mut scroll = egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_transcript_cues"))
                .max_height(360.0)
                .auto_shrink([false, true]);
            if self.watch_extras.transcript_follow {
                if let Some(index) = active_index {
                    scroll = scroll.vertical_scroll_offset(
                        (index as f32 * row_height - row_height * 3.0).max(0.0),
                    );
                }
            }
            scroll.show_rows(
                ui,
                row_height,
                self.watch_extras.transcript.len(),
                |ui, visible| {
                    for index in visible {
                        let cue = &self.watch_extras.transcript[index];
                        let is_active = active_index == Some(index);
                        let row_rect = ui
                            .allocate_exact_size(
                                egui::vec2(ui.available_width(), row_height),
                                egui::Sense::hover(),
                            )
                            .0;
                        let fill = if is_active {
                            if high_contrast && self.theme == Theme::Dark {
                                Color32::from_rgb(55, 45, 0)
                            } else {
                                palette.queue_next_bg
                            }
                        } else if index % 2 == 0 {
                            Color32::TRANSPARENT
                        } else {
                            palette.message_box_bg
                        };
                        if fill != Color32::TRANSPARENT {
                            ui.painter().rect_filled(row_rect, 4.0, fill);
                        }
                        let text = format!(
                            "{}  {}",
                            format_time(cue.start_ms as f64 / 1000.0),
                            truncate_text_chars(&cue.text, 180)
                        );
                        let text = RichText::new(text).size(font_size);
                        let text = if is_active { text.strong() } else { text };
                        let button = egui::Button::new(text)
                            .frame(false)
                            .fill(Color32::TRANSPARENT);
                        if ui
                            .put(row_rect.shrink2(egui::vec2(8.0, 3.0)), button)
                            .on_hover_text(&cue.text)
                            .clicked()
                        {
                            seek_to_ms = Some(cue.start_ms);
                        }
                    }
                },
            );
        }
        if let Some(start_ms) = seek_to_ms {
            #[cfg(windows)]
            if let Some(player) = &self.player {
                if let Err(error) = player.seek(start_ms as f64 / 1000.0, "absolute") {
                    self.message = error;
                }
            }
        }
    }

    fn library_video_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        title: &str,
        description: &str,
        videos: &[Video],
        scroll_id: &str,
    ) -> Option<Video> {
        let palette = self.palette();
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(20.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !videos.is_empty() {
                    ui.label(
                        RichText::new(format!("{} itens", videos.len()))
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                    );
                }
            });
        });
        ui.add_space(5.0);

        if videos.is_empty() {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(16, 16))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new(description)
                            .size(13.0)
                            .color(palette.neutral_status_fg),
                    );
                });
            return None;
        }

        self.thumbnails.begin_pass();
        egui::ScrollArea::horizontal()
            .id_salt(scroll_id)
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for video in videos.iter().take(16) {
                        ui.push_id((scroll_id, &video.url), |ui| {
                            ui.set_width(350.0);
                            ui.set_max_width(350.0);
                            if self.watch_recommendation_card_ui(ui, ctx, video, true) {
                                chosen = Some(video.clone());
                            }
                        });
                    }
                });
            });
        self.thumbnails.end_pass(ctx);
        chosen
    }

    fn explore_section_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, available_width: f32) {
        let palette = self.palette();
        ui.heading("Explorar");
        ui.label(
            RichText::new(
                "Escolha um assunto para montar uma descoberta. O ranking abaixo combina suas preferências locais com os resultados públicos carregados.",
            )
            .weak(),
        );
        ui.add_space(12.0);

        let categories = [
            (
                "Em alta",
                "Conteúdo em destaque",
                FeedFilter::RecentlyUploaded,
                "tendências brasil hoje",
            ),
            (
                "Música",
                "Clipes, álbuns e apresentações",
                FeedFilter::Music,
                "música clipe oficial",
            ),
            (
                "Jogos",
                "Gameplay, análise e esports",
                FeedFilter::Gaming,
                "gameplay jogos brasil",
            ),
            (
                "Podcasts",
                "Conversas e episódios completos",
                FeedFilter::Podcasts,
                "podcast entrevista completa",
            ),
            (
                "Ao vivo",
                "Transmissões em tempo real",
                FeedFilter::Live,
                "ao vivo transmissão",
            ),
            (
                "Mixes",
                "Sequências para ouvir por mais tempo",
                FeedFilter::Mixes,
                "mix música playlist",
            ),
            (
                "Novidades",
                "Assuntos que você ainda não viu",
                FeedFilter::NewToYou,
                "novidades cultura tecnologia brasil",
            ),
        ];
        let columns = if available_width < 620.0 { 1 } else { 2 };
        let gap = 10.0;
        let tile_width =
            ((available_width - gap * (columns as f32 - 1.0)) / columns as f32).clamp(180.0, 360.0);
        let mut chosen: Option<(FeedFilter, &str)> = None;

        egui::Grid::new("fasttube_explore_categories")
            .num_columns(columns)
            .spacing(egui::vec2(gap, gap))
            .show(ui, |ui| {
                for (index, (title, subtitle, filter, query)) in categories.into_iter().enumerate()
                {
                    let button = egui::Button::new(
                        RichText::new(format!("{title}\n{subtitle}"))
                            .size(13.0)
                            .strong()
                            .color(palette.header_button_fg),
                    )
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12);
                    if ui
                        .add_sized([tile_width, 66.0], button)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        chosen = Some((filter, query));
                    }
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });

        if let Some((filter, query)) = chosen {
            self.browse_section = BrowseSection::Home;
            self.search_results_mode = false;
            self.feed_filter = filter;
            self.query = query.to_owned();
            self.invalidate_feed_cache();
            self.start_search(ctx);
        }
    }

    fn browse_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        self.ensure_history_synchronized();
        let palette = self.palette();
        self.browse_alerts_ui(ui, palette);

        if self.channel_view_open {
            return self.channel_section_ui(
                ui,
                ctx,
                "browse_channel_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
            );
        }

        if self.browse_section == BrowseSection::Explore {
            self.explore_section_ui(ui, ctx, ui.available_width());
            return None;
        }

        self.browse_subscriptions_header_ui(ui);

        if !self.videos.is_empty() && self.browse_section == BrowseSection::Home {
            if let Some(video) = self.continue_watching_ui(ui, ctx) {
                return Some(video);
            }
            ui.add_space(12.0);
        }

        self.ensure_feed_cache();
        let profile = self.feed_cache.profile.clone();
        self.filter_chips_bar_ui(ui, ctx, &profile);
        ui.add_space(12.0);

        if self.search_results_mode && self.browse_section == BrowseSection::Home {
            return self.search_results_ui(ui, ctx);
        }

        if self.browse_section == BrowseSection::History {
            return self.history_section_ui(
                ui,
                "browse_history_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                None,
            );
        }

        if self.browse_section == BrowseSection::Queue {
            self.queue_section_ui(
                ui,
                "browse_queue_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                None,
            );
            return None;
        }

        if self.browse_section == BrowseSection::Library {
            return self.browse_library_ui(ui, ctx, palette);
        }

        return self.browse_home_content_ui(ui, ctx, palette);
    }

    fn browse_alerts_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette) {
        ui.add_space(4.0);
        if let Some(error) = self
            .settings_error
            .as_ref()
            .or(self.playback_settings_error.as_ref())
            .or(self.theme_settings_error.as_ref())
        {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Aviso de configurações: {error}"))
                            .color(palette.error_banner_fg),
                    );
                });
            ui.add_space(6.0);
        }
        if let Some(error) = &self.history_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Aviso do histórico: {error}"))
                            .color(palette.error_banner_fg),
                    );
                });
            ui.add_space(6.0);
        }
        if let Some(notice) = &self.history_notice {
            egui::Frame::new()
                .fill(palette.notice_bg)
                .stroke(egui::Stroke::new(1.0, palette.notice_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(notice).small().color(palette.notice_fg));
                });
            ui.add_space(6.0);
        }
    }

    fn browse_subscriptions_header_ui(&self, ui: &mut egui::Ui) {
        if self.browse_section == BrowseSection::Subscriptions {
            ui.heading("Inscri\u{00e7}\u{00f5}es");
            ui.label(
                RichText::new(if self.session_authenticated {
                    "Envios recentes dos canais que você acompanha. O Feed FastTube reorganiza os itens localmente."
                } else {
                    "Entre com Google para carregar os canais que você acompanha. A Home pública continua disponível."
                })
                .weak(),
            );
            ui.add_space(8.0);
        }
    }

    fn browse_library_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let mut chosen = None;
        let mut bookmark_to_open = None;
        let watch_later: Vec<Video> = self
            .library
            .watch_later
            .iter()
            .map(|record| record.video.clone())
            .collect();
        let favorites: Vec<Video> = self
            .library
            .favorites
            .iter()
            .map(|record| record.video.clone())
            .collect();
        let collections: Vec<(String, Vec<Video>)> = self
            .library
            .collections
            .iter()
            .map(|collection| {
                (
                    collection.name.clone(),
                    collection
                        .videos
                        .iter()
                        .map(|record| record.video.clone())
                        .collect(),
                )
            })
            .collect();
        let bookmarks = self.library.bookmarks.clone();

        egui::ScrollArea::vertical()
            .id_salt("browse_library_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(RichText::new("Você").size(28.0).strong());
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{} salvos · {} favoritos · {} coleções · {} marcadores",
                        watch_later.len(),
                        favorites.len(),
                        collections.len(),
                        bookmarks.len()
                    ))
                    .size(13.0)
                    .color(palette.neutral_status_fg),
                );
                if let Some(error) = &self.library_error {
                    ui.add_space(6.0);
                    ui.colored_label(palette.warning_fg, error);
                }
                ui.add_space(16.0);

                if let Some(video) = self.library_video_section_ui(
                    ui,
                    ctx,
                    "Assistir depois",
                    "Os vídeos salvos para assistir depois aparecerão aqui.",
                    &watch_later,
                    "library_watch_later",
                ) {
                    chosen = Some(video);
                }
                ui.add_space(20.0);
                if let Some(video) = self.library_video_section_ui(
                    ui,
                    ctx,
                    "Favoritos",
                    "Favorite vídeos nos cartões para encontrá-los aqui.",
                    &favorites,
                    "library_favorites",
                ) {
                    chosen = Some(video);
                }

                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Coleções").size(20.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}", collections.len()))
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    });
                });
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [240.0, 34.0],
                                egui::TextEdit::singleline(&mut self.collection_name)
                                    .hint_text("Nome da nova coleção"),
                            );
                            let create = egui::Button::new(
                                RichText::new("Criar")
                                    .size(13.0)
                                    .strong()
                                    .color(palette.active_button_fg),
                            )
                            .fill(palette.active_button_bg)
                            .corner_radius(18);
                            if ui.add_sized([72.0, 34.0], create).clicked() {
                                let name = self.collection_name.trim().to_owned();
                                if !name.is_empty()
                                    && name.len() <= 128
                                    && !self
                                        .library
                                        .collections
                                        .iter()
                                        .any(|collection| collection.name == name)
                                    && self.library.collections.len() < 100
                                {
                                    self.library.collections.push(library::Collection {
                                        name,
                                        videos: Vec::new(),
                                    });
                                    self.collection_name.clear();
                                    self.save_library();
                                }
                            }
                        });
                    });
                ui.add_space(10.0);

                if collections.is_empty() {
                    egui::Frame::new()
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(16, 16))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new(
                                    "Crie coleções para organizar vídeos por assunto ou projeto.",
                                )
                                .size(13.0)
                                .color(palette.neutral_status_fg),
                            );
                        });
                } else {
                    for (index, (name, videos)) in collections.iter().enumerate() {
                        if let Some(video) = self.library_video_section_ui(
                            ui,
                            ctx,
                            name,
                            "Esta coleção ainda não tem vídeos.",
                            videos,
                            &format!("library_collection_{index}"),
                        ) {
                            chosen = Some(video);
                        }
                        ui.add_space(18.0);
                    }
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Marcadores").size(20.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}", bookmarks.len()))
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    });
                });
                ui.add_space(6.0);
                if bookmarks.is_empty() {
                    ui.label(
                        RichText::new("Marcadores criados durante a reprodução aparecerão aqui.")
                            .size(13.0)
                            .color(palette.neutral_status_fg),
                    );
                } else {
                    for mark in &bookmarks {
                        egui::Frame::new()
                            .fill(palette.card_alt_bg)
                            .corner_radius(10)
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                let timestamp = format_time(mark.time_ms as f64 / 1000.0);
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("{timestamp}  {}", mark.note))
                                                .size(13.0)
                                                .strong()
                                                .color(palette.header_button_fg),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text(&mark.title)
                                    .clicked()
                                {
                                    bookmark_to_open = Some(mark.video_id.clone());
                                }
                                ui.label(
                                    RichText::new(&mark.title)
                                        .size(12.0)
                                        .color(palette.neutral_status_fg),
                                );
                            });
                        ui.add_space(6.0);
                    }
                }
            });

        if let Some(video_id) = bookmark_to_open {
            if let Some(url) = history::youtube_watch_url(&video_id) {
                self.query = url;
                self.start_search(ctx);
            }
        }
        return chosen;
    }

    fn browse_home_content_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        if !self.message.is_empty()
            && self.message != "Pesquise um vídeo ou cole um link do YouTube."
            && (self.videos.is_empty() || self.home_search.is_some())
        {
            egui::Frame::new()
                .fill(palette.message_box_bg)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(&self.message).small());
                });
            ui.add_space(8.0);
        }

        if !self.videos.is_empty() {
            return self.home_grid_ui(ui, ctx);
        }

        let mut chosen_from_history = None;
        if !self.queue.is_empty() {
            self.queue_section_ui(
                ui,
                "browse_queue_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                Some(180.0),
            );
            ui.add_space(8.0);
        }
        if !self.history.entries().is_empty() {
            chosen_from_history = self.history_section_ui(
                ui,
                "browse_history_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                None,
            );
        }
        chosen_from_history
    }

    fn browse_page_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        let available_width = ui.available_width();
        let content_width = (available_width - 48.0)
            .max(1.0)
            .min(BROWSE_MAX_CONTENT_WIDTH);
        let side_margin = ((available_width - content_width) * 0.5).max(0.0);
        let mut chosen = None;
        ui.horizontal_top(|ui| {
            ui.add_space(side_margin);
            ui.vertical(|ui| {
                ui.set_width(content_width);
                ui.set_max_width(content_width);
                chosen = self.browse_ui(ui, ctx);
            });
        });
        chosen
    }

    fn shorts_watch_channel_ui(
        &self,
        ui: &mut egui::Ui,
        channel_label: &str,
        channel_target: Option<&ChannelTarget>,
        palette: &ThemePalette,
    ) -> Option<ChannelTarget> {
        let mut open_channel_target = None;
        ui.horizontal_top(|ui| {
            if paint_channel_avatar(
                ui,
                if channel_label.is_empty() {
                    "Canal"
                } else {
                    channel_label
                },
                38.0,
            )
            .clicked()
            {
                open_channel_target = channel_target.cloned();
            }
            ui.vertical(|ui| {
                let label = if channel_label.is_empty() {
                    channel_target
                        .map(channel_target_display_label)
                        .unwrap_or("Canal")
                } else {
                    channel_label
                };
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(label)
                                .size(14.0)
                                .strong()
                                .color(palette.header_button_fg),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    open_channel_target = channel_target.cloned();
                }
                ui.label(
                    RichText::new("Ver canal")
                        .size(12.0)
                        .color(palette.neutral_status_fg),
                );
            });
        });
        open_channel_target
    }

    fn shorts_watch_actions_ui(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video_url: &str,
        saved_for_later: bool,
        palette: &ThemePalette,
        enqueue_video: &mut bool,
        toggle_watch_later: &mut bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            let queue = egui::Button::new(
                RichText::new("Fila")
                    .size(13.0)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            if ui.add_sized([70.0, 36.0], queue).clicked() {
                *enqueue_video = true;
            }
            let share = egui::Button::new(
                RichText::new("Compartilhar")
                    .size(13.0)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            if ui.add_sized([116.0, 36.0], share).clicked() {
                ctx.copy_text(video_url.to_owned());
            }
            let save = egui::Button::new(
                RichText::new(if saved_for_later { "Salvo" } else { "Salvar" })
                    .size(13.0)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            if ui.add_sized([86.0, 36.0], save).clicked() {
                *toggle_watch_later = true;
            }
        });
    }

    fn shorts_watch_details_ui(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        metadata: Option<&VideoMetadata>,
        channel_target: Option<&ChannelTarget>,
        saved_for_later: bool,
        palette: &ThemePalette,
        enqueue_video: &mut bool,
        toggle_watch_later: &mut bool,
        open_channel_target: &mut Option<ChannelTarget>,
    ) {
        let channel_label = video.channel.trim();
        let meta_line = format_youtube_card_subline(&video.duration, metadata);
        let description =
            description_snippet(metadata.and_then(|meta| meta.description.as_deref()), 220);

        ui.label(
            RichText::new("Shorts")
                .size(12.5)
                .strong()
                .color(Color32::from_rgb(255, 0, 51)),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(&video.title)
                .size(19.0)
                .strong()
                .color(palette.header_button_fg),
        );
        if let Some(line) = &meta_line {
            ui.add_space(4.0);
            ui.label(
                RichText::new(line)
                    .size(13.0)
                    .color(palette.neutral_status_fg),
            );
        }
        ui.add_space(12.0);
        if let Some(target) =
            self.shorts_watch_channel_ui(ui, channel_label, channel_target, palette)
        {
            *open_channel_target = Some(target);
        }
        ui.add_space(14.0);
        self.shorts_watch_actions_ui(
            ui,
            ctx,
            &video.url,
            saved_for_later,
            palette,
            enqueue_video,
            toggle_watch_later,
        );
        if let Some(text) = &description {
            ui.add_space(14.0);
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).size(13.0).color(palette.description_fg));
                });
        }
        if !self.message.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(&self.message)
                    .size(12.0)
                    .color(palette.neutral_status_fg),
            );
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Use os controles sobre o vídeo para pausar, ajustar o volume ou abrir a tela cheia.",
            )
            .size(12.0)
            .color(palette.neutral_status_fg),
        );
    }

    fn shorts_watch_page_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        let Some(video) = self.selected.clone() else {
            return None;
        };
        let palette = self.palette();
        let metadata = self.metadata_for(&video.url).cloned();
        let available_width = ui.available_width();
        let available_height = ui.available_height();
        let player_size = compute_player_size_with_theater(
            available_width.min(430.0),
            available_height,
            false,
            false,
            WatchLayoutMode::Short,
        );
        let wide = available_width >= 760.0;
        let mut enqueue_video = false;
        let mut toggle_watch_later = false;
        let mut open_channel_target = None;
        let saved_for_later = self
            .library
            .watch_later
            .iter()
            .any(|record| record.video.url == video.url);
        let channel_target = self.channel_target_for_video(&video);

        egui::ScrollArea::vertical()
            .id_salt("shorts_watch_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                if wide {
                    let details_width = 330.0;
                    let content_width = player_size.x + 24.0 + details_width;
                    let left = ((available_width - content_width) * 0.5).max(0.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(left);
                        ui.vertical(|ui| {
                            ui.set_width(player_size.x);
                            ui.set_max_width(player_size.x);
                            self.watch_primary_ui(
                                ui,
                                ctx,
                                player_size.x,
                                available_height,
                                WatchLayoutMode::Short,
                            );
                        });
                        ui.add_space(24.0);
                        ui.vertical(|ui| {
                            ui.set_width(details_width);
                            ui.set_max_width(details_width);
                            self.shorts_watch_details_ui(
                                ui,
                                ctx,
                                &video,
                                metadata.as_ref(),
                                channel_target.as_ref(),
                                saved_for_later,
                                &palette,
                                &mut enqueue_video,
                                &mut toggle_watch_later,
                                &mut open_channel_target,
                            );
                        });
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        self.watch_primary_ui(
                            ui,
                            ctx,
                            player_size.x,
                            available_height,
                            WatchLayoutMode::Short,
                        );
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        self.shorts_watch_details_ui(
                            ui,
                            ctx,
                            &video,
                            metadata.as_ref(),
                            channel_target.as_ref(),
                            saved_for_later,
                            &palette,
                            &mut enqueue_video,
                            &mut toggle_watch_later,
                            &mut open_channel_target,
                        );
                    });
                }
            });

        if enqueue_video {
            let _ = self.enqueue_video(video.clone());
        }
        if toggle_watch_later {
            self.library.toggle_watch_later(video.clone());
            self.save_library();
        }
        if let Some(target) = open_channel_target {
            self.open_channel(target, ctx);
        }
        None
    }

    fn is_watch_short_mode(&self) -> bool {
        if matches!(self.feed_filter, recommendations::FeedFilter::Shorts)
            || self.browse_section == BrowseSection::Shorts
        {
            self.selected.as_ref().is_some_and(|video| {
                recommendations::is_short_video(video, self.metadata_for(&video.url))
            })
        } else {
            false
        }
    }

    fn watch_page_side_by_side_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        ui.horizontal_top(|ui| {
            ui.add_space(watch_page_margin());
            ui.vertical(|ui| {
                ui.set_width(columns.primary_width);
                ui.set_max_width(columns.primary_width);
                egui::ScrollArea::vertical()
                    .id_salt("watch_primary_page_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(columns.primary_width);
                        self.watch_primary_ui(
                            ui,
                            ctx,
                            columns.primary_width,
                            available_height,
                            WatchLayoutMode::SideBySide,
                        );
                        if !self.message.is_empty() {
                            ui.add_space(6.0);
                            ui.label(&self.message);
                        }
                    });
            });
            ui.add_space(columns.gap);
            ui.vertical(|ui| {
                ui.set_width(columns.secondary_width);
                ui.set_max_width(columns.secondary_width);
                egui::ScrollArea::vertical()
                    .id_salt("watch_recommendations_page_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(columns.secondary_width);
                        chosen =
                            self.watch_sidebar_results_ui(ui, ctx, "watch_sidebar_results_scroll");
                    });
            });
        });
        chosen
    }

    fn watch_page_short_column_ui(
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
                if !self.message.is_empty() {
                    ui.add_space(6.0);
                    ui.label(&self.message);
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                chosen = self.watch_sidebar_results_ui(ui, ctx, "watch_short_results_scroll");
            });
        chosen
    }

    fn watch_page_stacked_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        let scroll_id = if self.theater_mode {
            "watch_theater_main_scroll"
        } else {
            "watch_stacked_page_scroll"
        };
        egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.watch_primary_ui(
                    ui,
                    ctx,
                    columns.primary_width,
                    available_height,
                    WatchLayoutMode::Stacked,
                );
                if !self.message.is_empty() {
                    ui.add_space(6.0);
                    ui.label(&self.message);
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                chosen = self.watch_sidebar_results_ui(
                    ui,
                    ctx,
                    if self.theater_mode {
                        "watch_theater_results_scroll"
                    } else {
                        "watch_stacked_results_scroll"
                    },
                );
            });
        chosen
    }

    fn watch_page_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        if self.is_watch_short_mode() && !self.theater_mode && !self.fullscreen {
            return self.shorts_watch_page_ui(ui, ctx);
        }
        let columns = compute_watch_page_columns(
            ui.available_width(),
            ui.available_height(),
            self.theater_mode,
        );
        let available_height = ui.available_height();
        match columns.mode {
            WatchLayoutMode::SideBySide => {
                self.watch_page_side_by_side_ui(ui, ctx, &columns, available_height)
            }
            WatchLayoutMode::Short => {
                self.watch_page_short_column_ui(ui, ctx, &columns, available_height)
            }
            WatchLayoutMode::Stacked => {
                self.watch_page_stacked_ui(ui, ctx, &columns, available_height)
            }
        }
    }
}

fn paint_youtube_logo_badge(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 20.0), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, 5.0, Color32::from_rgb(255, 0, 51));
    let center = rect.center();
    let triangle = vec![
        egui::pos2(center.x - 3.5, center.y - 4.5),
        egui::pos2(center.x - 3.5, center.y + 4.5),
        egui::pos2(center.x + 4.8, center.y),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        triangle,
        Color32::WHITE,
        egui::Stroke::NONE,
    ));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_channel_avatar(ui: &mut egui::Ui, channel_name: &str, diameter: f32) -> egui::Response {
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

fn youtube_tab_button(
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

fn sidebar_pill_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    active: bool,
    unread_dot: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let desired = egui::vec2(ui.available_width().max(110.0), 40.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let fill = if active {
        palette.card_alt_bg
    } else if response.hovered() {
        palette.header_button_bg
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 10.0, fill);
    }
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(16.0),
        palette.channel_fg,
    );
    let display_label = truncate_text_chars(label, 24);
    ui.painter().text(
        egui::pos2(rect.left() + 48.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_label,
        egui::FontId::proportional(14.0),
        palette.channel_fg,
    );
    if unread_dot {
        ui.painter().circle_filled(
            egui::pos2(rect.right() - 14.0, rect.center().y),
            3.0,
            Color32::from_rgb(62, 166, 255),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn compact_sidebar_item(
    ui: &mut egui::Ui,
    icon: &str,
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
    ui.painter().text(
        egui::pos2(rect.center().x, rect.center().y - 9.0),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(21.0),
        palette.header_button_fg,
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

fn sidebar_channel_item(
    ui: &mut egui::Ui,
    channel_name: &str,
    active: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let desired = egui::vec2(ui.available_width().max(110.0), 34.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let fill = if active {
        palette.header_button_bg
    } else if response.hovered() {
        palette.card_alt_bg
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 10.0, fill);
    }
    let avatar_center = egui::pos2(rect.left() + 20.0, rect.center().y);
    let [r, g, b] = recommendations::channel_avatar_rgb(channel_name);
    let initials = recommendations::channel_avatar_initials(channel_name);
    ui.painter()
        .circle_filled(avatar_center, 11.5, Color32::from_rgb(r, g, b));
    ui.painter().text(
        avatar_center,
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional(9.5),
        Color32::WHITE,
    );
    let display_name = truncate_text_chars(channel_name, 17);
    ui.painter().text(
        egui::pos2(rect.left() + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_name,
        egui::FontId::proportional(13.0),
        palette.channel_fg,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(channel_name)
}

fn paint_thumbnail_overlays(
    ui: &egui::Ui,
    rect: egui::Rect,
    duration: &str,
    progress_ratio: Option<f32>,
    is_music: bool,
) {
    let trimmed = duration.trim();
    if !trimmed.is_empty() {
        let badge_text = if is_music {
            format!("♫ {trimmed}")
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

fn truncate_text_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let prefix: String = trimmed.chars().take(keep).collect();
    format!("{}…", prefix.trim_end())
}

fn format_youtube_card_subline(duration: &str, metadata: Option<&VideoMetadata>) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(views) = metadata.and_then(|meta| meta.view_count) {
        parts.push(format!(
            "▷ {}",
            recommendations::format_youtube_compact_views(views)
        ));
    }
    if let Some(rel_date) = metadata
        .and_then(|meta| meta.upload_date.as_deref())
        .and_then(|date| recommendations::format_youtube_relative_time(date, None))
    {
        parts.push(rel_date);
    }
    if parts.is_empty() && !duration.trim().is_empty() {
        parts.push(duration.trim().to_owned());
    }
    (!parts.is_empty()).then(|| parts.join(" \u{00b7} "))
}

fn format_time(seconds: f64) -> String {
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

impl eframe::App for FastTube {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.watch_extras.sync_video(None);
        if let Some(cancel) = self.search_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.channel_cancel.take() {
            cancel.cancel();
        }
        self.cancel_channel_subrequests();
        if let Some(cancel) = self.quality_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.home_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.reauth_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.rating_cancel.take() {
            cancel.cancel();
        }
        self.reauth = None;
        self.rating_request = None;
        let now = Instant::now();
        #[cfg(windows)]
        self.sample_player_before_transition(now);
        self.flush_playback_history_at(now);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.watch_extras
            .sync_video(self.selected.as_ref().map(|video| video.url.as_str()));
        self.watch_extras.poll();
        self.poll_async_tasks(&ctx);
        if self.session_authenticated && !self.oauth_restore_attempted {
            self.oauth_restore_attempted = true;
            self.start_browser_feed(":ytrec".into(), false, &ctx);
        } else if !self.session_authenticated && !self.initial_public_home_started {
            self.initial_public_home_started = true;
            self.start_public_home(&ctx);
        }
        self.in_app_ui_pass = true;
        self.sync_theme_visuals(&ctx);
        self.scrolling.apply(&ctx);
        self.thumbnails.maintain(&ctx);
        #[cfg(windows)]
        if self.selected.is_some() {
            let ended = self
                .player
                .as_ref()
                .is_some_and(|player| player.property("eof-reached").as_deref() == Some("yes"))
                && self.queue_auto_advance_ready(true);
            if queue::should_advance(ended, &mut self.queue_eof_handled, !self.queue.is_empty()) {
                let _ = self.advance_queue_with_frame(frame, &ctx);
            }
        }
        let escape_pressed = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let closed_settings = escape_pressed && self.settings_open;
        if closed_settings {
            self.settings_open = false;
            self.sync_player_window_visibility();
        }
        self.handle_help_shortcuts(&ctx);
        if !ctx.egui_wants_keyboard_input() {
            let (home, explore, history, queue, library) = ctx.input(|input| {
                let alt = input.modifiers.alt;
                (
                    alt && input.key_pressed(egui::Key::Num1),
                    alt && input.key_pressed(egui::Key::Num2),
                    alt && input.key_pressed(egui::Key::Num3),
                    alt && input.key_pressed(egui::Key::Num4),
                    alt && input.key_pressed(egui::Key::Num5),
                )
            });
            if home {
                self.navigate_from_shortcut(&ctx, BrowseSection::Home);
            } else if explore {
                self.navigate_from_shortcut(&ctx, BrowseSection::Explore);
            } else if history {
                self.navigate_from_shortcut(&ctx, BrowseSection::History);
            } else if queue {
                self.navigate_from_shortcut(&ctx, BrowseSection::Queue);
            } else if library {
                self.navigate_from_shortcut(&ctx, BrowseSection::Library);
            }
        }
        if self.selected.is_some() && !ctx.egui_wants_keyboard_input() {
            let (toggle, back, forward, mute, theater, fullscreen) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::Space) || input.key_pressed(egui::Key::K),
                    input.key_pressed(egui::Key::ArrowLeft),
                    input.key_pressed(egui::Key::ArrowRight),
                    input.key_pressed(egui::Key::M),
                    input.key_pressed(egui::Key::T),
                    input.key_pressed(egui::Key::F),
                )
            });
            #[cfg(windows)]
            {
                let mut key_seek_pos: Option<f64> = None;
                let mut key_flush_pause = false;
                if let Some(player) = &self.player {
                    let was_ended = player.property("eof-reached").as_deref() == Some("yes");
                    let was_paused = player.property("pause").as_deref() == Some("yes");
                    let cur_pos = player
                        .number("time-pos")
                        .filter(|&pos| pos >= 0.0)
                        .or_else(|| {
                            self.active_playback
                                .as_ref()
                                .map(|active| active.position_ms as f64 / 1000.0)
                        })
                        .unwrap_or(0.0);
                    let result = if toggle {
                        if was_ended {
                            player
                                .seek(0.0, "absolute+exact")
                                .and_then(|()| player.set("pause", "no"))
                        } else {
                            player.set("pause", if was_paused { "no" } else { "yes" })
                        }
                    } else if back {
                        player.seek(-10.0, "relative")
                    } else if forward {
                        player.seek(10.0, "relative")
                    } else if mute {
                        player.set(
                            "mute",
                            if player.property("mute").as_deref() == Some("yes") {
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
                            if toggle {
                                if was_ended {
                                    key_seek_pos = Some(0.0);
                                } else if !was_paused {
                                    key_flush_pause = true;
                                }
                            } else if back {
                                key_seek_pos = Some((cur_pos - 10.0).max(0.0));
                            } else if forward {
                                key_seek_pos = Some(cur_pos + 10.0);
                            }
                        }
                        Err(error) => self.message = error,
                    }
                }
                if let Some(pos) = key_seek_pos {
                    self.record_manual_seek(pos, None, Instant::now());
                } else if key_flush_pause {
                    self.flush_playback_history_at(Instant::now());
                }
            }
            #[cfg(not(windows))]
            let _ = (toggle, back, forward, mute);
            if theater && !self.fullscreen {
                self.toggle_theater_mode();
            }
            if fullscreen || (escape_pressed && !closed_settings && self.fullscreen) {
                self.toggle_fullscreen(&ctx);
            }
        }
        let viewport_width = ui.available_width();
        if !self.fullscreen {
            egui::Panel::top("header")
                .show_separator_line(false)
                .show(ui, |ui| {
                    self.header_ui(ui, &ctx);
                });
            if self.selected.is_none()
                && should_show_navigation_panel(
                    self.navigation_open,
                    self.fullscreen,
                    false,
                    self.theater_mode,
                )
            {
                let nav_width = layout::compute_youtube_sidebar_width(viewport_width);
                egui::Panel::left("navigation")
                    .exact_size(nav_width)
                    .show(ui, |ui| {
                        self.navigation_ui(ui, &ctx);
                    });
            } else if should_show_compact_navigation(
                viewport_width,
                self.navigation_open,
                self.fullscreen,
                self.theater_mode,
            ) {
                egui::Panel::left("compact_navigation")
                    .exact_size(COMPACT_NAVIGATION_WIDTH)
                    .show(ui, |ui| {
                        self.compact_navigation_ui(ui, &ctx);
                    });
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if self.fullscreen {
                let avail_w = ui.available_width();
                let avail_h = ui.available_height();
                self.watch_primary_ui(ui, &ctx, avail_w, avail_h, WatchLayoutMode::SideBySide);
                if self.pending_queue_advance {
                    self.pending_queue_advance = false;
                    let _ = self.advance_queue_with_frame(frame, &ctx);
                }
                if !self.message.is_empty() {
                    ui.add_space(6.0);
                    ui.label(&self.message);
                }
                return;
            }

            let chosen = if self.selected.is_some() {
                self.watch_page_ui(ui, &ctx)
            } else {
                self.browse_page_ui(ui, &ctx)
            };

            if let Some(video) = chosen {
                let _ = self.open_video(video, frame, &ctx);
            }
            if self.pending_queue_advance {
                self.pending_queue_advance = false;
                let _ = self.advance_queue_with_frame(frame, &ctx);
            }
        });
        if self.selected.is_some() && self.navigation_open && !self.fullscreen && !self.theater_mode
        {
            self.watch_navigation_drawer_ui(&ctx);
        }
        self.in_app_ui_pass = false;
        self.settings_window_ui(&ctx);
        self.shortcuts_window_ui(&ctx);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([480.0, 420.0]),
        ..Default::default()
    };
    fastframe_graphics::Graphics::new("FastTube").run_native(
        || options.clone(),
        || {
            Box::new(|creation| {
                let rendering = fastframe_text::detect();
                let mut app = FastTube::default();
                let mut fonts = fastframe_fonts::FontSetup::default().definitions();
                rendering.apply_to(&mut fonts);
                creation.egui_ctx.set_fonts(fonts);
                app.sync_theme_visuals(&creation.egui_ctx);
                egui_extras::install_image_loaders(&creation.egui_ctx);
                fastframe_icons::install::<Icon>(&creation.egui_ctx);
                Ok(Box::new(app))
            })
        },
    )
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
