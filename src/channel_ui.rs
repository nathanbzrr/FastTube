use crate::browse_ui::compute_uniform_visible_row_range;
use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};
use std::ops::Range;

pub(crate) fn channel_video_row_height(card_w: f32) -> f32 {
    (card_w * 9.0 / 16.0).round() + 8.0 + 78.0
}

pub(crate) fn channel_short_row_height(card_w: f32) -> f32 {
    (card_w * 16.0 / 9.0).round() + 8.0 + 64.0
}

pub(crate) fn channel_playlist_row_height(card_w: f32) -> f32 {
    (card_w * 9.0 / 16.0).round() + 8.0 + 86.0
}

pub(crate) fn channel_grid_visible_row_range(
    total_rows: usize,
    row_height: f32,
    row_gap: f32,
    grid_top: f32,
    clip_top: f32,
    clip_bottom: f32,
    overscan_rows: usize,
) -> (Range<usize>, f32, f32) {
    compute_uniform_visible_row_range(
        total_rows,
        row_height,
        row_gap,
        grid_top,
        clip_top,
        clip_bottom,
        overscan_rows,
    )
}

#[derive(Default)]
struct ChannelBannerDeferred {
    open_about_modal: bool,
    sub_action: ChannelSubscribeAction,
    notify_level_change: Option<(String, settings::ChannelNotifyLevel)>,
    local_only: bool,
    channel_id: Option<String>,
    display_title: String,
    handle: Option<String>,
    canonical_url: String,
    avatar_url: Option<String>,
    target: Option<ChannelTarget>,
}

pub(crate) fn step_carousel_offset(current: f32, delta: f32, max_offset: f32) -> f32 {
    let clamped_max = max_offset.max(0.0);
    (current + delta).clamp(0.0, clamped_max)
}

fn current_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn format_upcoming_countdown(start_unix: i64, now_unix: i64, pt: bool) -> String {
    let delta = start_unix.saturating_sub(now_unix);
    if delta <= 0 {
        return if pt {
            "Iniciando em breve".to_owned()
        } else {
            "Starting soon".to_owned()
        };
    }
    let days = delta / 86_400;
    let hours = (delta % 86_400) / 3_600;
    let mins = (delta % 3_600) / 60;
    let prefix = if pt { "Estreia em" } else { "Premieres in" };
    if days > 0 {
        format!("{prefix} {days}d {hours}h")
    } else if hours > 0 {
        format!("{prefix} {hours}h {mins}m")
    } else if mins > 0 {
        format!("{prefix} {mins}m")
    } else {
        format!("{prefix} <1m")
    }
}

pub(crate) fn compute_poll_choice_percentages(
    choices: &[crate::innertube::ChannelPollChoice],
    voted_idx: Option<usize>,
) -> Vec<f32> {
    if choices.is_empty() {
        return Vec::new();
    }
    let has_server_ratios = choices
        .iter()
        .any(|c| c.vote_ratio.is_some_and(|r| r.is_finite() && r > 0.0));
    if has_server_ratios {
        return choices
            .iter()
            .map(|c| c.vote_ratio.unwrap_or(0.0).clamp(0.0, 1.0))
            .collect();
    }
    match voted_idx {
        Some(selected) if selected < choices.len() => (0..choices.len())
            .map(|idx| if idx == selected { 1.0 } else { 0.0 })
            .collect(),
        _ => vec![0.0; choices.len()],
    }
}

pub(crate) fn format_release_meta_line(
    metadata: Option<&VideoMetadata>,
    pt: bool,
) -> Option<String> {
    let meta = metadata?;
    let mut parts: Vec<String> = Vec::new();
    let release_kind = meta
        .badges
        .iter()
        .find(|b| {
            let l = b.to_ascii_lowercase();
            l.contains("album")
                || l.contains("álbum")
                || l.contains("single")
                || l == "ep"
                || l.contains("release")
        })
        .cloned()
        .or_else(|| {
            meta.playlist_count
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|c| {
                    let digits: String = c.chars().filter(|ch| ch.is_ascii_digit()).collect();
                    if let Ok(n) = digits.parse::<u64>() {
                        if n <= 1 {
                            "Single".to_owned()
                        } else if n <= 6 {
                            "EP".to_owned()
                        } else if pt {
                            "Álbum".to_owned()
                        } else {
                            "Album".to_owned()
                        }
                    } else {
                        c.to_owned()
                    }
                })
        });
    if let Some(kind) = release_kind {
        parts.push(kind);
    }
    if let Some(year_or_time) = meta
        .published_time_text
        .as_deref()
        .or(meta.upload_date.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(year_or_time.to_owned());
    }
    if let Some(tracks) = meta
        .playlist_count
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && !parts.iter().any(|p| p.eq_ignore_ascii_case(tracks))
    {
        parts.push(tracks.to_owned());
    }
    (!parts.is_empty()).then(|| parts.join(" • "))
}

pub(crate) fn channel_page_has_videos(page: &ChannelPage) -> bool {
    page.has_videos_tab
        || page
            .videos
            .iter()
            .any(|item| !search::is_playlist_search_result(item))
}

pub(crate) fn channel_page_has_home(page: &ChannelPage, has_videos: bool) -> bool {
    page.featured_video.is_some()
        || !page.home_shelves.is_empty()
        || page.has_home_tab
        || page
            .tab_endpoints
            .iter()
            .any(|ep| ep.kind == crate::innertube::ChannelTabKind::Home)
        || (has_videos && page.tab_endpoints.is_empty())
}

pub(crate) fn active_extra_channel_tab_kind(
    endpoints: &[crate::innertube::ChannelTabEndpoint],
) -> Option<crate::innertube::ChannelTabKind> {
    extra_channel_tab_endpoints(endpoints)
        .into_iter()
        .find(|ep| ep.selected)
        .map(|ep| ep.kind.clone())
}

fn extract_playlist_id_from_url(url: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix("list=") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

#[derive(Default)]
struct ChannelTabsDeferred {
    close_search: bool,
    trigger_search: bool,
    switch_to_tab: Option<ChannelTab>,
    open_dynamic_tab: Option<crate::innertube::ChannelTabEndpoint>,
}

#[derive(Default)]
struct ChannelVideosDeferred {
    requested_sort: Option<search::ChannelVideosSort>,
    selected_sort_chip_continuation: Option<String>,
    cancel_more_clicked: bool,
    load_more_clicked: bool,
    load_more_continuation_clicked: bool,
}

#[derive(Default)]
pub(crate) struct ChannelHomeDeferred {
    enqueue_all: Vec<Video>,
    open_playlist: Option<search::ChannelPlaylist>,
    videos_fallback: ChannelVideosDeferred,
}

#[derive(Default)]
struct ChannelAboutDeferred {
    copy_canonical_url: Option<String>,
    cancel_about: bool,
    reload_about: bool,
}

fn channel_tab_button_ui(
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
    let resp = ui
        .add(
            egui::Button::new(text)
                .frame(false)
                .fill(Color32::TRANSPARENT)
                .min_size(egui::vec2(0.0, 40.0)),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    paint_focus_ring_if_focused(ui, &resp, 6.0);
    resp
}

/// Filters `page.tab_endpoints` to extra dynamic channel tabs not already rendered by the
/// core tab bar (`Home`, `Videos`, `Shorts`, `Live`, `Playlists`, `Search`), deduplicating
/// entries while preserving YouTube's tab order.
pub(crate) fn extra_channel_tab_endpoints(
    endpoints: &[crate::innertube::ChannelTabEndpoint],
) -> Vec<&crate::innertube::ChannelTabEndpoint> {
    use crate::innertube::ChannelTabKind;
    use std::collections::HashSet;

    let mut out = Vec::new();
    let mut seen_kinds: HashSet<ChannelTabKind> = HashSet::new();
    let mut seen_other_titles: HashSet<String> = HashSet::new();

    for ep in endpoints {
        let title = ep.title.trim();
        let browse_id = ep.browse_id.trim();
        if title.is_empty() || browse_id.is_empty() {
            continue;
        }
        match &ep.kind {
            ChannelTabKind::Home
            | ChannelTabKind::Videos
            | ChannelTabKind::Shorts
            | ChannelTabKind::Live
            | ChannelTabKind::Playlists
            | ChannelTabKind::Search => continue,
            ChannelTabKind::Other(_) => {
                let norm = title.to_lowercase();
                if norm == "about" || norm == "sobre" {
                    continue;
                }
                if seen_other_titles.insert(norm) {
                    out.push(ep);
                }
            }
            known_extra => {
                if seen_kinds.insert(known_extra.clone()) {
                    out.push(ep);
                }
            }
        }
    }
    out
}

pub(crate) fn channel_dynamic_tab_label(
    endpoint: &crate::innertube::ChannelTabEndpoint,
    pt: bool,
) -> &str {
    use crate::innertube::ChannelTabKind;

    let trimmed = endpoint.title.trim();
    if !trimmed.is_empty() {
        return trimmed;
    }
    match &endpoint.kind {
        ChannelTabKind::Posts => {
            if pt {
                "Postagens"
            } else {
                "Posts"
            }
        }
        ChannelTabKind::Podcasts => "Podcasts",
        ChannelTabKind::Releases => {
            if pt {
                "Lan\u{00e7}amentos"
            } else {
                "Releases"
            }
        }
        ChannelTabKind::Store => {
            if pt {
                "Loja"
            } else {
                "Store"
            }
        }
        ChannelTabKind::Courses => {
            if pt {
                "Cursos"
            } else {
                "Courses"
            }
        }
        ChannelTabKind::Other(name) => name.as_str(),
        _ => "",
    }
}

fn channel_sort_chips_bar_ui(
    ui: &mut egui::Ui,
    chips: &[crate::innertube::ChannelSortChip],
    palette: &ThemePalette,
) -> Option<String> {
    let mut selected_continuation = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for chip in chips {
            let label = chip.title.trim();
            if label.is_empty() {
                continue;
            }
            let (fill, fg) = if chip.selected {
                (palette.active_button_bg, palette.active_button_fg)
            } else {
                (palette.header_button_bg, palette.header_button_fg)
            };
            let btn = egui::Button::new(RichText::new(label).size(13.0).strong().color(fg))
                .fill(fill)
                .stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(0.0, 32.0))
                .corner_radius(8);
            let chip_resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
            paint_focus_ring_if_focused(ui, &chip_resp, 8.0);
            if chip_resp.clicked() && !chip.selected && !chip.continuation.trim().is_empty() {
                selected_continuation = Some(chip.continuation.clone());
            }
        }
    });
    selected_continuation
}

fn channel_continuation_footer_ui(
    ui: &mut egui::Ui,
    card_max_w: f32,
    loading_more: bool,
    pt: bool,
    palette: &ThemePalette,
) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.set_max_width(card_max_w);
        if loading_more {
            ui.horizontal(|ui| {
                skeleton::visible_spinner(ui);
                ui.add_space(6.0);
                ui.label(
                    RichText::new(if pt {
                        "Carregando mais..."
                    } else {
                        "Loading more..."
                    })
                    .size(13.0)
                    .color(palette.neutral_status_fg),
                );
            });
        } else {
            let btn = egui::Button::new(
                RichText::new(if pt { "Carregar mais" } else { "Load more" })
                    .size(13.5)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .min_size(egui::vec2(144.0, 36.0))
            .corner_radius(18);
            let load_resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
            paint_focus_ring_if_focused(ui, &load_resp, 18.0);
            if load_resp.clicked() {
                clicked = true;
            }
        }
    });
    clicked
}

fn channel_empty_state_card_ui(
    ui: &mut egui::Ui,
    card_max_w: f32,
    message: &str,
    palette: ThemePalette,
) {
    egui::Frame::new()
        .fill(palette.card_alt_bg)
        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_min_width(card_max_w);
            ui.set_max_width(card_max_w);
            ui.label(
                RichText::new(message)
                    .size(13.5)
                    .color(palette.neutral_status_fg),
            );
        });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ChannelSubscribeAction {
    #[default]
    None,
    TogglePrimarySubscription,
    ToggleLocalFollow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChannelSubscribeMenuLabels {
    local_follow_toggle: &'static str,
    unsubscribe_item: &'static str,
    confirm_prompt: &'static str,
    confirm_button: &'static str,
    cancel_button: &'static str,
}

fn channel_subscribe_menu_labels(
    local_only: bool,
    is_local_followed: bool,
    pt: bool,
) -> ChannelSubscribeMenuLabels {
    let local_follow_toggle = if is_local_followed {
        if pt {
            "Seguindo localmente (RSS) ✓"
        } else {
            "Following locally (RSS) ✓"
        }
    } else if pt {
        "Seguir localmente (RSS)"
    } else {
        "Follow locally (RSS)"
    };
    let unsubscribe_item = if local_only {
        if pt { "Deixar de seguir" } else { "Unfollow" }
    } else if pt {
        "Cancelar inscrição"
    } else {
        "Unsubscribe"
    };
    let confirm_prompt = if local_only {
        if pt {
            "Confirmar deixar de seguir?"
        } else {
            "Confirm unfollow?"
        }
    } else if pt {
        "Confirmar cancelamento da inscrição?"
    } else {
        "Confirm unsubscribe?"
    };
    let confirm_button = if local_only {
        if pt { "Confirmar" } else { "Confirm" }
    } else if pt {
        "Cancelar inscrição"
    } else {
        "Unsubscribe"
    };
    let cancel_button = if pt { "Manter" } else { "Cancel" };
    ChannelSubscribeMenuLabels {
        local_follow_toggle,
        unsubscribe_item,
        confirm_prompt,
        confirm_button,
        cancel_button,
    }
}

impl FastTube {
    pub(crate) fn channel_header_toolbar_ui(
        &self,
        ui: &mut egui::Ui,
        retry_target: Option<&ChannelTarget>,
        loading: bool,
        compact: bool,
        current_limit: usize,
        palette: ThemePalette,
    ) -> ChannelHeaderActions {
        let mut actions = ChannelHeaderActions::default();
        let _ = current_limit;
        if !compact && retry_target.is_none() {
            return actions;
        }
        let pt = self.ui_pt();
        ui.horizontal(|ui| {
            if compact {
                let back_btn = egui::Button::new(
                    RichText::new(if pt { "← Voltar" } else { "← Back" })
                        .size(12.5)
                        .color(palette.header_button_fg),
                )
                .fill(palette.header_button_bg)
                .stroke(egui::Stroke::NONE)
                .corner_radius(16);
                let back_resp = ui
                    .add(back_btn)
                    .on_hover_text(if pt {
                        "Fechar p\u{00e1}gina do canal e retornar"
                    } else {
                        "Close channel view and return"
                    })
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                paint_focus_ring_if_focused(ui, &back_resp, 16.0);
                if back_resp.clicked() {
                    actions.back_to_results = true;
                }
            }
            if let Some(target) = retry_target {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let refresh_label = if compact {
                        if pt { "↻ Atualizar" } else { "↻ Refresh" }
                    } else {
                        "↻"
                    };
                    let refresh_btn = egui::Button::new(
                        RichText::new(refresh_label)
                            .size(if compact { 12.5 } else { 14.0 })
                            .color(palette.neutral_status_fg),
                    )
                    .fill(if compact {
                        palette.header_button_bg
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(16);
                    let refresh_resp = ui
                        .add_enabled(!loading, refresh_btn)
                        .on_hover_text(if pt {
                            "Recarregar do YouTube ignorando o cache local"
                        } else {
                            "Reload from YouTube bypassing local cache"
                        })
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    paint_focus_ring_if_focused(ui, &refresh_resp, 16.0);
                    if refresh_resp.clicked() {
                        actions.reload_target = Some(target.clone());
                        actions.force_refresh = true;
                    }
                });
            }
        });
        if compact {
            ui.add_space(6.0);
        }
        actions
    }

    pub(crate) fn handle_channel_card_actions(
        &mut self,
        ctx: &egui::Context,
        card_action: &SearchCardAction,
        chosen: &mut Option<Video>,
        video: &Video,
    ) {
        if card_action.open_clicked {
            *chosen = Some(video.clone());
        }
        if card_action.enqueue_clicked && self.enqueue_video(video.clone()).is_ok() {
            self.message = self.pick("Adicionado à fila.", "Added to queue.").into();
        }
        if let Some(fmt) = card_action.download_format {
            self.start_download_video(video.clone(), fmt, ctx);
        }
        if card_action.toggle_watch_later {
            self.library.data.toggle_watch_later(video.clone());
            self.save_library();
            let notice = self
                .pick(
                    "Lista \"Assistir mais tarde\" atualizada.",
                    "\"Watch later\" list updated.",
                )
                .to_owned();
            self.message = notice.clone();
            self.toasts.success(notice);
        }
        if card_action.open_save_modal {
            self.feed.search_save_modal_video = Some(video.clone());
            if self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
                && !self.auth.remote_playlists_task.is_running()
            {
                self.start_fetch_remote_playlists(ctx);
            }
        }
        if card_action.share_clicked {
            self.open_share_modal(video);
        }
        if card_action.open_report_modal {
            self.feed.search_report_reason = None;
            self.feed.search_report_details.clear();
            self.feed.search_report_modal_video = Some(video.clone());
        }
    }

    pub(crate) fn channel_card_menu_ui(
        &self,
        ui: &mut egui::Ui,
        video: &Video,
        palette: &ThemePalette,
        action: &mut SearchCardAction,
    ) {
        let pt = self.ui_pt();
        let more_resp = paint_more_dots_button(ui, palette);
        egui::Popup::menu(&more_resp).show(|ui| {
            let in_watch_later = self.library.data.is_in_watch_later(&video.url);
            ui.set_min_width(240.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Queue,
                if pt {
                    "Adicionar à fila"
                } else {
                    "Add to queue"
                },
                palette,
            )
            .clicked()
            {
                action.enqueue_clicked = true;
                ui.close();
            }
            let wl_label = if in_watch_later {
                if pt {
                    "Remover de Assistir mais tarde"
                } else {
                    "Remove from Watch later"
                }
            } else if pt {
                "Salvar em Assistir mais tarde"
            } else {
                "Save to Watch later"
            };
            if search_menu_item_ui(ui, SearchMenuIcon::Clock, wl_label, palette).clicked() {
                action.toggle_watch_later = true;
                ui.close();
            }
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Bookmark,
                if pt {
                    "Salvar na playlist"
                } else {
                    "Save to playlist"
                },
                palette,
            )
            .clicked()
            {
                action.open_save_modal = true;
                ui.close();
            }
            ui.menu_button(if pt { "⬇ Baixar" } else { "⬇ Download" }, |ui| {
                for fmt in downloads::DownloadFormat::ALL {
                    let lbl = if pt { fmt.label_pt() } else { fmt.label_en() };
                    if ui.button(lbl).clicked() {
                        action.download_format = Some(fmt);
                        ui.close();
                    }
                }
            });
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Share,
                if pt { "Compartilhar" } else { "Share" },
                palette,
            )
            .clicked()
            {
                action.share_clicked = true;
                ui.close();
            }
            ui.separator();
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Flag,
                if pt { "Denunciar" } else { "Report" },
                palette,
            )
            .clicked()
            {
                action.open_report_modal = true;
                ui.close();
            }
        });
    }

    pub(crate) fn channel_grid_video_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        width: f32,
        is_playing: bool,
        palette: &ThemePalette,
    ) -> SearchCardAction {
        let mut action = SearchCardAction::default();
        let pt = self.ui_pt();
        let upcoming_start_unix = self.upcoming_start_unix_for(&video.url);
        let (is_live, is_upcoming, is_music, subline, upcoming_fallback_text) = {
            let metadata = self.metadata_for(&video.url);
            let is_upcoming_badge = metadata.is_some_and(|m| {
                m.badges.iter().any(|b| {
                    b.eq_ignore_ascii_case("upcoming")
                        || b.eq_ignore_ascii_case("premiere")
                        || b.eq_ignore_ascii_case("estreia")
                })
            });
            let is_upcoming = upcoming_start_unix.is_some() || is_upcoming_badge;
            let is_live = !is_upcoming
                && metadata.is_some_and(|m| {
                    m.is_live
                        || m.badges.iter().any(|b| {
                            b.eq_ignore_ascii_case("ao vivo") || b.eq_ignore_ascii_case("live")
                        })
                });
            let upcoming_fallback = metadata
                .and_then(|m| m.published_time_text.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            (
                is_live,
                is_upcoming,
                recommendations::is_music_video(video, metadata),
                format_youtube_card_subline(&video.duration, metadata),
                upcoming_fallback,
            )
        };
        let progress_ratio = self.youtube_search_progress_ratio(&video.url);
        let in_queue = self.queue.data.is_queued(&video.url);
        let notify_enabled = is_upcoming && self.channels.upcoming_notify_urls.contains(&video.url);
        let mut toggle_upcoming_notify = false;
        let thumb_h = (width * 9.0 / 16.0).round();

        let wash_fill_idx = ui.painter().add(egui::Shape::Noop);
        let wash_stroke_idx = ui.painter().add(egui::Shape::Noop);
        let card_resp = ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let thumb_resp = self.thumbnails.show_rounded(
                ui,
                ctx,
                &video.thumbnail,
                egui::vec2(width, thumb_h),
                12.0,
            );
            paint_focus_ring_if_focused(ui, &thumb_resp, 12.0);
            let hover_t = ctx.animate_bool_with_time(
                ui.make_persistent_id(("channel_card_hover", &video.url)),
                thumb_resp.hovered(),
                0.14,
            );
            paint_card_hover_highlight(ui, thumb_resp.rect, 12.0, hover_t, palette);
            let overlay_dur = if is_live || is_upcoming {
                ""
            } else {
                video.duration.as_str()
            };
            paint_thumbnail_overlays(ui, thumb_resp.rect, overlay_dur, progress_ratio, is_music);
            if !is_live && !is_upcoming {
                crate::browse_ui::update_and_paint_card_hover_scrub(
                    ui,
                    ctx,
                    &video.url,
                    thumb_resp.rect,
                    thumb_resp.hovered(),
                    overlay_dur,
                );
            }
            if is_upcoming {
                let badge_text = if pt { "ESTREIA" } else { "UPCOMING" };
                let badge_w = if pt { 64.0 } else { 76.0 };
                let badge_rect = egui::Rect::from_min_size(
                    egui::pos2(thumb_resp.rect.left() + 8.0, thumb_resp.rect.top() + 8.0),
                    egui::vec2(badge_w, 20.0),
                );
                ui.painter().rect_filled(
                    badge_rect,
                    4.0,
                    Color32::from_rgba_unmultiplied(15, 15, 15, 224),
                );
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    badge_text,
                    egui::FontId::proportional(10.5),
                    Color32::WHITE,
                );
            }
            if in_queue {
                paint_queued_badge(ui, thumb_resp.rect, is_upcoming);
            }
            if thumb_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action.open_clicked = true;
            }

            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let title_w = (width - 28.0).max(60.0);
                ui.vertical(|ui| {
                    ui.set_width(title_w);
                    ui.set_max_width(title_w);
                    if is_playing {
                        ui.label(
                            RichText::new(if pt {
                                "▶ Em reprodução"
                            } else {
                                "▶ Now playing"
                            })
                            .small()
                            .strong()
                            .color(palette.playing_badge_fg),
                        );
                    }
                    let title_galley = layout_two_line_clamped_galley(
                        ui,
                        &video.title,
                        title_w,
                        egui::FontId::proportional(14.5),
                        palette.channel_fg,
                    );
                    let (title_rect, title_resp) = ui.allocate_exact_size(
                        egui::vec2(title_w, title_galley.size().y.max(18.0)),
                        egui::Sense::click(),
                    );
                    ui.painter()
                        .galley(title_rect.min, title_galley, palette.channel_fg);
                    paint_focus_ring_if_focused(ui, &title_resp, 6.0);
                    if title_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(&video.title)
                        .clicked()
                    {
                        action.open_clicked = true;
                    }
                    if is_live {
                        ui.add_space(2.0);
                        paint_live_now_badge_localized(ui, pt);
                    } else if is_upcoming {
                        ui.add_space(3.0);
                        let countdown_label = if let Some(start_unix) = upcoming_start_unix {
                            ctx.request_repaint_after(Duration::from_secs(30));
                            format_upcoming_countdown(start_unix, current_unix_secs(), pt)
                        } else {
                            upcoming_fallback_text.clone().unwrap_or_else(|| {
                                if pt {
                                    "Estreia agendada".to_owned()
                                } else {
                                    "Scheduled premiere".to_owned()
                                }
                            })
                        };
                        ui.label(
                            RichText::new(countdown_label)
                                .size(12.0)
                                .strong()
                                .color(palette.info_accent_fg),
                        );
                        ui.add_space(4.0);
                        let pill_label = if notify_enabled {
                            if pt {
                                "Notificação ativada ✓"
                            } else {
                                "Notification on ✓"
                            }
                        } else if pt {
                            "Receber notificação"
                        } else {
                            "Notify me"
                        };
                        let pill_w =
                            (pill_label.chars().count() as f32 * 6.4 + 34.0).clamp(108.0, title_w);
                        let (bell_rect, bell_resp) =
                            ui.allocate_exact_size(egui::vec2(pill_w, 26.0), egui::Sense::click());
                        let (pill_bg, pill_fg) = if notify_enabled {
                            (palette.active_button_bg, palette.active_button_fg)
                        } else if bell_resp.hovered() {
                            (palette.card_alt_bg, palette.header_button_fg)
                        } else {
                            (palette.header_button_bg, palette.header_button_fg)
                        };
                        ui.painter().rect_filled(bell_rect, 13.0, pill_bg);
                        ui.painter().rect_stroke(
                            bell_rect,
                            13.0,
                            egui::Stroke::new(1.0, palette.card_alt_stroke),
                            egui::StrokeKind::Inside,
                        );
                        paint_channel_notify_bell_icon(
                            ui.painter(),
                            egui::pos2(bell_rect.left() + 14.0, bell_rect.center().y),
                            pill_fg,
                            if notify_enabled {
                                settings::ChannelNotifyLevel::All
                            } else {
                                settings::ChannelNotifyLevel::Personalized
                            },
                        );
                        ui.painter().text(
                            egui::pos2(bell_rect.left() + 26.0, bell_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            pill_label,
                            egui::FontId::proportional(11.5),
                            pill_fg,
                        );
                        paint_focus_ring_if_focused(ui, &bell_resp, 13.0);
                        if bell_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            toggle_upcoming_notify = true;
                        }
                    }
                    if let Some(subline) = subline.as_deref() {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(subline)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    self.channel_card_menu_ui(ui, video, palette, &mut action);
                });
            });
        });

        let full_card_hovered = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| card_resp.response.rect.contains(pos));
        let card_wash_t = ctx.animate_bool_with_time(
            ui.make_persistent_id(("channel_card_wash", &video.url)),
            full_card_hovered,
            0.14,
        );
        let (wash_fill, wash_stroke) =
            card_hover_wash_shapes(card_resp.response.rect, 12.0, card_wash_t, palette);
        ui.painter().set(wash_fill_idx, wash_fill);
        ui.painter().set(wash_stroke_idx, wash_stroke);

        if toggle_upcoming_notify {
            self.toggle_upcoming_video_notify(&video.url, &video.title, upcoming_start_unix);
        }

        action
    }

    pub(crate) fn channel_short_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        width: f32,
        palette: &ThemePalette,
    ) -> SearchCardAction {
        let mut action = SearchCardAction::default();
        let (is_novo, views_line) = {
            let meta = self.metadata_for(&video.url);
            let is_novo = recommendations::is_recent_upload(meta, None);
            let views_line = meta
                .and_then(|m| m.view_count)
                .map(recommendations::format_youtube_compact_views)
                .or_else(|| {
                    meta.and_then(|m| m.view_count_text.as_deref())
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(str::to_owned)
                });
            (is_novo, views_line)
        };
        let in_queue = self.queue.data.is_queued(&video.url);
        let card_h = (width * 16.0 / 9.0).round();

        let wash_fill_idx = ui.painter().add(egui::Shape::Noop);
        let wash_stroke_idx = ui.painter().add(egui::Shape::Noop);
        let card_resp = ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let thumb_resp = self.thumbnails.show_cover_rounded(
                ui,
                ctx,
                &video.thumbnail,
                egui::vec2(width, card_h),
                10.0,
            );
            paint_focus_ring_if_focused(ui, &thumb_resp, 10.0);
            let hover_t = ctx.animate_bool_with_time(
                ui.make_persistent_id(("channel_short_hover", &video.url)),
                thumb_resp.hovered(),
                0.14,
            );
            paint_card_hover_highlight(ui, thumb_resp.rect, 10.0, hover_t, palette);
            if is_novo {
                paint_short_novo_badge(ui, thumb_resp.rect);
            }
            if in_queue {
                paint_queued_badge(ui, thumb_resp.rect, is_novo);
            }
            if thumb_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action.open_clicked = true;
            }
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let title_w = (width - 28.0).max(56.0);
                ui.vertical(|ui| {
                    ui.set_width(title_w);
                    ui.set_max_width(title_w);
                    let title_galley = layout_two_line_clamped_galley(
                        ui,
                        &video.title,
                        title_w,
                        egui::FontId::proportional(14.0),
                        palette.channel_fg,
                    );
                    let (title_rect, title_resp) = ui.allocate_exact_size(
                        egui::vec2(title_w, title_galley.size().y.max(18.0)),
                        egui::Sense::click(),
                    );
                    ui.painter()
                        .galley(title_rect.min, title_galley, palette.channel_fg);
                    paint_focus_ring_if_focused(ui, &title_resp, 6.0);
                    let title_resp = title_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(&video.title);
                    if title_resp.clicked() {
                        action.open_clicked = true;
                    }
                    if let Some(sub) = views_line {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(sub)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    self.channel_card_menu_ui(ui, video, palette, &mut action);
                });
            });
        });

        let full_card_hovered = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| card_resp.response.rect.contains(pos));
        let card_wash_t = ctx.animate_bool_with_time(
            ui.make_persistent_id(("channel_short_wash", &video.url)),
            full_card_hovered,
            0.14,
        );
        let (wash_fill, wash_stroke) =
            card_hover_wash_shapes(card_resp.response.rect, 10.0, card_wash_t, palette);
        ui.painter().set(wash_fill_idx, wash_fill);
        ui.painter().set(wash_stroke_idx, wash_stroke);

        action
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn channel_subscribe_button_ui(
        ui: &mut egui::Ui,
        subscribed: Option<bool>,
        is_local_followed: bool,
        pending: bool,
        local_only: bool,
        channel_key: &str,
        pt: bool,
        palette: &ThemePalette,
    ) -> ChannelSubscribeAction {
        let is_sub = subscribed == Some(true);
        let fill = if is_sub {
            palette.header_button_bg
        } else {
            palette.active_button_bg
        };
        let fg = if is_sub {
            palette.header_button_fg
        } else {
            palette.active_button_fg
        };
        if !is_sub {
            let label = if local_only {
                if pt { "Seguir canal" } else { "Follow channel" }
            } else if pt {
                "Inscrever-se"
            } else {
                "Subscribe"
            };
            let mut action = ChannelSubscribeAction::None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let btn = egui::Button::new(RichText::new(label).size(14.0).strong().color(fg))
                    .fill(fill)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(114.0, 36.0))
                    .corner_radius(18);
                let sub_btn_resp = ui
                    .add_enabled(!pending, btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                paint_focus_ring_if_focused(ui, &sub_btn_resp, 18.0);
                if sub_btn_resp.clicked() {
                    action = ChannelSubscribeAction::TogglePrimarySubscription;
                }
                if !local_only {
                    let rss_label = if is_local_followed {
                        if pt {
                            "Seguindo (RSS) ✓"
                        } else {
                            "Following (RSS) ✓"
                        }
                    } else if pt {
                        "Seguir (RSS)"
                    } else {
                        "Follow (RSS)"
                    };
                    let rss_btn = egui::Button::new(
                        RichText::new(rss_label)
                            .size(13.0)
                            .color(palette.header_button_fg),
                    )
                    .fill(palette.header_button_bg)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(0.0, 36.0))
                    .corner_radius(18);
                    let rss_resp = ui
                        .add(rss_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(if pt {
                            "Acompanhar este canal localmente via RSS"
                        } else {
                            "Follow this channel locally via RSS"
                        });
                    paint_focus_ring_if_focused(ui, &rss_resp, 18.0);
                    if rss_resp.clicked() {
                        action = ChannelSubscribeAction::ToggleLocalFollow;
                    }
                }
            });
            return action;
        }
        let sense = if pending {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(132.0, 36.0), sense);
        let bg = if response.hovered() && !pending {
            palette.card_alt_bg
        } else {
            fill
        };
        ui.painter().rect_filled(rect, 18.0, bg);
        let icon_color = if pending {
            palette.neutral_status_fg
        } else {
            fg
        };
        paint_yt_icon(
            ui.painter(),
            egui::pos2(rect.left() + 22.0, rect.center().y),
            YtSidebarIcon::Bell,
            icon_color,
            false,
        );
        let active_label = if local_only {
            if pt { "Seguindo" } else { "Following" }
        } else if pt {
            "Inscrito"
        } else {
            "Subscribed"
        };
        ui.painter().text(
            egui::pos2(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            active_label,
            egui::FontId::proportional(14.0),
            icon_color,
        );
        paint_yt_icon(
            ui.painter(),
            egui::pos2(rect.right() - 18.0, rect.center().y),
            YtSidebarIcon::ChevronDown,
            icon_color,
            false,
        );
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        paint_focus_ring_if_focused(ui, &response, 18.0);
        let confirm_id = ui.make_persistent_id(("channel_unsub_confirm", channel_key));
        if pending {
            return ChannelSubscribeAction::None;
        }
        let labels = channel_subscribe_menu_labels(local_only, is_local_followed, pt);
        let mut action = ChannelSubscribeAction::None;
        let mut popup_open = false;
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                popup_open = true;
                ui.set_min_width(236.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                if !local_only {
                    if styled_menu_item_button(
                        ui,
                        labels.local_follow_toggle,
                        is_local_followed,
                        palette,
                    )
                    .clicked()
                    {
                        action = ChannelSubscribeAction::ToggleLocalFollow;
                        ui.ctx().data_mut(|d| d.remove::<bool>(confirm_id));
                        ui.close();
                    }
                    ui.separator();
                }
                let confirming = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(confirm_id))
                    .unwrap_or(false);
                if !confirming {
                    if styled_menu_item_button(ui, labels.unsubscribe_item, false, palette)
                        .clicked()
                    {
                        ui.ctx().data_mut(|d| d.insert_temp(confirm_id, true));
                    }
                } else {
                    ui.label(
                        RichText::new(labels.confirm_prompt)
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                    );
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let confirm_btn = egui::Button::new(
                            RichText::new(labels.confirm_button)
                                .size(13.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(204, 0, 0))
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(8)
                        .min_size(egui::vec2(0.0, 28.0));
                        if ui
                            .add(confirm_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            action = ChannelSubscribeAction::TogglePrimarySubscription;
                            ui.ctx().data_mut(|d| d.remove::<bool>(confirm_id));
                            ui.close();
                        }
                        if styled_compact_button(ui, labels.cancel_button, false, palette).clicked()
                        {
                            ui.ctx().data_mut(|d| d.remove::<bool>(confirm_id));
                        }
                    });
                }
            });
        if !popup_open {
            ui.ctx().data_mut(|d| d.remove::<bool>(confirm_id));
        }
        action
    }

    #[allow(dead_code)]
    pub(crate) fn channel_banner_header_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        compact: bool,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        let deferred =
            self.channel_banner_header_ui_deferred(ui, ctx, page, compact, card_max_w, palette);
        self.apply_channel_banner_deferred(ctx, deferred);
    }

    fn channel_banner_header_ui_deferred(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        compact: bool,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> ChannelBannerDeferred {
        let pt = self.ui_pt();
        let local_only = !self.auth.authenticated;
        let is_local_followed = self
            .followed_channels
            .data
            .is_following_target(Some(&page.target), page.channel_id.as_deref());
        let subscribed = if local_only {
            Some(is_local_followed)
        } else {
            self.subscription_status_for(Some(&page.target), page.channel_id.as_deref())
                .map(|s| s.subscription_id.is_some())
        };
        let sub_pending = !local_only && self.auth.sub_toggle_task.is_running();
        let mut sub_action = ChannelSubscribeAction::None;
        let mut notify_level_change: Option<(String, settings::ChannelNotifyLevel)> = None;
        let mut open_about_modal = false;
        let notify_channel_key = page
            .channel_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| match &page.target {
                ChannelTarget::ChannelId(id) => id.clone(),
                ChannelTarget::Handle(h) => h.clone(),
            });
        let current_notify_level = self
            .settings
            .prefs
            .value
            .per_channel_notifications
            .get(&notify_channel_key)
            .copied()
            .unwrap_or_default();
        let dark_theme = self.is_dark_theme();
        let link_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };

        if let Some(banner_url) = page
            .banner_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            let banner_height = if compact {
                86.0
            } else {
                (card_max_w * 172.0 / 1070.0).clamp(132.0, 198.0).round()
            };
            self.thumbnails.show_cover_rounded(
                ui,
                ctx,
                banner_url,
                egui::vec2(card_max_w, banner_height),
                12.0,
            );
            ui.add_space(if compact { 12.0 } else { 18.0 });
        }

        ui.scope(|ui| {
            ui.set_max_width(card_max_w);
            ui.horizontal_top(|ui| {
                let title = channel_page_display_title(page);
                let avatar_size = if compact { 68.0 } else { 160.0 };
                let _ = self.paint_avatar_or_initials(
                    ui,
                    ctx,
                    page.avatar_url.as_deref(),
                    title,
                    avatar_size,
                    false,
                );
                ui.add_space(if compact { 12.0 } else { 22.0 });
                let info_w =
                    (card_max_w - avatar_size - if compact { 16.0 } else { 28.0 }).max(140.0);
                ui.vertical(|ui| {
                    ui.set_width(info_w);
                    ui.set_max_width(info_w);
                    ui.add_space(if compact { 0.0 } else { 4.0 });

                    // Title + Verified check badge
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(
                            RichText::new(title)
                                .size(if compact { 20.0 } else { 36.0 })
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if page.verified == Some(true) {
                            let badge_d = if compact { 14.0 } else { 18.0 };
                            paint_verified_badge_sized(ui, &palette, badge_d);
                        }
                    });
                    ui.add_space(4.0);

                    // Handle · Subscribers · Videos line
                    let handle_str = page
                        .handle
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let subs_str = page
                        .subscriber_count_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| page.subscriber_count.map(format_subscribers_pt));
                    let real_video_count = page
                        .videos
                        .iter()
                        .filter(|item| !search::is_playlist_search_result(item))
                        .count();
                    let videos_str = page
                        .video_count_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| {
                            (real_video_count > 0).then(|| {
                                if real_video_count == 1 {
                                    if pt {
                                        "1 vídeo".to_owned()
                                    } else {
                                        "1 video".to_owned()
                                    }
                                } else if pt {
                                    format!("{real_video_count} vídeos")
                                } else {
                                    format!("{real_video_count} videos")
                                }
                            })
                        });
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let mut wrote_any = false;
                        if let Some(handle) = handle_str {
                            ui.label(
                                RichText::new(handle)
                                    .size(if compact { 12.5 } else { 14.0 })
                                    .color(palette.channel_fg),
                            );
                            wrote_any = true;
                        }
                        if let Some(subs) = subs_str {
                            if wrote_any {
                                ui.label(
                                    RichText::new(" · ")
                                        .size(if compact { 12.5 } else { 14.0 })
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            ui.label(
                                RichText::new(subs)
                                    .size(if compact { 12.5 } else { 14.0 })
                                    .color(palette.neutral_status_fg),
                            );
                            wrote_any = true;
                        }
                        if let Some(vids) = videos_str {
                            if wrote_any {
                                ui.label(
                                    RichText::new(" · ")
                                        .size(if compact { 12.5 } else { 14.0 })
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            ui.label(
                                RichText::new(vids)
                                    .size(if compact { 12.5 } else { 14.0 })
                                    .color(palette.neutral_status_fg),
                            );
                        }
                    });

                    // Description preview + "...mais"
                    let desc_preview = page
                        .description_preview
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| {
                            clean_description(page.description.as_deref()).and_then(|clean| {
                                description_snippet(Some(&clean), if compact { 60 } else { 78 })
                            })
                        });
                    if desc_preview.is_some()
                        || !page.links.is_empty()
                        || page.country.is_some()
                        || page.joined_date_text.is_some()
                    {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if let Some(snippet) = desc_preview {
                                ui.label(
                                    RichText::new(snippet)
                                        .size(if compact { 12.5 } else { 13.5 })
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(if pt { "...mais" } else { "...more" })
                                            .size(if compact { 12.5 } else { 13.5 })
                                            .strong()
                                            .color(palette.channel_fg),
                                    )
                                    .frame(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                open_about_modal = true;
                            }
                        });
                    }

                    // External links line (icon + blue primary link + "and N more links")
                    if let Some(first_link) = page.links.first() {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            paint_link_chain_icon(ui, palette.neutral_status_fg);
                            let link_label = if first_link.title.trim().is_empty() {
                                first_link.url.as_str()
                            } else {
                                first_link.title.as_str()
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(link_label).size(13.5).color(link_blue),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text(&first_link.url)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                open_external_url(&first_link.url);
                            }
                            if page.links.len() > 1 || page.more_links_label.is_some() {
                                let more_text =
                                    page.more_links_label.clone().unwrap_or_else(|| {
                                        if page.links.len() == 2 {
                                            if pt {
                                                "e mais 1 link".to_owned()
                                            } else {
                                                "and 1 more link".to_owned()
                                            }
                                        } else if pt {
                                            format!("e mais {} links", page.links.len() - 1)
                                        } else {
                                            format!("and {} more links", page.links.len() - 1)
                                        }
                                    });
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(more_text)
                                                .size(13.5)
                                                .strong()
                                                .color(palette.channel_fg),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    open_about_modal = true;
                                }
                            }
                        });
                    }

                    ui.add_space(if compact { 8.0 } else { 12.0 });
                    let is_own_channel = self.is_own_channel_page(page);
                    if is_own_channel {
                        let studio_base = page
                            .channel_id
                            .as_deref()
                            .filter(|id| !id.trim().is_empty())
                            .map(|id| format!("https://studio.youtube.com/channel/{id}"))
                            .unwrap_or_else(|| "https://studio.youtube.com".to_owned());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if crate::ui_icons::styled_pill_button(
                                ui,
                                if pt {
                                    "Personalizar o canal"
                                } else {
                                    "Customize channel"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                crate::open_external_url(&format!("{studio_base}/editing/images"));
                            }
                            if crate::ui_icons::styled_pill_button(
                                ui,
                                if pt {
                                    "Gerenciar vídeos"
                                } else {
                                    "Manage videos"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                crate::open_external_url(&format!("{studio_base}/videos"));
                            }
                        });
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            sub_action = Self::channel_subscribe_button_ui(
                                ui,
                                subscribed,
                                is_local_followed,
                                sub_pending,
                                local_only,
                                &page.canonical_url,
                                pt,
                                &palette,
                            );
                            if subscribed == Some(true) || is_local_followed {
                                let level_label = match current_notify_level {
                                    settings::ChannelNotifyLevel::All => {
                                        if pt {
                                            "Todas"
                                        } else {
                                            "All"
                                        }
                                    }
                                    settings::ChannelNotifyLevel::Personalized => {
                                        if pt {
                                            "Personalizadas"
                                        } else {
                                            "Personalized"
                                        }
                                    }
                                    settings::ChannelNotifyLevel::None => {
                                        if pt {
                                            "Nenhuma"
                                        } else {
                                            "None"
                                        }
                                    }
                                };
                                let pill_w = if compact { 58.0 } else { 150.0 };
                                let (bell_rect, bell_resp) = ui.allocate_exact_size(
                                    egui::vec2(pill_w, 36.0),
                                    egui::Sense::click(),
                                );
                                let bell_bg = if bell_resp.hovered() {
                                    palette.card_alt_bg
                                } else {
                                    palette.header_button_bg
                                };
                                ui.painter().rect_filled(bell_rect, 18.0, bell_bg);
                                paint_channel_notify_bell_icon(
                                    ui.painter(),
                                    egui::pos2(bell_rect.left() + 20.0, bell_rect.center().y),
                                    palette.header_button_fg,
                                    current_notify_level,
                                );
                                if !compact {
                                    ui.painter().text(
                                        egui::pos2(bell_rect.left() + 36.0, bell_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        level_label,
                                        egui::FontId::proportional(13.0),
                                        palette.header_button_fg,
                                    );
                                }
                                paint_yt_icon(
                                    ui.painter(),
                                    egui::pos2(bell_rect.right() - 16.0, bell_rect.center().y),
                                    YtSidebarIcon::ChevronDown,
                                    palette.header_button_fg,
                                    false,
                                );
                                let bell_resp = bell_resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(if pt {
                                        "Preferência de notificações deste canal"
                                    } else {
                                        "Channel notification preference"
                                    });
                                paint_focus_ring_if_focused(ui, &bell_resp, 18.0);
                                egui::Popup::menu(&bell_resp).show(|ui| {
                                    ui.set_min_width(188.0);
                                    for (lvl, pt_l, en_l) in [
                                        (settings::ChannelNotifyLevel::All, "Todas", "All"),
                                        (
                                            settings::ChannelNotifyLevel::Personalized,
                                            "Personalizadas",
                                            "Personalized",
                                        ),
                                        (settings::ChannelNotifyLevel::None, "Nenhuma", "None"),
                                    ] {
                                        if styled_menu_item_button(
                                            ui,
                                            if pt { pt_l } else { en_l },
                                            current_notify_level == lvl,
                                            &palette,
                                        )
                                        .clicked()
                                        {
                                            notify_level_change =
                                                Some((notify_channel_key.clone(), lvl));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });
                    }
                });
            });
        });

        ui.add_space(if compact { 6.0 } else { 10.0 });
        let needs_channel_fields = sub_action != ChannelSubscribeAction::None;
        ChannelBannerDeferred {
            open_about_modal,
            sub_action,
            notify_level_change,
            local_only,
            channel_id: if needs_channel_fields {
                page.channel_id.clone()
            } else {
                None
            },
            display_title: if needs_channel_fields {
                channel_page_display_title(page).to_owned()
            } else {
                String::new()
            },
            handle: if needs_channel_fields {
                page.handle.clone()
            } else {
                None
            },
            canonical_url: if needs_channel_fields {
                page.canonical_url.clone()
            } else {
                String::new()
            },
            avatar_url: if needs_channel_fields {
                page.avatar_url.clone()
            } else {
                None
            },
            target: if needs_channel_fields {
                Some(page.target.clone())
            } else {
                None
            },
        }
    }

    fn apply_channel_banner_deferred(
        &mut self,
        ctx: &egui::Context,
        deferred: ChannelBannerDeferred,
    ) {
        if deferred.open_about_modal {
            self.channels.about_modal_open = true;
            if !self.channels.about.loaded
                && !self.is_channel_about_loading()
                && self.channels.about.error.is_none()
            {
                self.start_channel_about(ctx);
            }
        }
        if let Some((cid, level)) = deferred.notify_level_change {
            self.settings
                .prefs
                .value
                .per_channel_notifications
                .insert(cid.clone(), level);
            self.set_channel_notification_level(&cid, level, ctx);
        }
        match deferred.sub_action {
            ChannelSubscribeAction::None => {}
            ChannelSubscribeAction::ToggleLocalFollow => {
                let channel_id = deferred.channel_id.or_else(|| {
                    deferred.target.as_ref().map(|t| match t {
                        ChannelTarget::ChannelId(id) => id.clone(),
                        ChannelTarget::Handle(h) => h.clone(),
                    })
                });
                if let Some(cid) = channel_id {
                    self.toggle_local_follow_channel(
                        cid,
                        deferred.display_title,
                        deferred.handle,
                        deferred.canonical_url,
                        deferred.avatar_url,
                    );
                }
            }
            ChannelSubscribeAction::TogglePrimarySubscription => {
                if deferred.local_only {
                    let channel_id = deferred.channel_id.or_else(|| {
                        deferred.target.as_ref().map(|t| match t {
                            ChannelTarget::ChannelId(id) => id.clone(),
                            ChannelTarget::Handle(h) => h.clone(),
                        })
                    });
                    if let Some(cid) = channel_id {
                        self.toggle_local_follow_channel(
                            cid,
                            deferred.display_title,
                            deferred.handle,
                            deferred.canonical_url,
                            deferred.avatar_url,
                        );
                    }
                } else if let Some(target) = deferred.target {
                    self.start_toggle_subscription(target, deferred.channel_id, ctx);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn channel_tabs_bar_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        from_cache: bool,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        let deferred =
            self.channel_tabs_bar_ui_deferred(ui, ctx, page, from_cache, card_max_w, palette);
        self.apply_channel_tabs_deferred(ctx, deferred);
    }

    fn channel_tabs_bar_ui_deferred(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        from_cache: bool,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> ChannelTabsDeferred {
        let mut switch_to_tab = None;
        let mut open_dynamic_tab = None;
        let mut trigger_search = false;
        let mut close_search = false;

        let is_own_channel = self.is_own_channel_page(page);
        let has_ep_shorts = page
            .tab_endpoints
            .iter()
            .any(|ep| ep.kind == crate::innertube::ChannelTabKind::Shorts);
        let has_ep_live = page
            .tab_endpoints
            .iter()
            .any(|ep| ep.kind == crate::innertube::ChannelTabKind::Live);
        let has_ep_playlists = page
            .tab_endpoints
            .iter()
            .any(|ep| ep.kind == crate::innertube::ChannelTabKind::Playlists);
        let extra_tabs = extra_channel_tab_endpoints(&page.tab_endpoints);
        let any_extra_selected = extra_tabs.iter().any(|ep| ep.selected);

        let has_videos =
            channel_page_has_videos(page) || !self.channels.videos.sorted_items.is_empty();
        let has_tab_flags = page.featured_video.is_some()
            || !page.home_shelves.is_empty()
            || page.has_shorts_tab
            || page.has_streams_tab
            || page.has_playlists_tab
            || !page.tab_endpoints.is_empty();
        let show_videos = has_videos;
        let show_shorts = !has_tab_flags
            || page.has_shorts_tab
            || has_ep_shorts
            || !self.channels.shorts.items.is_empty()
            || self.channels.tab == ChannelTab::Shorts;
        let show_streams = !has_tab_flags
            || page.has_streams_tab
            || has_ep_live
            || !self.channels.streams.items.is_empty()
            || self.channels.tab == ChannelTab::Streams;
        let show_playlists = !has_tab_flags
            || page.has_playlists_tab
            || has_ep_playlists
            || !page.initial_playlists.is_empty()
            || !self.channels.playlists.items.is_empty()
            || self.channels.tab == ChannelTab::Playlists
            || (is_own_channel
                && (self.auth.authenticated || self.auth.youtube_web_session.is_some()));
        let show_home = channel_page_has_home(page, has_videos)
            || (!show_videos
                && !show_shorts
                && !show_streams
                && !show_playlists
                && extra_tabs.is_empty());

        let pt = self.ui_pt();
        let mut active_tab_rect: Option<egui::Rect> = None;
        ui.scope(|ui| {
            ui.set_max_width(card_max_w);
            egui::ScrollArea::horizontal()
                .id_salt("channel_tabs_bar_scroll")
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 22.0;
                        if show_home {
                            let home_selected =
                                !any_extra_selected && self.channels.tab == ChannelTab::Home;
                            let home_resp = channel_tab_button_ui(
                                ui,
                                if pt { "In\u{00ed}cio" } else { "Home" },
                                home_selected,
                                &palette,
                            );
                            if home_selected {
                                active_tab_rect = Some(home_resp.rect);
                            }
                            if home_resp.clicked() {
                                switch_to_tab = Some(ChannelTab::Home);
                            }
                        }

                        if show_videos {
                            let videos_selected =
                                !any_extra_selected && self.channels.tab == ChannelTab::Videos;
                            let videos_resp = channel_tab_button_ui(
                                ui,
                                if pt { "V\u{00ed}deos" } else { "Videos" },
                                videos_selected,
                                &palette,
                            );
                            if videos_selected {
                                active_tab_rect = Some(videos_resp.rect);
                            }
                            if videos_resp.clicked() {
                                switch_to_tab = Some(ChannelTab::Videos);
                            }
                        }

                        if show_shorts {
                            let shorts_selected =
                                !any_extra_selected && self.channels.tab == ChannelTab::Shorts;
                            let shorts_resp =
                                channel_tab_button_ui(ui, "Shorts", shorts_selected, &palette);
                            if shorts_selected {
                                active_tab_rect = Some(shorts_resp.rect);
                            }
                            if shorts_resp.clicked() {
                                switch_to_tab = Some(ChannelTab::Shorts);
                            }
                        }
                        if show_streams {
                            let streams_selected =
                                !any_extra_selected && self.channels.tab == ChannelTab::Streams;
                            let streams_resp = channel_tab_button_ui(
                                ui,
                                if pt { "Ao vivo" } else { "Live" },
                                streams_selected,
                                &palette,
                            );
                            if streams_selected {
                                active_tab_rect = Some(streams_resp.rect);
                            }
                            if streams_resp.clicked() {
                                switch_to_tab = Some(ChannelTab::Streams);
                            }
                        }
                        if show_playlists {
                            let playlists_selected =
                                !any_extra_selected && self.channels.tab == ChannelTab::Playlists;
                            let playlists_resp = channel_tab_button_ui(
                                ui,
                                "Playlists",
                                playlists_selected,
                                &palette,
                            );
                            if playlists_selected {
                                active_tab_rect = Some(playlists_resp.rect);
                            }
                            if playlists_resp.clicked() {
                                switch_to_tab = Some(ChannelTab::Playlists);
                            }
                        }

                        // Dynamically rendered extra InnerTube channel tabs (Posts, Podcasts, Releases, Courses, etc.)
                        for ep in &extra_tabs {
                            let label = channel_dynamic_tab_label(ep, pt);
                            let ep_resp = channel_tab_button_ui(ui, label, ep.selected, &palette);
                            if ep.selected {
                                active_tab_rect = Some(ep_resp.rect);
                            }
                            if ep_resp.clicked() {
                                open_dynamic_tab = Some((*ep).clone());
                            }
                        }

                        if self.channels.tab == ChannelTab::About && !any_extra_selected {
                            let about_resp = channel_tab_button_ui(
                                ui,
                                if pt { "Sobre" } else { "About" },
                                true,
                                &palette,
                            );
                            active_tab_rect = Some(about_resp.rect);
                            if about_resp.clicked() {
                                switch_to_tab = Some(ChannelTab::About);
                            }
                        }

                        // Inline search toggle + expandable input
                        let search_btn = paint_channel_search_icon_btn(
                            ui,
                            self.channels.search.input_open
                                || (!any_extra_selected && self.channels.tab == ChannelTab::Search),
                            &palette,
                        );
                        if !any_extra_selected && self.channels.tab == ChannelTab::Search {
                            active_tab_rect = Some(search_btn.rect);
                        }
                        if search_btn
                            .on_hover_text(if pt {
                                "Pesquisar neste canal"
                            } else {
                                "Search this channel"
                            })
                            .clicked()
                        {
                            if self.channels.search.input_open
                                && !self.channels.search.query.trim().is_empty()
                            {
                                trigger_search = true;
                            } else {
                                self.channels.search.input_open = !self.channels.search.input_open;
                            }
                        }

                        if self.channels.search.input_open
                            || self.channels.tab == ChannelTab::Search
                        {
                            let input_id = ui.make_persistent_id("channel_inline_search_input");
                            let edit = egui::TextEdit::singleline(&mut self.channels.search.query)
                                .id(input_id)
                                .hint_text(if pt { "Pesquisar" } else { "Search" })
                                .desired_width(180.0);
                            let resp = ui.add(edit);
                            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                trigger_search = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("✕")
                                            .size(13.0)
                                            .color(palette.neutral_status_fg),
                                    )
                                    .frame(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                close_search = true;
                            }
                        }

                        let _ = from_cache;
                    });
                });
            ui.add_space(-2.0);
            let sep_rect = ui.available_rect_before_wrap();
            let sep_y = sep_rect.top();
            ui.painter().line_segment(
                [
                    egui::pos2(sep_rect.left(), sep_y),
                    egui::pos2(sep_rect.left() + card_max_w, sep_y),
                ],
                egui::Stroke::new(1.0, palette.card_alt_stroke),
            );
            if let Some(tab_rect) = active_tab_rect {
                let inset = if self.channels.tab == ChannelTab::Search && !any_extra_selected {
                    4.0
                } else {
                    6.0
                };
                let bar_left = sep_rect.left();
                let bar_right = bar_left + card_max_w;
                let target_x = (tab_rect.left() + inset).clamp(bar_left, bar_right);
                let target_right = (tab_rect.right() - inset).clamp(target_x, bar_right);
                let target_w = (target_right - target_x)
                    .max(16.0)
                    .min(bar_right - target_x);
                let anim_x = ctx.animate_value_with_time(
                    egui::Id::new("channel_tab_indicator_x"),
                    target_x,
                    0.16,
                );
                let anim_w = ctx.animate_value_with_time(
                    egui::Id::new("channel_tab_indicator_w"),
                    target_w,
                    0.16,
                );
                let indicator_rect = egui::Rect::from_min_size(
                    egui::pos2(anim_x, sep_y - 2.5),
                    egui::vec2(anim_w, 2.5),
                );
                ui.painter()
                    .rect_filled(indicator_rect, 1.25, palette.header_button_fg);
            }
            ui.add_space(16.0);
        });

        ChannelTabsDeferred {
            close_search,
            trigger_search,
            switch_to_tab,
            open_dynamic_tab,
        }
    }

    fn apply_channel_tabs_deferred(&mut self, ctx: &egui::Context, deferred: ChannelTabsDeferred) {
        if deferred.close_search {
            self.channels.search.input_open = false;
            self.channels.search.query.clear();
            self.channels.cancel_channel_search();
            if self.channels.tab == ChannelTab::Search {
                self.channels.tab = self
                    .channels
                    .page
                    .as_ref()
                    .map(|p| self.default_available_channel_tab_for_page(p))
                    .unwrap_or(ChannelTab::Home);
            }
        } else if deferred.trigger_search {
            let query = self.channels.search.query.clone();
            self.start_channel_search(&query, ctx);
        }

        if let Some(endpoint) = deferred.open_dynamic_tab {
            self.open_channel_dynamic_tab(endpoint, ctx);
            return;
        }

        if let Some(tab) = deferred.switch_to_tab {
            if let Some(page) = &mut self.channels.page {
                for ep in &mut page.tab_endpoints {
                    ep.selected = false;
                }
            }
            self.channels.tab = tab;
            match tab {
                ChannelTab::Home | ChannelTab::Videos | ChannelTab::Search => {}
                ChannelTab::Shorts => {
                    if !self.channels.shorts.loaded
                        && !self.channels.is_shorts_loading()
                        && self.channels.shorts.error.is_none()
                    {
                        self.start_channel_shorts(ctx);
                    }
                }
                ChannelTab::Streams => {
                    if !self.channels.streams.loaded
                        && !self.channels.is_streams_loading()
                        && self.channels.streams.error.is_none()
                    {
                        self.start_channel_streams(ctx);
                    }
                }
                ChannelTab::Playlists => {
                    if !self.channels.playlists.loaded
                        && !self.is_channel_playlists_loading()
                        && self.channels.playlists.error.is_none()
                    {
                        self.start_channel_playlists(ctx);
                    }
                }
                ChannelTab::About => {
                    if !self.channels.about.loaded
                        && !self.is_channel_about_loading()
                        && self.channels.about.error.is_none()
                    {
                        self.start_channel_about(ctx);
                    }
                }
            }
        }
    }

    pub(crate) fn channel_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
    ) -> Option<Video> {
        let palette = self.palette();
        let loading = self.is_channel_loading();
        let error = self.channels.error.clone();
        let has_page = self.channels.page.is_some();
        let retry_target = self.current_channel_target();
        let from_cache = self.channels.from_cache;
        let current_limit = self.channels.video_limit;

        let mut actions = self.channel_header_toolbar_ui(
            ui,
            retry_target.as_ref(),
            loading,
            compact,
            current_limit,
            palette,
        );

        let (cancel_initial, status_reload_target) = self.channel_load_status_ui(
            ui,
            loading,
            error.as_deref(),
            retry_target.as_ref(),
            palette,
        );
        if status_reload_target.is_some() {
            actions.reload_target = status_reload_target;
        }

        let mut chosen = None;
        if has_page {
            let max_container = if compact {
                ui.available_width()
            } else {
                max_card_width
                    .map(|limit| ui.available_width().min(limit).min(1284.0))
                    .unwrap_or_else(|| ui.available_width().min(1284.0))
            };
            self.thumbnails.begin_pass();
            egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_channel_page"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let avail = ui.available_width();
                    let card_max_w = max_container.min(avail).max(200.0);
                    let side_pad = if !compact && avail > card_max_w {
                        ((avail - card_max_w) * 0.5).floor()
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        if side_pad > 0.0 {
                            ui.add_space(side_pad);
                        }
                        ui.vertical(|ui| {
                            ui.set_width(card_max_w);
                            ui.set_max_width(card_max_w);
                            // Take ChannelPage temporarily to avoid cloning the full page every frame,
                            // restoring it before executing any deferred action that reads `self.channels.page`.
                            let (banner_deferred, tabs_deferred) =
                                if let Some(page) = self.channels.page.take() {
                                    let b = self.channel_banner_header_ui_deferred(
                                        ui, ctx, &page, compact, card_max_w, palette,
                                    );
                                    let t = self.channel_tabs_bar_ui_deferred(
                                        ui, ctx, &page, from_cache, card_max_w, palette,
                                    );
                                    self.channels.page = Some(page);
                                    (b, t)
                                } else {
                                    (
                                        ChannelBannerDeferred::default(),
                                        ChannelTabsDeferred::default(),
                                    )
                                };
                            self.apply_channel_banner_deferred(ctx, banner_deferred);
                            self.apply_channel_tabs_deferred(ctx, tabs_deferred);

                            let layout = ChannelTabLayout {
                                compact,
                                card_max_w,
                                palette,
                            };
                            let active_extra_tab = self.channels.page.as_ref().and_then(|page| {
                                active_extra_channel_tab_kind(&page.tab_endpoints)
                            });
                            match active_extra_tab {
                                Some(crate::innertube::ChannelTabKind::Posts) => {
                                    if let Some(page) = self.channels.page.take() {
                                        let load_more =
                                            self.channel_posts_tab_ui(ui, ctx, &page, &layout);
                                        self.channels.page = Some(page);
                                        if load_more {
                                            self.start_load_more_channel_continuation(ctx);
                                        }
                                    }
                                }
                                Some(crate::innertube::ChannelTabKind::Podcasts) => {
                                    if let Some(page) = self.channels.page.take() {
                                        let (sel, deferred) =
                                            self.channel_podcasts_tab_ui(ui, ctx, &page, &layout);
                                        self.channels.page = Some(page);
                                        chosen = sel;
                                        self.apply_channel_home_deferred(ctx, deferred);
                                    }
                                }
                                Some(crate::innertube::ChannelTabKind::Releases) => {
                                    if let Some(page) = self.channels.page.take() {
                                        let (sel, deferred) =
                                            self.channel_releases_tab_ui(ui, ctx, &page, &layout);
                                        self.channels.page = Some(page);
                                        chosen = sel;
                                        self.apply_channel_home_deferred(ctx, deferred);
                                    }
                                }
                                _ => match self.channels.tab {
                                    ChannelTab::Home => {
                                        if let Some(page) = self.channels.page.take() {
                                            let (sel, deferred) = self
                                                .channel_home_tab_ui_deferred(
                                                    ui, ctx, &page, &layout,
                                                );
                                            self.channels.page = Some(page);
                                            chosen = sel;
                                            self.apply_channel_home_deferred(ctx, deferred);
                                        }
                                    }
                                    ChannelTab::Videos => {
                                        if let Some(page) = self.channels.page.take() {
                                            let (sel, deferred) = self
                                                .channel_videos_tab_ui_deferred(
                                                    ui, ctx, &page, &layout,
                                                );
                                            self.channels.page = Some(page);
                                            chosen = sel;
                                            self.apply_channel_videos_deferred(ctx, deferred);
                                        }
                                    }
                                    ChannelTab::Shorts => {
                                        chosen = self.channel_tab_videos_ui(ui, ctx, true, &layout);
                                    }
                                    ChannelTab::Streams => {
                                        chosen =
                                            self.channel_tab_videos_ui(ui, ctx, false, &layout);
                                    }
                                    ChannelTab::Playlists => {
                                        chosen = self.channel_playlists_tab_ui(ui, ctx, &layout);
                                    }
                                    ChannelTab::Search => {
                                        chosen = self.channel_search_tab_ui(ui, ctx, &layout);
                                    }
                                    ChannelTab::About => {
                                        if let Some(page) = self.channels.page.take() {
                                            let deferred = self
                                                .channel_about_tab_ui_deferred(ui, &page, &layout);
                                            self.channels.page = Some(page);
                                            self.apply_channel_about_deferred(ctx, deferred);
                                        }
                                    }
                                },
                            }
                        });
                    });
                });
            self.thumbnails.end_pass(ctx);
        } else if loading {
            let max_container = if compact {
                ui.available_width()
            } else {
                max_card_width
                    .map(|limit| ui.available_width().min(limit).min(1284.0))
                    .unwrap_or_else(|| ui.available_width().min(1284.0))
            };
            let dark_theme = self.is_dark_theme();
            egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_channel_skeleton"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let avail = ui.available_width();
                    let card_max_w = max_container.min(avail).max(200.0);
                    let side_pad = if !compact && avail > card_max_w {
                        ((avail - card_max_w) * 0.5).floor()
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        if side_pad > 0.0 {
                            ui.add_space(side_pad);
                        }
                        ui.vertical(|ui| {
                            ui.set_width(card_max_w);
                            ui.set_max_width(card_max_w);
                            skeleton::channel_page_skeleton(ui, card_max_w, compact, dark_theme);
                        });
                    });
                });
        }

        if cancel_initial {
            self.cancel_channel_loading();
        }
        if actions.back_to_results {
            self.close_channel_view();
        }
        if let Some(limit) = actions.new_limit {
            self.channels.video_limit = limit;
        }
        if (actions.new_limit.is_some() || actions.reload_target.is_some())
            && let Some(target) = actions
                .reload_target
                .or_else(|| self.current_channel_target())
        {
            let force = actions.force_refresh && actions.new_limit.is_none();
            self.open_channel_with_options(target, ctx, force);
        }
        chosen
    }

    #[allow(dead_code)]
    pub(crate) fn channel_home_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let (chosen, deferred) = self.channel_home_tab_ui_deferred(ui, ctx, page, layout);
        self.apply_channel_home_deferred(ctx, deferred);
        chosen
    }

    fn channel_home_tab_ui_deferred(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> (Option<Video>, ChannelHomeDeferred) {
        if page.featured_video.is_none() && page.home_shelves.is_empty() {
            let (chosen, videos_fallback) =
                self.channel_videos_tab_ui_deferred(ui, ctx, page, layout);
            return (
                chosen,
                ChannelHomeDeferred {
                    videos_fallback,
                    ..ChannelHomeDeferred::default()
                },
            );
        }

        let palette = layout.palette;
        let current_url = self.selected.as_ref().map(|v| v.url.clone());
        let mut chosen = None;
        let mut enqueue_all: Vec<Video> = Vec::new();
        let mut open_playlist: Option<search::ChannelPlaylist> = None;

        // 1. Featured Spotlight Video (Hero Row)
        if let Some(featured) = &page.featured_video {
            let video = &featured.video;
            let meta = &featured.metadata;
            let mut card_action = SearchCardAction::default();
            let is_live = meta.is_live
                || meta
                    .badges
                    .iter()
                    .any(|b| b.eq_ignore_ascii_case("ao vivo") || b.eq_ignore_ascii_case("live"));
            let is_music = recommendations::is_music_video(video, Some(meta));
            let progress_ratio = self.youtube_search_progress_ratio(&video.url);
            let channel_name = if video.channel.trim().is_empty() {
                channel_page_display_title(page)
            } else {
                video.channel.as_str()
            };

            let paint_featured_info =
                |app: &mut Self,
                 ui: &mut egui::Ui,
                 info_w: f32,
                 card_action: &mut SearchCardAction| {
                    ui.vertical(|ui| {
                        ui.set_width(info_w);
                        ui.set_max_width(info_w);
                        ui.horizontal_top(|ui| {
                            let title_w = (info_w - 30.0).max(80.0);
                            ui.vertical(|ui| {
                                ui.set_width(title_w);
                                ui.set_max_width(title_w);
                                let title_btn = egui::Button::new(
                                    RichText::new(&video.title)
                                        .size(if layout.compact { 15.0 } else { 18.0 })
                                        .strong()
                                        .color(palette.channel_fg),
                                )
                                .frame(false)
                                .wrap();
                                if ui
                                    .add(title_btn)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    card_action.open_clicked = true;
                                }
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.label(
                                        RichText::new(channel_name)
                                            .size(12.5)
                                            .color(palette.neutral_status_fg),
                                    );
                                    if page.verified == Some(true) {
                                        paint_verified_badge(ui, &palette);
                                    }
                                    if let Some(sub) =
                                        format_youtube_card_subline(&video.duration, Some(meta))
                                    {
                                        ui.label(
                                            RichText::new(format!(" · {sub}"))
                                                .size(12.5)
                                                .color(palette.neutral_status_fg),
                                        );
                                    }
                                });
                                if let Some(desc) =
                                    description_snippet(meta.description.as_deref(), 220)
                                {
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(desc)
                                            .size(12.5)
                                            .color(palette.neutral_status_fg),
                                    );
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                app.channel_card_menu_ui(ui, video, &palette, card_action);
                            });
                        });
                    });
                };

            ui.push_id(("chan_home_featured", &video.url), |ui| {
                if layout.compact {
                    let thumb_w = layout.card_max_w;
                    let thumb_h = (thumb_w * 9.0 / 16.0).round();
                    let thumb_resp = self.thumbnails.show_rounded(
                        ui,
                        ctx,
                        &video.thumbnail,
                        egui::vec2(thumb_w, thumb_h),
                        12.0,
                    );
                    let hover_t = ctx.animate_bool_with_time(
                        ui.make_persistent_id(("channel_featured_hover", &video.url)),
                        thumb_resp.hovered(),
                        0.14,
                    );
                    paint_card_hover_highlight(ui, thumb_resp.rect, 12.0, hover_t, &palette);
                    let overlay_dur = if is_live { "" } else { video.duration.as_str() };
                    paint_thumbnail_overlays(
                        ui,
                        thumb_resp.rect,
                        overlay_dur,
                        progress_ratio,
                        is_music,
                    );
                    if thumb_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        card_action.open_clicked = true;
                    }
                    ui.add_space(8.0);
                    paint_featured_info(self, ui, layout.card_max_w, &mut card_action);
                } else {
                    let thumb_w = (layout.card_max_w * 0.39).clamp(300.0, 424.0);
                    let thumb_h = (thumb_w * 9.0 / 16.0).round();
                    ui.horizontal_top(|ui| {
                        let thumb_resp = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            &video.thumbnail,
                            egui::vec2(thumb_w, thumb_h),
                            12.0,
                        );
                        let hover_t = ctx.animate_bool_with_time(
                            ui.make_persistent_id(("channel_featured_hover", &video.url)),
                            thumb_resp.hovered(),
                            0.14,
                        );
                        paint_card_hover_highlight(ui, thumb_resp.rect, 12.0, hover_t, &palette);
                        let overlay_dur = if is_live { "" } else { video.duration.as_str() };
                        paint_thumbnail_overlays(
                            ui,
                            thumb_resp.rect,
                            overlay_dur,
                            progress_ratio,
                            is_music,
                        );
                        if thumb_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            card_action.open_clicked = true;
                        }
                        ui.add_space(16.0);
                        let info_w = (layout.card_max_w - thumb_w - 20.0).max(160.0);
                        paint_featured_info(self, ui, info_w, &mut card_action);
                    });
                }
            });
            self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
            ui.add_space(18.0);
            if !page.home_shelves.is_empty() {
                ui.separator();
                ui.add_space(14.0);
            }
        }

        // 2. Home Shelves (Single-row horizontal carousels with < / > scroll step buttons)
        let pt = self.ui_pt();
        for (shelf_idx, shelf) in page.home_shelves.iter().enumerate() {
            if shelf.items.is_empty() {
                continue;
            }
            ui.push_id(("chan_shelf", shelf_idx, &shelf.title), |ui| {
                let is_shorts_shelf = shelf.kind == search::ChannelShelfKind::Shorts;
                let gap = if is_shorts_shelf { 12.0 } else { 16.0 };
                let card_w = if is_shorts_shelf {
                    if layout.compact {
                        (layout.card_max_w * 0.46).clamp(132.0, 178.0)
                    } else {
                        ((layout.card_max_w - gap * 4.0) / 5.0).clamp(156.0, 212.0)
                    }
                } else if layout.compact {
                    (layout.card_max_w * 0.82).clamp(210.0, 280.0)
                } else {
                    ((layout.card_max_w - gap * 3.0) / 4.0).clamp(230.0, 300.0)
                };
                let step_px = ((card_w + gap) * if layout.compact { 1.0 } else { 2.0 }).max(180.0);
                let scroll_offset_key =
                    ui.make_persistent_id(("chan_home_carousel_offset", shelf_idx, &shelf.title));
                let stored_offset = ctx
                    .data(|d| d.get_temp::<f32>(scroll_offset_key))
                    .unwrap_or(0.0);
                let est_content_w = shelf.items.len() as f32 * card_w
                    + shelf.items.len().saturating_sub(1) as f32 * gap;
                let est_max_x = (est_content_w - layout.card_max_w).max(0.0);
                let can_scroll_left = stored_offset > 1.0;
                let can_scroll_right = stored_offset + 1.0 < est_max_x;
                let mut prev_clicked = false;
                let mut next_clicked = false;

                ui.horizontal(|ui| {
                    ui.set_max_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if is_shorts_shelf {
                            paint_shorts_badge_icon(ui);
                        }
                        ui.label(
                            RichText::new(&shelf.title)
                                .size(if layout.compact { 16.5 } else { 20.0 })
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if let Some(pid) = &shelf.playlist_id {
                            let play_btn = egui::Button::new(
                                RichText::new(if pt {
                                    "▶ Reproduzir tudo"
                                } else {
                                    "▶ Play all"
                                })
                                .size(13.0)
                                .strong()
                                .color(palette.header_button_fg),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(16);
                            let play_resp = ui
                                .add(play_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            paint_focus_ring_if_focused(ui, &play_resp, 16.0);
                            if play_resp.clicked() {
                                if let Some(first) = shelf.items.first() {
                                    chosen = Some(first.video.clone());
                                    for extra in shelf.items.iter().skip(1) {
                                        enqueue_all.push(extra.video.clone());
                                    }
                                } else {
                                    open_playlist = Some(search::ChannelPlaylist {
                                        id: pid.clone(),
                                        title: shelf.title.clone(),
                                        url: format!("https://www.youtube.com/playlist?list={pid}"),
                                        thumbnail: String::new(),
                                        item_count: None,
                                        description: shelf.subtitle.clone(),
                                        privacy: crate::playlist_sync::PlaylistPrivacy::Unknown,
                                    });
                                }
                            }
                        }
                    });

                    if est_max_x > 1.0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let next_resp =
                                styled_icon_chip_button(ui, can_scroll_right, "›", &palette)
                                    .on_hover_text(if pt { "Avançar" } else { "Scroll right" });
                            if next_resp.clicked() && can_scroll_right {
                                next_clicked = true;
                            }
                            let prev_resp =
                                styled_icon_chip_button(ui, can_scroll_left, "‹", &palette)
                                    .on_hover_text(if pt { "Voltar" } else { "Scroll left" });
                            if prev_resp.clicked() && can_scroll_left {
                                prev_clicked = true;
                            }
                        });
                    }
                });

                if let Some(subtitle) = &shelf.subtitle {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(subtitle)
                            .size(13.0)
                            .color(palette.neutral_status_fg),
                    );
                }
                ui.add_space(12.0);

                let scroll_out = egui::ScrollArea::horizontal()
                    .id_salt(("chan_home_carousel_scroll", shelf_idx, &shelf.title))
                    .max_width(layout.card_max_w)
                    .auto_shrink([false, true])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .horizontal_scroll_offset(stored_offset)
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for (item_idx, item) in shelf.items.iter().enumerate() {
                                if is_shorts_shelf {
                                    let card_action = ui
                                        .push_id(("shelf_short", item_idx, &item.video.url), |ui| {
                                            self.channel_short_card_ui(
                                                ui,
                                                ctx,
                                                &item.video,
                                                card_w,
                                                &palette,
                                            )
                                        })
                                        .inner;
                                    self.handle_channel_card_actions(
                                        ctx,
                                        &card_action,
                                        &mut chosen,
                                        &item.video,
                                    );
                                } else if item.metadata.kind == search::SearchResultKind::Playlist
                                    || item.video.url.contains("playlist?list=")
                                {
                                    let pid = extract_playlist_id_from_url(&item.video.url)
                                        .unwrap_or_else(|| format!("shelf_pl_{item_idx}"));
                                    let count_num =
                                        item.metadata.playlist_count.as_deref().and_then(|s| {
                                            let digits: String =
                                                s.chars().filter(|c| c.is_ascii_digit()).collect();
                                            digits.parse::<u64>().ok()
                                        });
                                    let pl = search::ChannelPlaylist {
                                        id: pid,
                                        title: item.video.title.clone(),
                                        url: item.video.url.clone(),
                                        thumbnail: item.video.thumbnail.clone(),
                                        item_count: count_num,
                                        description: item.metadata.description.clone(),
                                        privacy: search::playlist_privacy_from_badges(
                                            &item.metadata.badges,
                                        ),
                                    };
                                    let opened = ui
                                        .push_id(("shelf_pl", item_idx, &pl.id), |ui| {
                                            self.channel_playlist_card_ui(
                                                ui, ctx, &pl, card_w, palette,
                                            )
                                        })
                                        .inner;
                                    if opened {
                                        open_playlist = Some(pl);
                                    }
                                } else {
                                    let is_playing =
                                        current_url.as_deref() == Some(item.video.url.as_str());
                                    let card_action = ui
                                        .push_id(("shelf_vid", item_idx, &item.video.url), |ui| {
                                            self.channel_grid_video_card_ui(
                                                ui,
                                                ctx,
                                                &item.video,
                                                card_w,
                                                is_playing,
                                                &palette,
                                            )
                                        })
                                        .inner;
                                    self.handle_channel_card_actions(
                                        ctx,
                                        &card_action,
                                        &mut chosen,
                                        &item.video,
                                    );
                                }
                            }
                        });
                    });

                let max_x = (scroll_out.content_size.x - scroll_out.inner_rect.width()).max(0.0);
                let current_x = scroll_out.state.offset.x.clamp(0.0, max_x);
                let next_x = if prev_clicked {
                    step_carousel_offset(current_x, -step_px, max_x)
                } else if next_clicked {
                    step_carousel_offset(current_x, step_px, max_x)
                } else {
                    current_x
                };
                ctx.data_mut(|d| {
                    d.insert_temp(scroll_offset_key, next_x);
                });
                if prev_clicked || next_clicked {
                    let mut state = scroll_out.state;
                    state.offset.x = next_x;
                    state.store(ctx, scroll_out.id);
                }
                ui.add_space(16.0);
            });
            if shelf_idx + 1 < page.home_shelves.len() {
                ui.separator();
                ui.add_space(14.0);
            }
        }

        (
            chosen,
            ChannelHomeDeferred {
                enqueue_all,
                open_playlist,
                videos_fallback: ChannelVideosDeferred::default(),
            },
        )
    }

    fn apply_channel_home_deferred(&mut self, ctx: &egui::Context, deferred: ChannelHomeDeferred) {
        self.apply_channel_videos_deferred(ctx, deferred.videos_fallback);
        for video in deferred.enqueue_all {
            let _ = self.enqueue_video(video);
        }
        if let Some(pl) = deferred.open_playlist {
            self.channels.tab = ChannelTab::Playlists;
            self.open_channel_public_playlist(pl, ctx);
        }
    }

    pub(crate) fn channel_tab_videos_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        is_shorts: bool,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let pt = self.ui_pt();
        let (loading, loaded, error, items) = if is_shorts {
            (
                self.channels.is_shorts_loading(),
                self.channels.shorts.loaded,
                self.channels.shorts.error.clone(),
                std::mem::take(&mut self.channels.shorts.items),
            )
        } else {
            (
                self.channels.is_streams_loading(),
                self.channels.streams.loaded,
                self.channels.streams.error.clone(),
                std::mem::take(&mut self.channels.streams.items),
            )
        };

        let mut reload = false;
        let mut cancel = false;
        let mut chosen = None;
        let mut selected_sort_chip_continuation = None;
        let mut load_more_continuation = false;

        if let Some(page) = self.channels.page.as_ref()
            && !page.sort_chips.is_empty()
        {
            selected_sort_chip_continuation =
                channel_sort_chips_bar_ui(ui, &page.sort_chips, &palette);
            ui.add_space(12.0);
        }

        let dark_theme = self.is_dark_theme();

        if loading {
            ui.horizontal(|ui| {
                ui.set_max_width(layout.card_max_w);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_ghost_button(ui, "Cancelar", &palette).clicked() {
                        cancel = true;
                    }
                });
            });
            ui.add_space(6.0);
            if items.is_empty() {
                skeleton::channel_tab_grid_skeleton(
                    ui,
                    layout.card_max_w,
                    layout.compact,
                    is_shorts,
                    2,
                    dark_theme,
                );
                ui.add_space(6.0);
            }
        }

        if let Some(err) = &error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar: {err}"))
                                .color(palette.error_banner_fg),
                        );
                        if styled_compact_button(ui, "↻ Tentar novamente", false, &palette)
                            .clicked()
                        {
                            reload = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if loaded && items.is_empty() && error.is_none() {
            channel_empty_state_card_ui(
                ui,
                layout.card_max_w,
                if is_shorts {
                    "Este canal não tem Shorts públicos."
                } else {
                    "Este canal não tem transmissões públicas."
                },
                palette,
            );
        } else if !items.is_empty() {
            let current_url = self.selected.as_ref().map(|v| v.url.clone());
            let inner_spacing_y = ui.spacing().item_spacing.y;
            if is_shorts {
                let gap = 12.0;
                let row_gap = 14.0;
                let cols = if layout.compact {
                    2
                } else {
                    ((layout.card_max_w + gap) / 205.0).floor().clamp(2.0, 5.0) as usize
                };
                let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                    / cols as f32)
                    .max(120.0);
                let total_rows = items.len().div_ceil(cols);
                let row_h = channel_short_row_height(card_w);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let grid_top = ui.cursor().top();
                    let clip = ui.clip_rect();
                    let (visible_rows, top_spacer_h, bottom_spacer_h) =
                        channel_grid_visible_row_range(
                            total_rows,
                            row_h,
                            row_gap,
                            grid_top,
                            clip.top(),
                            clip.bottom(),
                            2,
                        );
                    if top_spacer_h > 0.0 {
                        ui.allocate_exact_size(
                            egui::vec2(layout.card_max_w, top_spacer_h),
                            egui::Sense::hover(),
                        );
                    }
                    let rendered_end = visible_rows.end;
                    for row_idx in visible_rows {
                        let start = row_idx * cols;
                        let end = (start + cols).min(items.len());
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.y = inner_spacing_y;
                            ui.spacing_mut().item_spacing.x = gap;
                            for (col_idx, item) in items[start..end].iter().enumerate() {
                                let video = &item.video;
                                let card_action = ui
                                    .push_id(("chan_short", row_idx, col_idx, &video.url), |ui| {
                                        self.channel_short_card_ui(ui, ctx, video, card_w, &palette)
                                    })
                                    .inner;
                                self.handle_channel_card_actions(
                                    ctx,
                                    &card_action,
                                    &mut chosen,
                                    video,
                                );
                            }
                        });
                        if row_idx + 1 < rendered_end {
                            ui.add_space(row_gap);
                        }
                    }
                    if bottom_spacer_h > 0.0 {
                        ui.allocate_exact_size(
                            egui::vec2(layout.card_max_w, bottom_spacer_h),
                            egui::Sense::hover(),
                        );
                    }
                });
                ui.add_space(row_gap);
            } else {
                let gap = 16.0;
                let row_gap = 18.0;
                let cols = if layout.compact {
                    1
                } else {
                    ((layout.card_max_w + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
                };
                let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                    / cols as f32)
                    .max(180.0);
                let total_rows = items.len().div_ceil(cols);
                let row_h = channel_video_row_height(card_w);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let grid_top = ui.cursor().top();
                    let clip = ui.clip_rect();
                    let (visible_rows, top_spacer_h, bottom_spacer_h) =
                        channel_grid_visible_row_range(
                            total_rows,
                            row_h,
                            row_gap,
                            grid_top,
                            clip.top(),
                            clip.bottom(),
                            2,
                        );
                    if top_spacer_h > 0.0 {
                        ui.allocate_exact_size(
                            egui::vec2(layout.card_max_w, top_spacer_h),
                            egui::Sense::hover(),
                        );
                    }
                    let rendered_end = visible_rows.end;
                    for row_idx in visible_rows {
                        let start = row_idx * cols;
                        let end = (start + cols).min(items.len());
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.y = inner_spacing_y;
                            ui.spacing_mut().item_spacing.x = gap;
                            for (col_idx, item) in items[start..end].iter().enumerate() {
                                let video = &item.video;
                                let is_playing = current_url.as_deref() == Some(video.url.as_str());
                                let card_action = ui
                                    .push_id(("chan_stream", row_idx, col_idx, &video.url), |ui| {
                                        self.channel_grid_video_card_ui(
                                            ui, ctx, video, card_w, is_playing, &palette,
                                        )
                                    })
                                    .inner;
                                self.handle_channel_card_actions(
                                    ctx,
                                    &card_action,
                                    &mut chosen,
                                    video,
                                );
                            }
                        });
                        if row_idx + 1 < rendered_end {
                            ui.add_space(row_gap);
                        }
                    }
                    if bottom_spacer_h > 0.0 {
                        ui.allocate_exact_size(
                            egui::vec2(layout.card_max_w, bottom_spacer_h),
                            egui::Sense::hover(),
                        );
                    }
                });
                ui.add_space(row_gap);
            }
        }

        let has_continuation = self
            .channels
            .page
            .as_ref()
            .is_some_and(|p| p.next_continuation.is_some());
        let more_loading = self.is_channel_more_videos_loading() || (loading && !items.is_empty());
        if has_continuation && (!items.is_empty() || more_loading) {
            if channel_continuation_footer_ui(ui, layout.card_max_w, more_loading, pt, &palette) {
                load_more_continuation = true;
            }
            ui.add_space(12.0);
        }

        if is_shorts {
            self.channels.shorts.items = items;
        } else {
            self.channels.streams.items = items;
        }

        if let Some(continuation) = selected_sort_chip_continuation {
            self.select_channel_sort_chip(continuation, ctx);
        } else if load_more_continuation {
            self.start_load_more_channel_continuation(ctx);
        } else if cancel {
            if is_shorts {
                self.cancel_channel_shorts();
            } else {
                self.cancel_channel_streams();
            }
        } else if reload {
            if is_shorts {
                self.start_channel_shorts(ctx);
            } else {
                self.start_channel_streams(ctx);
            }
        }
        chosen
    }

    #[allow(dead_code)]
    pub(crate) fn channel_videos_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let (chosen, deferred) = self.channel_videos_tab_ui_deferred(ui, ctx, page, layout);
        self.apply_channel_videos_deferred(ctx, deferred);
        chosen
    }

    fn channel_videos_tab_ui_deferred(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> (Option<Video>, ChannelVideosDeferred) {
        let palette = layout.palette;
        let pt = self.ui_pt();
        let mut chosen = None;
        let mut load_more_clicked = false;
        let mut load_more_continuation_clicked = false;
        let mut cancel_more_clicked = false;
        let mut requested_sort = None;
        let mut selected_sort_chip_continuation = None;
        let active_sort = self.channels.videos.sort;
        let sort_loading = self.channels.is_sorted_videos_loading();
        let sort_error = self.channels.videos.sorted_error.clone();
        let more_loading = self.is_channel_more_videos_loading();
        let more_error = self.channels.videos.more_error.clone();
        let has_innertube_continuation = page.next_continuation.is_some();
        let has_more = self.channels.videos.has_more;
        let reached_max = !has_innertube_continuation
            && (self.channels.videos.reached_max
                || page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT);

        // Sort chips: prefer InnerTube dynamic sort chips (Latest / Popular / Oldest) when available.
        if !page.sort_chips.is_empty() {
            selected_sort_chip_continuation =
                channel_sort_chips_bar_ui(ui, &page.sort_chips, &palette);
            ui.add_space(14.0);
        } else if !page.videos.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (sort, label) in [
                    (
                        search::ChannelVideosSort::Latest,
                        if pt { "Mais recentes" } else { "Latest" },
                    ),
                    (
                        search::ChannelVideosSort::Popular,
                        if pt { "Em alta" } else { "Popular" },
                    ),
                    (
                        search::ChannelVideosSort::Oldest,
                        if pt { "Mais antigos" } else { "Oldest" },
                    ),
                ] {
                    let selected = active_sort == sort;
                    let fill = if selected {
                        palette.active_button_bg
                    } else {
                        palette.header_button_bg
                    };
                    let fg = if selected {
                        palette.active_button_fg
                    } else {
                        palette.header_button_fg
                    };
                    let chip =
                        egui::Button::new(RichText::new(label).size(13.0).strong().color(fg))
                            .fill(fill)
                            .stroke(egui::Stroke::NONE)
                            .min_size(egui::vec2(0.0, 32.0))
                            .corner_radius(8);
                    if ui
                        .add(chip)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                        && active_sort != sort
                    {
                        requested_sort = Some(sort);
                    }
                }
            });
            ui.add_space(14.0);
        }

        let dark_theme = self.is_dark_theme();
        if sort_loading {
            skeleton::channel_tab_grid_skeleton(
                ui,
                layout.card_max_w,
                layout.compact,
                false,
                2,
                dark_theme,
            );
            ui.add_space(10.0);
            return (
                None,
                ChannelVideosDeferred {
                    requested_sort,
                    selected_sort_chip_continuation,
                    cancel_more_clicked: false,
                    load_more_clicked: false,
                    load_more_continuation_clicked: false,
                },
            );
        } else if let Some(err) = sort_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.label(
                        RichText::new(if pt {
                            format!("Erro ao ordenar v\u{00ed}deos: {err}")
                        } else {
                            format!("Failed to sort videos: {err}")
                        })
                        .small()
                        .color(palette.error_banner_fg),
                    );
                });
            ui.add_space(8.0);
        }

        let use_sorted = active_sort != search::ChannelVideosSort::Latest
            && !self.channels.videos.sorted_items.is_empty();
        let taken_sorted = if use_sorted {
            Some(std::mem::take(&mut self.channels.videos.sorted_items))
        } else {
            None
        };
        let channel_items: &[SearchResult] = taken_sorted.as_deref().unwrap_or(&page.videos);

        if channel_items.is_empty() {
            if let Some(sorted) = taken_sorted {
                self.channels.videos.sorted_items = sorted;
            }
            channel_empty_state_card_ui(
                ui,
                layout.card_max_w,
                if pt {
                    "Nenhum v\u{00ed}deo p\u{00fa}blico listado para este canal."
                } else {
                    "No public videos listed for this channel."
                },
                palette,
            );
            return (
                None,
                ChannelVideosDeferred {
                    requested_sort,
                    selected_sort_chip_continuation,
                    cancel_more_clicked: false,
                    load_more_clicked: false,
                    load_more_continuation_clicked: false,
                },
            );
        }

        let total_video_count = channel_items.len();
        let current_url = self.selected.as_ref().map(|video| video.url.clone());
        let gap = 16.0;
        let row_gap = 18.0;
        let cols = if layout.compact {
            1
        } else {
            ((layout.card_max_w + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
        };
        let card_w =
            ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(180.0);
        let total_rows = total_video_count.div_ceil(cols);
        let row_h = channel_video_row_height(card_w);
        let inner_spacing_y = ui.spacing().item_spacing.y;

        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let grid_top = ui.cursor().top();
            let clip = ui.clip_rect();
            let (visible_rows, top_spacer_h, bottom_spacer_h) = channel_grid_visible_row_range(
                total_rows,
                row_h,
                row_gap,
                grid_top,
                clip.top(),
                clip.bottom(),
                2,
            );
            if top_spacer_h > 0.0 {
                ui.allocate_exact_size(
                    egui::vec2(layout.card_max_w, top_spacer_h),
                    egui::Sense::hover(),
                );
            }
            let rendered_end = visible_rows.end;
            for row_idx in visible_rows {
                let start = row_idx * cols;
                let end = (start + cols).min(total_video_count);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.y = inner_spacing_y;
                    ui.spacing_mut().item_spacing.x = gap;
                    for (col_idx, item) in channel_items[start..end].iter().enumerate() {
                        let video = &item.video;
                        let is_playing = current_url.as_deref() == Some(video.url.as_str());
                        let card_action = ui
                            .push_id(("chan_vid_grid", row_idx, col_idx, &video.url), |ui| {
                                self.channel_grid_video_card_ui(
                                    ui, ctx, video, card_w, is_playing, &palette,
                                )
                            })
                            .inner;
                        self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                    }
                });
                if row_idx + 1 < rendered_end {
                    ui.add_space(row_gap);
                }
            }
            if bottom_spacer_h > 0.0 {
                ui.allocate_exact_size(
                    egui::vec2(layout.card_max_w, bottom_spacer_h),
                    egui::Sense::hover(),
                );
            }
        });
        ui.add_space(row_gap);

        if let Some(sorted) = taken_sorted {
            self.channels.videos.sorted_items = sorted;
        }

        if has_innertube_continuation {
            if let Some(err) = &more_error {
                egui::Frame::new()
                    .fill(palette.error_bg)
                    .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(layout.card_max_w);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(if pt {
                                    format!("Erro ao carregar mais v\u{00ed}deos: {err}")
                                } else {
                                    format!("Failed to load more videos: {err}")
                                })
                                .small()
                                .color(palette.error_banner_fg),
                            );
                            if styled_compact_button(
                                ui,
                                if pt {
                                    "↻ Tentar novamente"
                                } else {
                                    "↻ Retry"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                load_more_continuation_clicked = true;
                            }
                        });
                    });
                ui.add_space(8.0);
            }

            if channel_continuation_footer_ui(ui, layout.card_max_w, more_loading, pt, &palette) {
                load_more_continuation_clicked = true;
            }
            if more_loading {
                ui.horizontal(|ui| {
                    ui.set_max_width(layout.card_max_w);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if styled_ghost_button(ui, if pt { "Cancelar" } else { "Cancel" }, &palette)
                            .clicked()
                        {
                            cancel_more_clicked = true;
                        }
                    });
                });
            }
        } else if active_sort == search::ChannelVideosSort::Latest {
            if let Some(err) = &more_error {
                egui::Frame::new()
                    .fill(palette.error_bg)
                    .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(layout.card_max_w);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(if pt {
                                    format!("Erro ao carregar mais v\u{00ed}deos: {err}")
                                } else {
                                    format!("Failed to load more videos: {err}")
                                })
                                .small()
                                .color(palette.error_banner_fg),
                            );
                            if styled_compact_button(
                                ui,
                                if pt {
                                    "↻ Tentar novamente"
                                } else {
                                    "↻ Retry"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                load_more_clicked = true;
                            }
                        });
                    });
                ui.add_space(8.0);
            }

            if more_loading {
                skeleton::channel_tab_grid_skeleton(
                    ui,
                    layout.card_max_w,
                    layout.compact,
                    false,
                    1,
                    dark_theme,
                );
                ui.horizontal(|ui| {
                    ui.set_max_width(layout.card_max_w);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if styled_ghost_button(ui, if pt { "Cancelar" } else { "Cancel" }, &palette)
                            .on_hover_text(if pt {
                                "Cancelar carregamento adicional de v\u{00ed}deos"
                            } else {
                                "Cancel loading more videos"
                            })
                            .clicked()
                        {
                            cancel_more_clicked = true;
                        }
                    });
                });
            } else if reached_max {
                ui.label(
                    RichText::new(if pt {
                        format!(
                            "Limite m\u{00e1}ximo de {} v\u{00ed}deos por canal atingido.",
                            search::MAX_CHANNEL_VIDEOS_LIMIT
                        )
                    } else {
                        format!(
                            "Reached maximum limit of {} videos for this channel.",
                            search::MAX_CHANNEL_VIDEOS_LIMIT
                        )
                    })
                    .small()
                    .color(palette.neutral_status_fg),
                );
            } else if has_more {
                let remaining = search::MAX_CHANNEL_VIDEOS_LIMIT.saturating_sub(total_video_count);
                let step = remaining.min(search::CHANNEL_VIDEOS_PAGE_STEP);
                let more_btn = egui::Button::new(
                    RichText::new(if pt {
                        format!(
                            "Carregar mais v\u{00ed}deos (+{step}, at\u{00e9} {})",
                            search::MAX_CHANNEL_VIDEOS_LIMIT
                        )
                    } else {
                        format!(
                            "Load more videos (+{step}, up to {})",
                            search::MAX_CHANNEL_VIDEOS_LIMIT
                        )
                    })
                    .size(13.0)
                    .strong()
                    .color(palette.header_button_fg),
                )
                .fill(palette.header_button_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .min_size(egui::vec2(0.0, 34.0))
                .corner_radius(17);
                if ui
                    .add(more_btn)
                    .on_hover_text(if pt {
                        "Buscar pr\u{00f3}ximo lote de v\u{00ed}deos p\u{00fa}blicos do canal sob demanda"
                    } else {
                        "Fetch next batch of public channel videos on demand"
                    })
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    load_more_clicked = true;
                }
            }
        }

        (
            chosen,
            ChannelVideosDeferred {
                requested_sort,
                selected_sort_chip_continuation,
                cancel_more_clicked,
                load_more_clicked,
                load_more_continuation_clicked,
            },
        )
    }

    fn apply_channel_videos_deferred(
        &mut self,
        ctx: &egui::Context,
        deferred: ChannelVideosDeferred,
    ) {
        if let Some(continuation) = deferred.selected_sort_chip_continuation {
            self.select_channel_sort_chip(continuation, ctx);
        } else if let Some(sort) = deferred.requested_sort {
            self.start_channel_sorted_videos(sort, ctx);
        }
        if deferred.cancel_more_clicked {
            self.cancel_channel_more_videos();
        } else if deferred.load_more_continuation_clicked {
            self.start_load_more_channel_continuation(ctx);
        } else if deferred.load_more_clicked {
            self.load_more_channel_videos(ctx);
        }
    }

    pub(crate) fn channel_playlists_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        if let Some(selected_playlist) = self.channels.playlist_detail.selected.clone() {
            return self.channel_playlist_detail_ui(ui, ctx, layout, selected_playlist);
        }

        self.channel_playlists_list_ui(ui, ctx, layout)
    }

    pub(crate) fn channel_playlist_detail_header_ui(
        &self,
        ui: &mut egui::Ui,
        selected_playlist: &search::ChannelPlaylist,
        loading_videos: bool,
        palette: ThemePalette,
    ) -> (bool, bool) {
        let mut back_to_playlists = false;
        let mut reload_playlist = false;

        ui.horizontal_wrapped(|ui| {
            let back_btn = egui::Button::new(
                RichText::new("← Voltar às playlists")
                    .size(12.5)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .stroke(egui::Stroke::NONE)
            .corner_radius(16);
            if ui
                .add(back_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                back_to_playlists = true;
            }
            ui.label(
                RichText::new(&selected_playlist.title)
                    .size(16.0)
                    .strong()
                    .color(palette.channel_fg),
            );
            if let Some(count) = selected_playlist.item_count {
                ui.label(
                    RichText::new(format!("({count} vídeos)"))
                        .size(13.0)
                        .color(palette.neutral_status_fg),
                );
            }
            if styled_compact_button_enabled(ui, !loading_videos, "↻ Atualizar", false, &palette)
                .clicked()
            {
                reload_playlist = true;
            }
        });
        ui.add_space(10.0);

        (back_to_playlists, reload_playlist)
    }

    pub(crate) fn channel_playlist_detail_loading_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> bool {
        let mut cancel_playlist_videos = false;
        let dark_theme = self.is_dark_theme();
        let compact = card_max_w < 560.0;

        ui.horizontal(|ui| {
            ui.set_max_width(card_max_w);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if styled_ghost_button(ui, "Cancelar", &palette).clicked() {
                    cancel_playlist_videos = true;
                }
            });
        });
        ui.add_space(6.0);
        skeleton::channel_tab_grid_skeleton(ui, card_max_w, compact, false, 2, dark_theme);
        ui.add_space(8.0);

        cancel_playlist_videos
    }

    pub(crate) fn channel_playlist_detail_error_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
        err: &str,
    ) -> bool {
        let mut reload_playlist = false;

        egui::Frame::new()
            .fill(palette.error_bg)
            .stroke(egui::Stroke::new(1.0, palette.error_stroke))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(card_max_w);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Erro ao carregar vídeos da playlist: {err}"))
                            .color(palette.error_banner_fg),
                    );
                    if styled_compact_button(ui, "↻ Tentar novamente", false, &palette).clicked()
                    {
                        reload_playlist = true;
                    }
                });
            });
        ui.add_space(8.0);

        reload_playlist
    }

    pub(crate) fn channel_playlist_detail_empty_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        channel_empty_state_card_ui(
            ui,
            card_max_w,
            self.pick(
                "Nenhum vídeo disponível nesta playlist.",
                "No videos available in this playlist.",
            ),
            palette,
        );
    }

    pub(crate) fn channel_playlist_detail_status_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> (bool, bool) {
        let loading_videos = self.is_channel_playlist_videos_loading();
        let videos_error = self.channels.playlist_detail.error.as_deref();
        let videos_loaded = self.channels.playlist_detail.loaded;
        let has_videos = !self.channels.playlist_detail.items.is_empty();
        let mut cancel_playlist_videos = false;
        let mut reload_playlist = false;

        if loading_videos && self.channel_playlist_detail_loading_ui(ui, card_max_w, palette) {
            cancel_playlist_videos = true;
        }

        if let Some(err) = videos_error
            && self.channel_playlist_detail_error_ui(ui, card_max_w, palette, err)
        {
            reload_playlist = true;
        }

        if videos_loaded && !has_videos && videos_error.is_none() {
            self.channel_playlist_detail_empty_ui(ui, card_max_w, palette);
        }

        (cancel_playlist_videos, reload_playlist)
    }

    pub(crate) fn channel_playlist_detail_videos_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let mut chosen = None;
        let current_url = self.selected.as_ref().map(|video| video.url.clone());
        let playlist_items = std::mem::take(&mut self.channels.playlist_detail.items);

        let gap = 16.0;
        let row_gap = 18.0;
        let cols = if layout.compact {
            1
        } else {
            ((layout.card_max_w + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
        };
        let card_w =
            ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(180.0);
        let total_rows = playlist_items.len().div_ceil(cols);
        let row_h = channel_video_row_height(card_w);
        let inner_spacing_y = ui.spacing().item_spacing.y;

        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let grid_top = ui.cursor().top();
            let clip = ui.clip_rect();
            let (visible_rows, top_spacer_h, bottom_spacer_h) = channel_grid_visible_row_range(
                total_rows,
                row_h,
                row_gap,
                grid_top,
                clip.top(),
                clip.bottom(),
                2,
            );
            if top_spacer_h > 0.0 {
                ui.allocate_exact_size(
                    egui::vec2(layout.card_max_w, top_spacer_h),
                    egui::Sense::hover(),
                );
            }
            let rendered_end = visible_rows.end;
            for row_idx in visible_rows {
                let start = row_idx * cols;
                let end = (start + cols).min(playlist_items.len());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.y = inner_spacing_y;
                    ui.spacing_mut().item_spacing.x = gap;
                    for (col_idx, item) in playlist_items[start..end].iter().enumerate() {
                        let video = &item.video;
                        let is_playing = current_url.as_deref() == Some(video.url.as_str());
                        let card_action = ui
                            .push_id(("chan_pl_vid", row_idx, col_idx, &video.url), |ui| {
                                self.channel_grid_video_card_ui(
                                    ui, ctx, video, card_w, is_playing, &palette,
                                )
                            })
                            .inner;
                        self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                    }
                });
                if row_idx + 1 < rendered_end {
                    ui.add_space(row_gap);
                }
            }
            if bottom_spacer_h > 0.0 {
                ui.allocate_exact_size(
                    egui::vec2(layout.card_max_w, bottom_spacer_h),
                    egui::Sense::hover(),
                );
            }
        });
        ui.add_space(row_gap);

        let item_count = playlist_items.len();
        self.channels.playlist_detail.items = playlist_items;

        if item_count >= search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT {
            let pt = self.ui_pt();
            ui.label(
                RichText::new(if pt {
                    format!(
                        "Exibindo até o limite de {} vídeos nesta playlist.",
                        search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT
                    )
                } else {
                    format!(
                        "Showing up to the limit of {} videos in this playlist.",
                        search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT
                    )
                })
                .small()
                .color(palette.neutral_status_fg),
            );
        }

        chosen
    }

    pub(crate) fn channel_playlist_detail_handle_actions(
        &mut self,
        ctx: &egui::Context,
        selected_playlist: search::ChannelPlaylist,
        back_to_playlists: bool,
        cancel_playlist_videos: bool,
        reload_playlist: bool,
    ) {
        if back_to_playlists {
            self.close_channel_public_playlist(ctx);
        } else if cancel_playlist_videos {
            self.cancel_channel_playlist_videos();
        } else if reload_playlist {
            self.open_channel_public_playlist(selected_playlist, ctx);
        }
    }

    pub(crate) fn channel_playlist_detail_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
        selected_playlist: search::ChannelPlaylist,
    ) -> Option<Video> {
        let loading_videos = self.is_channel_playlist_videos_loading();

        let (back_to_playlists, reload_header) = self.channel_playlist_detail_header_ui(
            ui,
            &selected_playlist,
            loading_videos,
            layout.palette,
        );

        let (cancel_playlist_videos, reload_status) =
            self.channel_playlist_detail_status_ui(ui, layout.card_max_w, layout.palette);

        let reload_playlist = reload_header || reload_status;

        let mut chosen = None;
        if !self.channels.playlist_detail.items.is_empty() {
            chosen = self.channel_playlist_detail_videos_ui(ui, ctx, layout);
        }
        if let Some(ref clicked) = chosen
            && let Some(pos) = self
                .channels
                .playlist_detail
                .items
                .iter()
                .position(|it| it.video.url == clicked.url)
        {
            let videos: Vec<Video> = self
                .channels
                .playlist_detail
                .items
                .iter()
                .map(|it| it.video.clone())
                .collect();
            let owner = self
                .channels
                .page
                .as_ref()
                .and_then(|p| p.title.clone())
                .unwrap_or_default();
            let pl = crate::app_state::ActivePlaylistPlayback::new(
                selected_playlist.id.clone(),
                selected_playlist.title.clone(),
                owner,
                selected_playlist.privacy,
                selected_playlist.id.starts_with("RD"),
                videos,
                selected_playlist.item_count.map(|c| c as usize),
                pos,
                false,
                1,
                loading_videos,
                None,
            );
            self.start_playlist_playback(pl);
            chosen = None;
        }

        self.channel_playlist_detail_handle_actions(
            ctx,
            selected_playlist,
            back_to_playlists,
            cancel_playlist_videos,
            reload_playlist,
        );

        chosen
    }

    pub(crate) fn channel_playlist_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        playlist: &search::ChannelPlaylist,
        width: f32,
        palette: ThemePalette,
    ) -> bool {
        let mut open_clicked = false;
        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let thumb_h = (width * 9.0 / 16.0).round();

        let wash_fill_idx = ui.painter().add(egui::Shape::Noop);
        let wash_stroke_idx = ui.painter().add(egui::Shape::Noop);
        let card_resp = ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let mut thumb_rect = egui::Rect::NOTHING;
            let mut thumb_clicked = false;
            let mut thumb_hovered = false;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (tabs_rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, 9.0), egui::Sense::hover());
                paint_mix_top_tabs(ui, tabs_rect, dark_theme);
                let resp = self.thumbnails.show_rounded(
                    ui,
                    ctx,
                    &playlist.thumbnail,
                    egui::vec2(width, (thumb_h - 9.0).max(40.0)),
                    12.0,
                );
                paint_focus_ring_if_focused(ui, &resp, 12.0);
                thumb_rect = resp.rect;
                thumb_hovered = resp.hovered();
                thumb_clicked = resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked();
            });
            let hover_t = ctx.animate_bool_with_time(
                ui.make_persistent_id(("channel_playlist_hover", &playlist.id)),
                thumb_hovered,
                0.14,
            );
            paint_card_hover_highlight(ui, thumb_rect, 12.0, hover_t, &palette);
            let count_label = playlist
                .item_count
                .map(|c| {
                    if c == 1 {
                        if pt {
                            "1 vídeo".to_owned()
                        } else {
                            "1 video".to_owned()
                        }
                    } else if pt {
                        format!("{c} vídeos")
                    } else {
                        format!("{c} videos")
                    }
                })
                .unwrap_or_else(|| "Playlist".to_owned());
            paint_playlist_badge_overlay(ui, thumb_rect, &count_label);
            if thumb_clicked {
                open_clicked = true;
            }
            ui.add_space(8.0);
            let title_galley = layout_two_line_clamped_galley(
                ui,
                &playlist.title,
                width,
                egui::FontId::proportional(14.5),
                palette.channel_fg,
            );
            let (title_rect, title_resp) = ui.allocate_exact_size(
                egui::vec2(width, title_galley.size().y.max(18.0)),
                egui::Sense::click(),
            );
            ui.painter()
                .galley(title_rect.min, title_galley, palette.channel_fg);
            paint_focus_ring_if_focused(ui, &title_resp, 6.0);
            if title_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&playlist.title)
                .clicked()
            {
                open_clicked = true;
            }
            if playlist.privacy != crate::playlist_sync::PlaylistPrivacy::Unknown {
                let privacy_label = match (playlist.privacy, pt) {
                    (crate::playlist_sync::PlaylistPrivacy::Public, true) => "Pública",
                    (crate::playlist_sync::PlaylistPrivacy::Public, false) => "Public",
                    (crate::playlist_sync::PlaylistPrivacy::Private, true) => "Particular",
                    (crate::playlist_sync::PlaylistPrivacy::Private, false) => "Private",
                    (crate::playlist_sync::PlaylistPrivacy::Unlisted, true) => "Não listada",
                    (crate::playlist_sync::PlaylistPrivacy::Unlisted, false) => "Unlisted",
                    (crate::playlist_sync::PlaylistPrivacy::Unknown, _) => "",
                };
                if !privacy_label.is_empty() {
                    ui.label(
                        RichText::new(format!("Playlist · {privacy_label}"))
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                    );
                }
            }
            let view_btn = ui
                .add(
                    egui::Button::new(
                        RichText::new(if pt {
                            "Ver playlist completa"
                        } else {
                            "View full playlist"
                        })
                        .size(12.5)
                        .color(palette.neutral_status_fg),
                    )
                    .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            paint_focus_ring_if_focused(ui, &view_btn, 4.0);
            if view_btn.clicked() {
                open_clicked = true;
            }
        });

        let full_card_hovered = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| card_resp.response.rect.contains(pos));
        let card_wash_t = ctx.animate_bool_with_time(
            ui.make_persistent_id(("channel_playlist_wash", &playlist.id)),
            full_card_hovered,
            0.14,
        );
        let (wash_fill, wash_stroke) =
            card_hover_wash_shapes(card_resp.response.rect, 12.0, card_wash_t, &palette);
        ui.painter().set(wash_fill_idx, wash_fill);
        ui.painter().set(wash_stroke_idx, wash_stroke);

        open_clicked
    }

    pub(crate) fn channel_playlists_list_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let pt = self.ui_pt();
        let mut reload_playlists = false;
        let mut cancel_playlists = false;
        let mut open_playlist = None;
        let loading_playlists = self.is_channel_playlists_loading();
        let playlists_error = self.channels.playlists.error.clone();
        let playlists_loaded = self.channels.playlists.loaded;
        let playlists = std::mem::take(&mut self.channels.playlists.items);

        let dark_theme = self.is_dark_theme();
        if loading_playlists {
            ui.horizontal(|ui| {
                ui.set_max_width(layout.card_max_w);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_ghost_button(ui, if pt { "Cancelar" } else { "Cancel" }, &palette)
                        .clicked()
                    {
                        cancel_playlists = true;
                    }
                });
            });
            ui.add_space(6.0);
            if playlists.is_empty() {
                skeleton::channel_tab_grid_skeleton(
                    ui,
                    layout.card_max_w,
                    layout.compact,
                    false,
                    2,
                    dark_theme,
                );
                ui.add_space(10.0);
            }
        }

        if let Some(err) = &playlists_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(if pt {
                                format!("Erro ao carregar playlists: {err}")
                            } else {
                                format!("Failed to load playlists: {err}")
                            })
                            .color(palette.error_banner_fg),
                        );
                        if styled_compact_button(
                            ui,
                            if pt {
                                "↻ Tentar novamente"
                            } else {
                                "↻ Retry"
                            },
                            false,
                            &palette,
                        )
                        .clicked()
                        {
                            reload_playlists = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if playlists_loaded && playlists.is_empty() && playlists_error.is_none() {
            Self::channel_playlists_empty_state_ui(ui, layout.card_max_w, palette);
        } else if !playlists.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if pt {
                        "Playlists criadas"
                    } else {
                        "Created playlists"
                    })
                    .size(16.0)
                    .strong()
                    .color(palette.channel_fg),
                );
            });
            ui.add_space(10.0);
            let gap = 16.0;
            let row_gap = 18.0;
            let cols = if layout.compact {
                1
            } else {
                ((layout.card_max_w + gap) / 260.0).floor().clamp(2.0, 4.0) as usize
            };
            let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                / cols as f32)
                .max(170.0);
            let total_rows = playlists.len().div_ceil(cols);
            let row_h = channel_playlist_row_height(card_w);
            let inner_spacing_y = ui.spacing().item_spacing.y;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let grid_top = ui.cursor().top();
                let clip = ui.clip_rect();
                let (visible_rows, top_spacer_h, bottom_spacer_h) = channel_grid_visible_row_range(
                    total_rows,
                    row_h,
                    row_gap,
                    grid_top,
                    clip.top(),
                    clip.bottom(),
                    2,
                );
                if top_spacer_h > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(layout.card_max_w, top_spacer_h),
                        egui::Sense::hover(),
                    );
                }
                let rendered_end = visible_rows.end;
                for row_idx in visible_rows {
                    let start = row_idx * cols;
                    let end = (start + cols).min(playlists.len());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.y = inner_spacing_y;
                        ui.spacing_mut().item_spacing.x = gap;
                        for (col_idx, playlist) in playlists[start..end].iter().enumerate() {
                            if ui
                                .push_id(("chan_pl", row_idx, col_idx, &playlist.id), |ui| {
                                    self.channel_playlist_card_ui(
                                        ui, ctx, playlist, card_w, palette,
                                    )
                                })
                                .inner
                            {
                                open_playlist = Some(playlist.clone());
                            }
                        }
                    });
                    if row_idx + 1 < rendered_end {
                        ui.add_space(row_gap);
                    }
                }
                if bottom_spacer_h > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(layout.card_max_w, bottom_spacer_h),
                        egui::Sense::hover(),
                    );
                }
            });
            ui.add_space(row_gap);
        }

        self.channels.playlists.items = playlists;

        if cancel_playlists {
            self.cancel_channel_playlists();
        } else if reload_playlists {
            self.start_channel_playlists(ctx);
        } else if let Some(pl) = open_playlist {
            self.open_channel_public_playlist(pl, ctx);
        }
        None
    }

    pub(crate) fn channel_search_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let loading = self.channels.is_channel_search_loading();
        let error = self.channels.search.error.clone();
        let loaded = self.channels.search.loaded;
        let query = self.channels.search.query.clone();
        let search_items = std::mem::take(&mut self.channels.search.items);
        let mut chosen = None;
        let mut retry = false;
        let mut cancel = false;

        let dark_theme = self.is_dark_theme();
        if loading {
            ui.horizontal(|ui| {
                ui.set_max_width(layout.card_max_w);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_ghost_button(ui, "Cancelar", &palette).clicked() {
                        cancel = true;
                    }
                });
            });
            ui.add_space(6.0);
            if search_items.is_empty() {
                skeleton::search_results_skeleton(ui, layout.card_max_w, dark_theme);
                ui.add_space(10.0);
            }
        }

        if let Some(err) = error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao pesquisar no canal: {err}"))
                                .color(palette.error_banner_fg),
                        );
                        if styled_compact_button(ui, "↻ Tentar novamente", false, &palette)
                            .clicked()
                        {
                            retry = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if loaded && search_items.is_empty() && self.channels.search.error.is_none() {
            channel_empty_state_card_ui(
                ui,
                layout.card_max_w,
                &format!("Nenhum resultado encontrado para \"{query}\"."),
                palette,
            );
        } else if !search_items.is_empty() {
            let row_h = crate::browse_ui::estimated_search_card_height(layout.card_max_w);
            let row_gap = 16.0;
            let inner_spacing_y = ui.spacing().item_spacing.y;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let list_top = ui.cursor().top();
                let clip = ui.clip_rect();
                let (visible_rows, top_spacer_h, bottom_spacer_h) = channel_grid_visible_row_range(
                    search_items.len(),
                    row_h,
                    row_gap,
                    list_top,
                    clip.top(),
                    clip.bottom(),
                    2,
                );
                if top_spacer_h > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(layout.card_max_w, top_spacer_h),
                        egui::Sense::hover(),
                    );
                }
                let rendered_end = visible_rows.end;
                for index in visible_rows {
                    let video = &search_items[index].video;
                    let card_action = ui
                        .push_id(("chan_search_row", index, &video.url), |ui| {
                            ui.spacing_mut().item_spacing.y = inner_spacing_y;
                            ui.set_max_width(layout.card_max_w);
                            self.youtube_search_result_card_ui(ui, ctx, video)
                        })
                        .inner;
                    self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                    if index + 1 < rendered_end {
                        ui.add_space(row_gap);
                    }
                }
                if bottom_spacer_h > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(layout.card_max_w, bottom_spacer_h),
                        egui::Sense::hover(),
                    );
                }
            });
            ui.add_space(row_gap);
        }

        self.channels.search.items = search_items;

        if cancel {
            self.channels.cancel_channel_search();
        } else if retry {
            self.start_channel_search(&query, ctx);
        }
        chosen
    }

    pub(crate) fn channel_about_status_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> (bool, bool) {
        let loading_about = self.is_channel_about_loading();
        let about_error = self.channels.about.error.as_deref();
        let mut reload_about = false;
        let mut cancel_about = false;
        let dark_theme = self.is_dark_theme();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Informações e metadados públicos do canal")
                    .small()
                    .color(palette.neutral_status_fg),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if styled_compact_button_enabled(
                    ui,
                    !loading_about,
                    "↻ Atualizar informações",
                    false,
                    &palette,
                )
                .clicked()
                {
                    reload_about = true;
                }
            });
        });
        ui.add_space(6.0);

        if loading_about {
            ui.horizontal(|ui| {
                ui.set_max_width(card_max_w);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_ghost_button(ui, "Cancelar", &palette).clicked() {
                        cancel_about = true;
                    }
                });
            });
            ui.add_space(6.0);
            skeleton::transcript_skeleton(ui, card_max_w, dark_theme);
            ui.add_space(8.0);
        }

        if let Some(err) = about_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
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
                        if styled_compact_button(ui, "↻ Tentar novamente", false, &palette)
                            .clicked()
                        {
                            reload_about = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        (cancel_about, reload_about)
    }

    pub(crate) fn channel_about_card_ui(
        ui: &mut egui::Ui,
        page: &ChannelPage,
        about: Option<&search::ChannelAboutInfo>,
        layout: &ChannelTabLayout,
        dark_theme: bool,
    ) -> bool {
        let mut copy_link = false;
        let palette = layout.palette;
        let link_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        egui::Frame::new()
            .fill(palette.card_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_stroke))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_min_width(layout.card_max_w);
                ui.set_max_width(layout.card_max_w);
                Self::channel_about_modal_body_ui(
                    ui,
                    page,
                    about,
                    &palette,
                    link_blue,
                    &mut copy_link,
                );
            });
        copy_link
    }

    #[allow(dead_code)]
    pub(crate) fn channel_about_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) {
        let deferred = self.channel_about_tab_ui_deferred(ui, page, layout);
        self.apply_channel_about_deferred(ctx, deferred);
    }

    fn channel_about_tab_ui_deferred(
        &self,
        ui: &mut egui::Ui,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> ChannelAboutDeferred {
        let dark_theme = self.is_dark_theme();
        let (cancel_about, reload_about) =
            self.channel_about_status_ui(ui, layout.card_max_w, layout.palette);
        let copy_canonical_url = if Self::channel_about_card_ui(
            ui,
            page,
            self.channels.about.info.as_ref(),
            layout,
            dark_theme,
        ) {
            Some(page.canonical_url.clone())
        } else {
            None
        };
        ChannelAboutDeferred {
            copy_canonical_url,
            cancel_about,
            reload_about,
        }
    }

    fn apply_channel_about_deferred(
        &mut self,
        ctx: &egui::Context,
        deferred: ChannelAboutDeferred,
    ) {
        if let Some(url) = deferred.copy_canonical_url {
            self.message = match copy_text_to_clipboard(ctx, &url) {
                Ok(()) => format!("Link do canal copiado: {url}"),
                Err(error) => error,
            };
        }

        if deferred.cancel_about {
            self.cancel_channel_about();
        } else if deferred.reload_about {
            self.start_channel_about(ctx);
        }
    }

    pub(crate) fn channel_posts_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> bool {
        let palette = layout.palette;
        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let loading_more = self.is_channel_more_videos_loading();
        let posts = std::mem::take(&mut self.channels.community_posts);
        let mut load_more_clicked = false;

        if loading_more && posts.is_empty() {
            skeleton::transcript_skeleton(ui, layout.card_max_w.min(680.0), dark_theme);
            self.channels.community_posts = posts;
            return false;
        }

        if posts.is_empty() {
            channel_empty_state_card_ui(
                ui,
                layout.card_max_w,
                if pt {
                    "Nenhuma postagem da comunidade disponível para este canal."
                } else {
                    "No community posts available for this channel."
                },
                palette,
            );
            self.channels.community_posts = posts;
            return false;
        }

        let feed_w = layout.card_max_w.min(720.0);
        ui.vertical(|ui| {
            ui.set_max_width(feed_w);
            ui.spacing_mut().item_spacing.y = 14.0;
            for post in &posts {
                ui.push_id(("channel_community_post", &post.id), |ui| {
                    egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(12.0)
                        .inner_margin(egui::Margin::symmetric(16, 14))
                        .show(ui, |ui| {
                            ui.set_width((feed_w - 34.0).max(200.0));
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let author_name = if post.author.trim().is_empty() {
                                    channel_page_display_title(page)
                                } else {
                                    post.author.as_str()
                                };
                                let avatar_url =
                                    post.author_avatar.as_deref().or(page.avatar_url.as_deref());
                                let _ = Self::paint_avatar_with_cache(
                                    &mut self.avatars,
                                    ui,
                                    ctx,
                                    avatar_url,
                                    author_name,
                                    36.0,
                                    false,
                                );
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        ui.label(
                                            RichText::new(author_name)
                                                .size(14.0)
                                                .strong()
                                                .color(palette.channel_fg),
                                        );
                                        if !post.published_text.trim().is_empty() {
                                            ui.label(
                                                RichText::new(&post.published_text)
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                            );
                                        }
                                        if post.is_quiz {
                                            paint_search_metadata_badge(ui, "Quiz", &palette);
                                        }
                                    });
                                });
                            });

                            if !post.content_text.trim().is_empty() {
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new(&post.content_text)
                                        .size(14.0)
                                        .color(palette.description_fg),
                                );
                            }

                            if let Some(img_url) = post
                                .image_url
                                .as_deref()
                                .map(str::trim)
                                .filter(|u| !u.is_empty())
                            {
                                ui.add_space(10.0);
                                let img_w = ui.available_width().clamp(160.0, 560.0);
                                let img_h = (img_w * 9.0 / 16.0).clamp(120.0, 340.0).round();
                                let _ = self.thumbnails.show_rounded(
                                    ui,
                                    ctx,
                                    img_url,
                                    egui::vec2(img_w, img_h),
                                    10.0,
                                );
                            }

                            if !post.poll_choices.is_empty() {
                                ui.add_space(10.0);
                                let vote_key =
                                    ui.make_persistent_id(("channel_post_poll_vote", &post.id));
                                let voted_idx = ctx
                                    .data(|d| d.get_temp::<Option<usize>>(vote_key))
                                    .flatten();
                                let ratios =
                                    compute_poll_choice_percentages(&post.poll_choices, voted_idx);
                                let reveal_results = voted_idx.is_some();
                                let mut clicked_choice: Option<usize> = None;

                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    for (idx, choice) in post.poll_choices.iter().enumerate() {
                                        let ratio = ratios.get(idx).copied().unwrap_or(0.0);
                                        let is_selected = voted_idx == Some(idx);
                                        let row_w = ui.available_width().max(180.0);
                                        let (rect, resp) = ui.allocate_exact_size(
                                            egui::vec2(row_w, 36.0),
                                            egui::Sense::click(),
                                        );
                                        let border_color = if reveal_results && post.is_quiz {
                                            if choice.is_correct == Some(true) {
                                                palette.watched_status_fg
                                            } else if is_selected {
                                                palette.error_stroke
                                            } else {
                                                palette.card_alt_stroke
                                            }
                                        } else if is_selected {
                                            palette.info_accent_fg
                                        } else {
                                            palette.card_alt_stroke
                                        };
                                        let base_bg = if resp.hovered() {
                                            palette.header_button_bg
                                        } else {
                                            palette.card_bg
                                        };
                                        ui.painter().rect_filled(rect, 8.0, base_bg);
                                        if reveal_results && ratio > 0.005 {
                                            let bar_fill = if post.is_quiz
                                                && choice.is_correct == Some(true)
                                            {
                                                palette.watched_status_fg.gamma_multiply(0.28)
                                            } else if post.is_quiz && is_selected {
                                                palette.error_stroke.gamma_multiply(0.28)
                                            } else if is_selected {
                                                palette.info_accent_fg.gamma_multiply(0.28)
                                            } else {
                                                palette.header_button_bg.gamma_multiply(0.9)
                                            };
                                            let bar_rect = egui::Rect::from_min_size(
                                                rect.min,
                                                egui::vec2(
                                                    (rect.width() * ratio).clamp(4.0, rect.width()),
                                                    rect.height(),
                                                ),
                                            );
                                            ui.painter().rect_filled(bar_rect, 8.0, bar_fill);
                                        }
                                        ui.painter().rect_stroke(
                                            rect,
                                            8.0,
                                            egui::Stroke::new(
                                                if is_selected { 1.5 } else { 1.0 },
                                                border_color,
                                            ),
                                            egui::StrokeKind::Inside,
                                        );

                                        let prefix = if reveal_results && post.is_quiz {
                                            if choice.is_correct == Some(true) {
                                                "✓ "
                                            } else if is_selected {
                                                "✕ "
                                            } else {
                                                ""
                                            }
                                        } else if is_selected {
                                            "● "
                                        } else {
                                            ""
                                        };
                                        ui.painter().text(
                                            egui::pos2(rect.left() + 12.0, rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            format!("{prefix}{}", choice.text),
                                            egui::FontId::proportional(13.0),
                                            palette.channel_fg,
                                        );
                                        if reveal_results {
                                            let pct = (ratio * 100.0).round() as u32;
                                            ui.painter().text(
                                                egui::pos2(rect.right() - 12.0, rect.center().y),
                                                egui::Align2::RIGHT_CENTER,
                                                format!("{pct}%"),
                                                egui::FontId::proportional(12.5),
                                                palette.channel_fg,
                                            );
                                        }
                                        paint_focus_ring_if_focused(ui, &resp, 8.0);
                                        if resp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            clicked_choice = Some(idx);
                                        }
                                    }
                                });

                                if let Some(idx) = clicked_choice {
                                    ctx.data_mut(|d| {
                                        d.insert_temp(vote_key, Some(idx));
                                    });
                                }

                                if reveal_results && post.is_quiz {
                                    let explanation = post
                                        .poll_choices
                                        .iter()
                                        .find(|c| c.is_correct == Some(true))
                                        .and_then(|c| c.explanation.as_deref())
                                        .or_else(|| {
                                            voted_idx
                                                .and_then(|i| post.poll_choices.get(i))
                                                .and_then(|c| c.explanation.as_deref())
                                        })
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty());
                                    if let Some(expl) = explanation {
                                        ui.add_space(6.0);
                                        egui::Frame::new()
                                            .fill(palette.info_box_bg)
                                            .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                                            .corner_radius(8.0)
                                            .inner_margin(egui::Margin::symmetric(10, 8))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(if pt {
                                                        format!("Explicação: {expl}")
                                                    } else {
                                                        format!("Explanation: {expl}")
                                                    })
                                                    .size(12.5)
                                                    .color(palette.description_fg),
                                                );
                                            });
                                    }
                                }
                            }

                            let likes = post
                                .like_count_text
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty());
                            let comments = post
                                .comment_count_text
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty());
                            if likes.is_some() || comments.is_some() {
                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 14.0;
                                    if let Some(l) = likes {
                                        ui.label(
                                            RichText::new(format!("👍 {l}"))
                                                .size(12.5)
                                                .color(palette.neutral_status_fg),
                                        );
                                    }
                                    if let Some(c) = comments {
                                        ui.label(
                                            RichText::new(format!("💬 {c}"))
                                                .size(12.5)
                                                .color(palette.neutral_status_fg),
                                        );
                                    }
                                });
                            }
                        });
                });
            }
        });

        self.channels.community_posts = posts;

        if page.next_continuation.is_some() {
            ui.add_space(14.0);
            load_more_clicked =
                channel_continuation_footer_ui(ui, feed_w, loading_more, pt, &palette);
        }
        load_more_clicked
    }

    pub(crate) fn channel_podcasts_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> (Option<Video>, ChannelHomeDeferred) {
        let palette = layout.palette;
        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let loading_more = self.is_channel_more_videos_loading();
        let mut chosen = None;
        let mut open_playlist: Option<search::ChannelPlaylist> = None;
        let mut selected_sort_chip_continuation = None;
        let mut load_more_continuation_clicked = false;

        if !page.sort_chips.is_empty() {
            selected_sort_chip_continuation =
                channel_sort_chips_bar_ui(ui, &page.sort_chips, &palette);
            ui.add_space(12.0);
        }

        if loading_more && page.videos.is_empty() {
            skeleton::channel_tab_grid_skeleton(
                ui,
                layout.card_max_w,
                layout.compact,
                false,
                2,
                dark_theme,
            );
            return (
                None,
                ChannelHomeDeferred {
                    videos_fallback: ChannelVideosDeferred {
                        selected_sort_chip_continuation,
                        ..ChannelVideosDeferred::default()
                    },
                    ..ChannelHomeDeferred::default()
                },
            );
        }

        if page.videos.is_empty() {
            channel_empty_state_card_ui(
                ui,
                layout.card_max_w,
                if pt {
                    "Nenhum podcast listado para este canal."
                } else {
                    "No podcasts listed for this channel."
                },
                palette,
            );
            return (
                None,
                ChannelHomeDeferred {
                    videos_fallback: ChannelVideosDeferred {
                        selected_sort_chip_continuation,
                        ..ChannelVideosDeferred::default()
                    },
                    ..ChannelHomeDeferred::default()
                },
            );
        }

        let gap = 16.0;
        let row_gap = 20.0;
        let cols = if layout.compact {
            1
        } else {
            ((layout.card_max_w + gap) / 260.0).floor().clamp(2.0, 4.0) as usize
        };
        let card_w =
            ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(180.0);

        for (row_idx, row) in page.videos.chunks(cols).enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (col_idx, item) in row.iter().enumerate() {
                    let video = &item.video;
                    let meta = &item.metadata;
                    ui.push_id(("chan_podcast_card", row_idx, col_idx, &video.url), |ui| {
                        let wash_fill_idx = ui.painter().add(egui::Shape::Noop);
                        let wash_stroke_idx = ui.painter().add(egui::Shape::Noop);
                        let mut trigger_open = false;
                        let card_resp = ui.vertical(|ui| {
                            ui.set_width(card_w);
                            ui.set_max_width(card_w);
                            let thumb_h = (card_w * 9.0 / 16.0).round();
                            let thumb_resp = self.thumbnails.show_cover_rounded(
                                ui,
                                ctx,
                                &video.thumbnail,
                                egui::vec2(card_w, thumb_h),
                                12.0,
                            );
                            paint_focus_ring_if_focused(ui, &thumb_resp, 12.0);
                            let ep_badge = meta
                                .playlist_count
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .unwrap_or("Podcast");
                            paint_playlist_badge_overlay(ui, thumb_resp.rect, ep_badge);
                            if thumb_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                trigger_open = true;
                            }

                            ui.add_space(8.0);
                            let galley = layout_two_line_clamped_galley(
                                ui,
                                &video.title,
                                card_w,
                                egui::FontId::proportional(14.5),
                                palette.channel_fg,
                            );
                            let (t_rect, t_resp) = ui.allocate_exact_size(
                                egui::vec2(card_w, galley.size().y.max(18.0)),
                                egui::Sense::click(),
                            );
                            ui.painter().galley(t_rect.min, galley, palette.channel_fg);
                            paint_focus_ring_if_focused(ui, &t_resp, 6.0);
                            if t_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(&video.title)
                                .clicked()
                            {
                                trigger_open = true;
                            }

                            if let Some(ep_count) = meta
                                .playlist_count
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                            {
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(ep_count)
                                        .size(12.0)
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            ui.add_space(6.0);
                            if styled_compact_button(
                                ui,
                                if pt {
                                    "▶ Reproduzir tudo"
                                } else {
                                    "▶ Play all"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                trigger_open = true;
                            }
                        });

                        let full_card_hovered = ui
                            .input(|i| i.pointer.hover_pos())
                            .is_some_and(|pos| card_resp.response.rect.contains(pos));
                        let wash_t = ctx.animate_bool_with_time(
                            ui.make_persistent_id(("chan_podcast_wash", &video.url)),
                            full_card_hovered,
                            0.14,
                        );
                        let (wf, ws) =
                            card_hover_wash_shapes(card_resp.response.rect, 12.0, wash_t, &palette);
                        ui.painter().set(wash_fill_idx, wf);
                        ui.painter().set(wash_stroke_idx, ws);

                        if trigger_open {
                            if let Some(pid) = extract_playlist_id_from_url(&video.url) {
                                let count_num = meta.playlist_count.as_deref().and_then(|s| {
                                    let d: String =
                                        s.chars().filter(|c| c.is_ascii_digit()).collect();
                                    d.parse::<u64>().ok()
                                });
                                open_playlist = Some(search::ChannelPlaylist {
                                    id: pid,
                                    title: video.title.clone(),
                                    url: video.url.clone(),
                                    thumbnail: video.thumbnail.clone(),
                                    item_count: count_num,
                                    description: meta.description.clone(),
                                    privacy: crate::playlist_sync::PlaylistPrivacy::Unknown,
                                });
                            } else {
                                chosen = Some(video.clone());
                            }
                        }
                    });
                }
            });
            ui.add_space(row_gap);
        }

        if page.next_continuation.is_some() {
            load_more_continuation_clicked =
                channel_continuation_footer_ui(ui, layout.card_max_w, loading_more, pt, &palette);
        }

        (
            chosen,
            ChannelHomeDeferred {
                open_playlist,
                videos_fallback: ChannelVideosDeferred {
                    selected_sort_chip_continuation,
                    load_more_continuation_clicked,
                    ..ChannelVideosDeferred::default()
                },
                ..ChannelHomeDeferred::default()
            },
        )
    }

    pub(crate) fn channel_releases_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> (Option<Video>, ChannelHomeDeferred) {
        let palette = layout.palette;
        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let loading_more = self.is_channel_more_videos_loading();
        let mut chosen = None;
        let mut open_playlist: Option<search::ChannelPlaylist> = None;
        let mut selected_sort_chip_continuation = None;
        let mut load_more_continuation_clicked = false;

        if !page.sort_chips.is_empty() {
            selected_sort_chip_continuation =
                channel_sort_chips_bar_ui(ui, &page.sort_chips, &palette);
            ui.add_space(12.0);
        }

        if loading_more && page.videos.is_empty() {
            skeleton::channel_tab_grid_skeleton(
                ui,
                layout.card_max_w,
                layout.compact,
                false,
                2,
                dark_theme,
            );
            return (
                None,
                ChannelHomeDeferred {
                    videos_fallback: ChannelVideosDeferred {
                        selected_sort_chip_continuation,
                        ..ChannelVideosDeferred::default()
                    },
                    ..ChannelHomeDeferred::default()
                },
            );
        }

        if page.videos.is_empty() {
            channel_empty_state_card_ui(
                ui,
                layout.card_max_w,
                if pt {
                    "Nenhum lançamento listado para este canal."
                } else {
                    "No releases listed for this channel."
                },
                palette,
            );
            return (
                None,
                ChannelHomeDeferred {
                    videos_fallback: ChannelVideosDeferred {
                        selected_sort_chip_continuation,
                        ..ChannelVideosDeferred::default()
                    },
                    ..ChannelHomeDeferred::default()
                },
            );
        }

        let gap = 16.0;
        let row_gap = 20.0;
        let cols = if layout.compact {
            2
        } else {
            ((layout.card_max_w + gap) / 210.0).floor().clamp(2.0, 5.0) as usize
        };
        let card_w =
            ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(140.0);

        for (row_idx, row) in page.videos.chunks(cols).enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (col_idx, item) in row.iter().enumerate() {
                    let video = &item.video;
                    let meta = &item.metadata;
                    ui.push_id(("chan_release_card", row_idx, col_idx, &video.url), |ui| {
                        let wash_fill_idx = ui.painter().add(egui::Shape::Noop);
                        let wash_stroke_idx = ui.painter().add(egui::Shape::Noop);
                        let mut trigger_open = false;
                        let card_resp = ui.vertical(|ui| {
                            ui.set_width(card_w);
                            ui.set_max_width(card_w);
                            // Square 1:1 album cover art.
                            let cover_resp = self.thumbnails.show_cover_rounded(
                                ui,
                                ctx,
                                &video.thumbnail,
                                egui::vec2(card_w, card_w),
                                10.0,
                            );
                            paint_focus_ring_if_focused(ui, &cover_resp, 10.0);
                            let hover_t = ctx.animate_bool_with_time(
                                ui.make_persistent_id(("chan_release_hover", &video.url)),
                                cover_resp.hovered(),
                                0.14,
                            );
                            paint_card_hover_highlight(
                                ui,
                                cover_resp.rect,
                                10.0,
                                hover_t,
                                &palette,
                            );
                            if cover_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                trigger_open = true;
                            }

                            ui.add_space(8.0);
                            let galley = layout_two_line_clamped_galley(
                                ui,
                                &video.title,
                                card_w,
                                egui::FontId::proportional(14.0),
                                palette.channel_fg,
                            );
                            let (t_rect, t_resp) = ui.allocate_exact_size(
                                egui::vec2(card_w, galley.size().y.max(18.0)),
                                egui::Sense::click(),
                            );
                            ui.painter().galley(t_rect.min, galley, palette.channel_fg);
                            paint_focus_ring_if_focused(ui, &t_resp, 6.0);
                            if t_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(&video.title)
                                .clicked()
                            {
                                trigger_open = true;
                            }

                            if let Some(meta_line) = format_release_meta_line(Some(meta), pt) {
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(meta_line)
                                        .size(12.0)
                                        .color(palette.neutral_status_fg),
                                );
                            }
                        });

                        let full_card_hovered = ui
                            .input(|i| i.pointer.hover_pos())
                            .is_some_and(|pos| card_resp.response.rect.contains(pos));
                        let wash_t = ctx.animate_bool_with_time(
                            ui.make_persistent_id(("chan_release_wash", &video.url)),
                            full_card_hovered,
                            0.14,
                        );
                        let (wf, ws) =
                            card_hover_wash_shapes(card_resp.response.rect, 10.0, wash_t, &palette);
                        ui.painter().set(wash_fill_idx, wf);
                        ui.painter().set(wash_stroke_idx, ws);

                        if trigger_open {
                            if let Some(pid) = extract_playlist_id_from_url(&video.url) {
                                let count_num = meta.playlist_count.as_deref().and_then(|s| {
                                    let d: String =
                                        s.chars().filter(|c| c.is_ascii_digit()).collect();
                                    d.parse::<u64>().ok()
                                });
                                open_playlist = Some(search::ChannelPlaylist {
                                    id: pid,
                                    title: video.title.clone(),
                                    url: video.url.clone(),
                                    thumbnail: video.thumbnail.clone(),
                                    item_count: count_num,
                                    description: meta.description.clone(),
                                    privacy: crate::playlist_sync::PlaylistPrivacy::Unknown,
                                });
                            } else {
                                chosen = Some(video.clone());
                            }
                        }
                    });
                }
            });
            ui.add_space(row_gap);
        }

        if page.next_continuation.is_some() {
            load_more_continuation_clicked =
                channel_continuation_footer_ui(ui, layout.card_max_w, loading_more, pt, &palette);
        }

        (
            chosen,
            ChannelHomeDeferred {
                open_playlist,
                videos_fallback: ChannelVideosDeferred {
                    selected_sort_chip_continuation,
                    load_more_continuation_clicked,
                    ..ChannelVideosDeferred::default()
                },
                ..ChannelHomeDeferred::default()
            },
        )
    }

    pub(crate) fn channel_about_modal_body_ui(
        ui: &mut egui::Ui,
        page: &ChannelPage,
        about: Option<&search::ChannelAboutInfo>,
        palette: &ThemePalette,
        link_blue: Color32,
        copy_link: &mut bool,
    ) {
        Self::channel_about_modal_body_ui_localized(
            ui, page, about, palette, link_blue, copy_link, true,
        );
    }

    fn channel_about_modal_body_ui_localized(
        ui: &mut egui::Ui,
        page: &ChannelPage,
        about: Option<&search::ChannelAboutInfo>,
        palette: &ThemePalette,
        link_blue: Color32,
        copy_link: &mut bool,
        pt: bool,
    ) {
        let full_desc = about
            .and_then(|a| clean_description(a.description.as_deref()))
            .or_else(|| clean_description(page.description.as_deref()));

        ui.label(
            RichText::new(if pt { "Descrição" } else { "Description" })
                .size(16.0)
                .strong()
                .color(palette.channel_fg),
        );
        ui.add_space(6.0);
        if let Some(desc) = full_desc {
            ui.label(RichText::new(desc).size(13.5).color(palette.description_fg));
        } else {
            ui.label(
                RichText::new(if pt {
                    "Nenhuma descrição pública informada."
                } else {
                    "No public description provided."
                })
                .size(13.0)
                .color(palette.neutral_status_fg),
            );
        }

        let links = if !page.links.is_empty() {
            &page.links
        } else if let Some(info) = about {
            &info.links
        } else {
            &page.links
        };
        if !links.is_empty() {
            ui.add_space(16.0);
            ui.label(
                RichText::new("Links")
                    .size(16.0)
                    .strong()
                    .color(palette.channel_fg),
            );
            ui.add_space(8.0);
            for link in links {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    paint_link_chain_icon(ui, palette.neutral_status_fg);
                    ui.vertical(|ui| {
                        if !link.title.trim().is_empty() {
                            ui.label(
                                RichText::new(&link.title)
                                    .size(13.5)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                        }
                        let link_resp = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(&link.url).size(12.5).color(link_blue),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        paint_focus_ring_if_focused(ui, &link_resp, 4.0);
                        if link_resp.clicked() {
                            open_external_url(&link.url);
                        }
                    });
                });
                ui.add_space(6.0);
            }
        }

        ui.add_space(16.0);
        ui.label(
            RichText::new(if pt {
                "Detalhes do canal"
            } else {
                "Channel details"
            })
            .size(16.0)
            .strong()
            .color(palette.channel_fg),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            paint_link_chain_icon(ui, palette.neutral_status_fg);
            ui.label(
                RichText::new(&page.canonical_url)
                    .size(13.0)
                    .color(palette.channel_fg),
            );
        });
        ui.add_space(6.0);

        if let Some(subs) = page
            .subscriber_count_text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                page.subscriber_count
                    .or_else(|| about.and_then(|a| a.subscriber_count))
                    .map(format_subscribers_pt)
            })
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(subs).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        if let Some(vids) = page
            .video_count_text
            .as_deref()
            .or_else(|| about.and_then(|a| a.video_count_text.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(vids).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        if let Some(views_text) = page
            .view_count_text
            .as_deref()
            .or_else(|| about.and_then(|a| a.view_count_text.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                page.view_count
                    .or_else(|| about.and_then(|a| a.view_count))
                    .map(|v| {
                        if pt {
                            format!("{} visualizações", format_view_count(v))
                        } else {
                            format!("{} views", format_view_count(v))
                        }
                    })
            })
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(
                    RichText::new(views_text)
                        .size(13.0)
                        .color(palette.channel_fg),
                );
            });
            ui.add_space(6.0);
        }

        if let Some(joined) = page
            .joined_date_text
            .as_deref()
            .or_else(|| about.and_then(|a| a.joined_date_text.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(joined).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        if let Some(country) = page
            .country
            .as_deref()
            .or_else(|| about.and_then(|a| a.country.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_globe_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(country).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        let tags = if !page.tags.is_empty() {
            &page.tags
        } else if let Some(about_info) = about {
            &about_info.tags
        } else {
            &page.tags
        };
        if !tags.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Tags: {}", tags.join(", ")))
                    .size(12.0)
                    .color(palette.neutral_status_fg),
            );
        }

        ui.add_space(12.0);
        let share_btn = egui::Button::new(
            RichText::new(if pt {
                "Compartilhar canal"
            } else {
                "Share channel"
            })
            .size(13.0)
            .strong()
            .color(palette.header_button_fg),
        )
        .fill(palette.header_button_bg)
        .stroke(egui::Stroke::NONE)
        .min_size(egui::vec2(0.0, 34.0))
        .corner_radius(17);
        let share_resp = ui
            .add(share_btn)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        paint_focus_ring_if_focused(ui, &share_resp, 17.0);
        if share_resp.clicked() {
            *copy_link = true;
        }
    }

    pub(crate) fn channel_about_modal_ui(&mut self, ctx: &egui::Context) {
        let mut modal_open = self.channels.about_modal_open
            && self.channels.view_open
            && self.channels.page.is_some();
        let screen = ctx.content_rect();
        let modal_w = (screen.width() - 48.0).clamp(320.0, 520.0);
        let modal_body_h = (screen.height() - 140.0).clamp(220.0, 420.0);
        let pt = self.ui_pt();

        if !modal_open {
            // Allow `app_modal_window` scrim fade-out to settle cleanly.
            app_modal_window(
                ctx,
                egui::Id::new("channel_about_modal_window"),
                "",
                &mut modal_open,
                modal_w,
                |_| {},
            );
            return;
        }

        let Some(page) = self.channels.page.take() else {
            return;
        };
        let palette = self.palette();
        let dark_theme = self.is_dark_theme();
        let link_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let loading_about = self.is_channel_about_loading();
        let about = self.channels.about.info.take();
        let mut copy_link = false;
        let title = channel_page_display_title(&page);

        app_modal_window(
            ctx,
            egui::Id::new("channel_about_modal_window"),
            title,
            &mut modal_open,
            modal_w,
            |ui| {
                if loading_about {
                    skeleton::transcript_skeleton(ui, ui.available_width(), dark_theme);
                    ui.add_space(10.0);
                }
                egui::ScrollArea::vertical()
                    .id_salt("channel_about_modal_scroll")
                    .max_height(modal_body_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        Self::channel_about_modal_body_ui_localized(
                            ui,
                            &page,
                            about.as_ref(),
                            &palette,
                            link_blue,
                            &mut copy_link,
                            pt,
                        );
                    });
            },
        );

        let copy_url = copy_link.then(|| page.canonical_url.clone());
        self.channels.page = Some(page);
        self.channels.about.info = about;

        if let Some(url) = copy_url {
            self.message = match copy_text_to_clipboard(ctx, &url) {
                Ok(()) => {
                    if pt {
                        format!("Link do canal copiado: {url}")
                    } else {
                        format!("Channel link copied: {url}")
                    }
                }
                Err(error) => error,
            };
        }
        if !modal_open {
            self.channels.about_modal_open = false;
        }
    }

    pub(crate) fn channel_load_status_ui(
        &self,
        ui: &mut egui::Ui,
        loading: bool,
        error: Option<&str>,
        retry_target: Option<&ChannelTarget>,
        palette: ThemePalette,
    ) -> (bool, Option<ChannelTarget>) {
        let mut cancel_initial = false;
        let mut reload_target = None;

        if loading {
            ui.horizontal(|ui| {
                ui.set_min_width(ui.available_width());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_ghost_button(ui, "Cancelar", &palette)
                        .on_hover_text("Cancelar o carregamento em andamento do canal")
                        .clicked()
                    {
                        cancel_initial = true;
                    }
                });
            });
            ui.add_space(4.0);
        }

        if let Some(error_msg) = error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar canal: {error_msg}"))
                                .color(palette.error_banner_fg),
                        );
                        if let Some(target) = retry_target
                            && styled_compact_button(ui, "↻ Tentar novamente", false, &palette)
                                .on_hover_text("Tentar buscar novamente os dados do canal")
                                .clicked()
                        {
                            reload_target = Some(target.clone());
                        }
                    });
                });
            ui.add_space(6.0);
        }

        (cancel_initial, reload_target)
    }

    pub(crate) fn channel_playlists_empty_state_ui(
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        channel_empty_state_card_ui(
            ui,
            card_max_w,
            "Nenhuma playlist listada para este canal.",
            palette,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        channel_dynamic_tab_label, channel_grid_visible_row_range, channel_playlist_row_height,
        channel_short_row_height, channel_subscribe_menu_labels, channel_video_row_height,
        extra_channel_tab_endpoints,
    };
    use crate::innertube::{ChannelTabEndpoint, ChannelTabKind};

    #[test]
    fn channel_subscribe_menu_labels_cover_pt_and_en_states() {
        let auth_en = channel_subscribe_menu_labels(false, false, false);
        assert_eq!(auth_en.local_follow_toggle, "Follow locally (RSS)");
        assert_eq!(auth_en.unsubscribe_item, "Unsubscribe");
        assert_eq!(auth_en.confirm_button, "Unsubscribe");
        assert_eq!(auth_en.cancel_button, "Cancel");

        let auth_followed_pt = channel_subscribe_menu_labels(false, true, true);
        assert_eq!(
            auth_followed_pt.local_follow_toggle,
            "Seguindo localmente (RSS) ✓"
        );
        assert_eq!(auth_followed_pt.unsubscribe_item, "Cancelar inscrição");
        assert_eq!(auth_followed_pt.confirm_button, "Cancelar inscrição");
        assert_eq!(auth_followed_pt.cancel_button, "Manter");

        let local_en = channel_subscribe_menu_labels(true, true, false);
        assert_eq!(local_en.unsubscribe_item, "Unfollow");
        assert_eq!(local_en.confirm_button, "Confirm");
    }

    #[test]
    fn channel_grid_visible_row_range_skips_offscreen_rows_with_invariant_height() {
        let row_h = channel_video_row_height(280.0);
        let gap = 18.0;
        let total_rows = 25;
        let expected_total = total_rows as f32 * row_h + (total_rows - 1) as f32 * gap;

        let (range, top_h, bottom_h) =
            channel_grid_visible_row_range(total_rows, row_h, gap, 320.0, 1600.0, 2300.0, 2);
        assert!(range.start > 0);
        assert!(range.end < total_rows);
        let rendered_rows = range.end - range.start;
        let rendered_h =
            rendered_rows as f32 * row_h + rendered_rows.saturating_sub(1) as f32 * gap;
        assert!((top_h + rendered_h + bottom_h - expected_total).abs() < 0.01);
        assert!(channel_short_row_height(190.0) > row_h);
        assert!(channel_playlist_row_height(260.0) > 150.0);
    }

    #[test]
    fn channel_grid_visible_row_range_handles_empty_partial_bottom_clamp_and_overscan() {
        assert_eq!(
            channel_grid_visible_row_range(0, 240.0, 16.0, 0.0, 0.0, 800.0, 2),
            (0..0, 0.0, 0.0)
        );

        let total_rows = 10;
        let row_h = 200.0;
        let gap = 16.0;
        let expected_total = total_rows as f32 * row_h + (total_rows - 1) as f32 * gap;

        // Partial viewport at the top with overscan=0 vs overscan=2.
        let (r0, t0, b0) =
            channel_grid_visible_row_range(total_rows, row_h, gap, 0.0, 0.0, 350.0, 0);
        assert_eq!(r0, 0..2);
        assert_eq!(t0, 0.0);
        let h0 = (r0.end - r0.start) as f32 * row_h + (r0.end - r0.start - 1) as f32 * gap;
        assert!((t0 + h0 + b0 - expected_total).abs() < 0.01);

        let (r2, t2, b2) =
            channel_grid_visible_row_range(total_rows, row_h, gap, 0.0, 0.0, 350.0, 2);
        assert_eq!(r2, 0..4);
        assert_eq!(t2, 0.0);
        let h2 = (r2.end - r2.start) as f32 * row_h + (r2.end - r2.start - 1) as f32 * gap;
        assert!((t2 + h2 + b2 - expected_total).abs() < 0.01);

        // Bottom clamp when scrolled past the end of the grid.
        let (range, top_h, bottom_h) =
            channel_grid_visible_row_range(total_rows, row_h, gap, 0.0, 5000.0, 5800.0, 2);
        assert_eq!(range, 8..10);
        assert!(top_h > 0.0);
        assert_eq!(bottom_h, 0.0);
        let hb =
            (range.end - range.start) as f32 * row_h + (range.end - range.start - 1) as f32 * gap;
        assert!((top_h + hb + bottom_h - expected_total).abs() < 0.01);
    }

    #[test]
    fn extra_channel_tab_endpoints_filters_core_tabs_and_deduplicates() {
        let ep = |title: &str, browse_id: &str, kind: ChannelTabKind| ChannelTabEndpoint {
            title: title.into(),
            browse_id: browse_id.into(),
            params: Some("p".into()),
            selected: false,
            kind,
        };
        let endpoints = vec![
            ep("Home", "UC123", ChannelTabKind::Home),
            ep("Videos", "UC123", ChannelTabKind::Videos),
            ep("Shorts", "UC123", ChannelTabKind::Shorts),
            ep("Live", "UC123", ChannelTabKind::Live),
            ep("Playlists", "UC123", ChannelTabKind::Playlists),
            ep("Search", "UC123", ChannelTabKind::Search),
            ep("Posts", "UC123", ChannelTabKind::Posts),
            ep("Posts duplicate", "UC123", ChannelTabKind::Posts),
            ep("Podcasts", "UC123", ChannelTabKind::Podcasts),
            ep("Releases", "UC123", ChannelTabKind::Releases),
            ep("Courses", "UC123", ChannelTabKind::Courses),
            ep("Custom", "UC123", ChannelTabKind::Other("Custom".into())),
            ep("custom", "UC123", ChannelTabKind::Other("custom".into())),
            ep("About", "UC123", ChannelTabKind::Other("About".into())),
            ep("   ", "UC123", ChannelTabKind::Store),
            ep("Store", "", ChannelTabKind::Store),
        ];

        let extras = extra_channel_tab_endpoints(&endpoints);
        let titles: Vec<&str> = extras.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Posts", "Podcasts", "Releases", "Courses", "Custom"]
        );
    }

    #[test]
    fn channel_dynamic_tab_label_prefers_title_and_falls_back_bilingually() {
        let named = ChannelTabEndpoint {
            title: "Community".into(),
            browse_id: "UC123".into(),
            params: None,
            selected: false,
            kind: ChannelTabKind::Posts,
        };
        assert_eq!(channel_dynamic_tab_label(&named, true), "Community");
        assert_eq!(channel_dynamic_tab_label(&named, false), "Community");

        let untitled_posts = ChannelTabEndpoint {
            title: "  ".into(),
            browse_id: "UC123".into(),
            params: None,
            selected: false,
            kind: ChannelTabKind::Posts,
        };
        assert_eq!(
            channel_dynamic_tab_label(&untitled_posts, true),
            "Postagens"
        );
        assert_eq!(channel_dynamic_tab_label(&untitled_posts, false), "Posts");

        let untitled_releases = ChannelTabEndpoint {
            title: String::new(),
            browse_id: "UC123".into(),
            params: None,
            selected: false,
            kind: ChannelTabKind::Releases,
        };
        assert_eq!(
            channel_dynamic_tab_label(&untitled_releases, true),
            "Lan\u{00e7}amentos"
        );
        assert_eq!(
            channel_dynamic_tab_label(&untitled_releases, false),
            "Releases"
        );
    }

    #[test]
    fn carousel_offset_steps_and_clamps_within_bounds() {
        use super::step_carousel_offset;

        assert_eq!(step_carousel_offset(0.0, 260.0, 800.0), 260.0);
        assert_eq!(step_carousel_offset(700.0, 260.0, 800.0), 800.0);
        assert_eq!(step_carousel_offset(100.0, -260.0, 800.0), 0.0);
        assert_eq!(step_carousel_offset(50.0, 100.0, -20.0), 0.0);
    }

    #[test]
    fn upcoming_countdown_formats_bilingual_days_hours_minutes_and_imminent() {
        use super::format_upcoming_countdown;

        assert_eq!(
            format_upcoming_countdown(1_000, 1_000, true),
            "Iniciando em breve"
        );
        assert_eq!(
            format_upcoming_countdown(1_000, 1_000, false),
            "Starting soon"
        );
        assert_eq!(
            format_upcoming_countdown(1_000 + 2 * 3_600 + 15 * 60, 1_000, true),
            "Estreia em 2h 15m"
        );
        assert_eq!(
            format_upcoming_countdown(1_000 + 2 * 3_600 + 15 * 60, 1_000, false),
            "Premieres in 2h 15m"
        );
        assert_eq!(
            format_upcoming_countdown(1_000 + 2 * 86_400 + 5 * 3_600, 1_000, false),
            "Premieres in 2d 5h"
        );
        assert_eq!(
            format_upcoming_countdown(1_000 + 12 * 60, 1_000, true),
            "Estreia em 12m"
        );
        assert_eq!(
            format_upcoming_countdown(1_000 + 25, 1_000, false),
            "Premieres in <1m"
        );
    }

    #[test]
    fn poll_choice_percentages_use_server_ratios_and_local_vote_fallback() {
        use super::compute_poll_choice_percentages;
        use crate::innertube::ChannelPollChoice;

        let with_ratios = vec![
            ChannelPollChoice {
                text: "Rust".into(),
                vote_ratio: Some(0.75),
                is_correct: Some(true),
                explanation: Some("Memory safe".into()),
            },
            ChannelPollChoice {
                text: "Other".into(),
                vote_ratio: Some(0.25),
                is_correct: Some(false),
                explanation: None,
            },
        ];
        assert_eq!(
            compute_poll_choice_percentages(&with_ratios, Some(1)),
            vec![0.75, 0.25]
        );

        let local_only = vec![
            ChannelPollChoice {
                text: "Option A".into(),
                vote_ratio: None,
                is_correct: None,
                explanation: None,
            },
            ChannelPollChoice {
                text: "Option B".into(),
                vote_ratio: None,
                is_correct: None,
                explanation: None,
            },
        ];
        assert_eq!(
            compute_poll_choice_percentages(&local_only, None),
            vec![0.0, 0.0]
        );
        assert_eq!(
            compute_poll_choice_percentages(&local_only, Some(1)),
            vec![0.0, 1.0]
        );
    }

    #[test]
    fn release_meta_line_formats_year_and_type_bilingually() {
        use super::format_release_meta_line;
        use crate::search::VideoMetadata;

        let album = VideoMetadata {
            playlist_count: Some("12 tracks".into()),
            published_time_text: Some("2025".into()),
            ..VideoMetadata::default()
        };
        assert_eq!(
            format_release_meta_line(Some(&album), true).as_deref(),
            Some("Álbum • 2025 • 12 tracks")
        );
        assert_eq!(
            format_release_meta_line(Some(&album), false).as_deref(),
            Some("Album • 2025 • 12 tracks")
        );

        let single = VideoMetadata {
            badges: vec!["Single".into()],
            published_time_text: Some("2026".into()),
            ..VideoMetadata::default()
        };
        assert_eq!(
            format_release_meta_line(Some(&single), false).as_deref(),
            Some("Single • 2026")
        );
        assert_eq!(format_release_meta_line(None, false), None);
    }
}
