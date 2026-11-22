use super::{empty_state_card_ui, paint_library_card_hover_glow, paint_section_hairline};
use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;
use std::sync::Arc;

fn history_shorts_shelf_estimated_height(feed_col_w: f32) -> f32 {
    let cols: usize = if feed_col_w >= 540.0 { 3 } else { 2 };
    let card_gap = 12.0;
    let card_w = ((feed_col_w - card_gap * (cols.saturating_sub(1) as f32)) / cols as f32)
        .clamp(130.0, 236.0);
    let card_h = (card_w * 16.0 / 9.0).round();
    // Header (24 + 10) + 9:16 card + title & views footer (~64).
    card_h + 98.0
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryDisplayCacheKey {
    pub(crate) filter: history::HistoryFilter,
    pub(crate) search_query: String,
    pub(crate) pt: bool,
    pub(crate) local_len: usize,
    pub(crate) local_first_sig: Option<(String, u64, Option<u64>, Option<u64>)>,
    pub(crate) local_last_sig: Option<(String, u64)>,
    pub(crate) local_minute_bucket: u64,
    pub(crate) remote_loaded: bool,
    pub(crate) remote_page_count: usize,
    pub(crate) remote_sections_len: usize,
    pub(crate) remote_items_len: usize,
    pub(crate) remote_seen_len: usize,
    pub(crate) remote_first_id: Option<String>,
    pub(crate) remote_last_id: Option<String>,
}

#[derive(Clone)]
pub(crate) struct CachedHistoryDisplay {
    pub(crate) key: HistoryDisplayCacheKey,
    pub(crate) sections: Vec<HistoryDisplaySection>,
    pub(crate) from_remote: bool,
}

pub(crate) fn resolve_history_display_cache<F>(
    existing: Option<&Arc<CachedHistoryDisplay>>,
    key: HistoryDisplayCacheKey,
    build: F,
) -> Arc<CachedHistoryDisplay>
where
    F: FnOnce() -> (Vec<HistoryDisplaySection>, bool),
{
    if let Some(cached) = existing
        && cached.key == key
    {
        return Arc::clone(cached);
    }
    let (sections, from_remote) = build();
    Arc::new(CachedHistoryDisplay {
        key,
        sections,
        from_remote,
    })
}

#[derive(Clone)]
enum CompactHistoryRow {
    Header(history::DateGroup),
    Item(Box<HistoryEntry>),
}

#[derive(Clone, PartialEq, Eq)]
struct CompactHistoryCacheKey {
    filter: history::HistoryFilter,
    search_query: String,
    local_len: usize,
    local_first_sig: Option<(String, u64, Option<u64>, Option<u64>)>,
    local_last_sig: Option<(String, u64)>,
    local_minute_bucket: u64,
}

#[derive(Clone)]
struct CachedCompactHistory {
    key: CompactHistoryCacheKey,
    rows: Vec<CompactHistoryRow>,
}

#[derive(Default)]
struct HistoryRowAction {
    play_clicked: bool,
    remove_clicked: bool,
    enqueue_clicked: bool,
    toggle_watch_later: bool,
    open_save_modal: bool,
    share_clicked: bool,
    download_format: Option<downloads::DownloadFormat>,
    open_channel: Option<ChannelTarget>,
}

#[derive(Default)]
struct HistoryRailOutcome {
    clear_clicked: bool,
    confirm_clear_clicked: bool,
    cancel_clear_clicked: bool,
    toggle_pause_clicked: bool,
    manage_clicked: bool,
    sync_now_clicked: bool,
    connect_sync_clicked: bool,
    external_url: Option<&'static str>,
}

#[derive(Default)]
struct HistoryShortsShelfOutcome {
    play_video: Option<Video>,
    remove_video_id: Option<String>,
    enqueue_video: Option<Video>,
    toggle_watch_later: Option<Video>,
    open_save_modal: Option<Video>,
    open_share_modal: Option<Video>,
    download_req: Option<(Video, downloads::DownloadFormat)>,
}

#[derive(Clone)]
pub(crate) struct HistoryDisplaySection {
    date_label: String,
    entries: Vec<HistoryEntry>,
    all_short_runs: usize,
    all_regular_count: usize,
    all_short_gaps: usize,
    all_regular_gaps: usize,
}

impl HistoryDisplaySection {
    pub(crate) fn from_entries(date_label: String, entries: Vec<HistoryEntry>) -> Self {
        let mut all_short_runs = 0usize;
        let mut all_regular_count = 0usize;
        let mut all_short_gaps = 0usize;
        let mut all_regular_gaps = 0usize;
        let mut scan = 0usize;
        while scan < entries.len() {
            if entries[scan].is_short() {
                while scan < entries.len() && entries[scan].is_short() {
                    scan += 1;
                }
                all_short_runs += 1;
                if scan < entries.len() {
                    all_short_gaps += 1;
                }
            } else {
                scan += 1;
                all_regular_count += 1;
                if scan < entries.len() {
                    all_regular_gaps += 1;
                }
            }
        }
        Self {
            date_label,
            entries,
            all_short_runs,
            all_regular_count,
            all_short_gaps,
            all_regular_gaps,
        }
    }

    pub(crate) fn estimated_body_height(
        &self,
        active_filter: history::HistoryFilter,
        thumb_h: f32,
        est_shelf_h: f32,
        row_spacing_y: f32,
    ) -> f32 {
        match active_filter {
            history::HistoryFilter::Shorts => est_shelf_h,
            history::HistoryFilter::All => {
                self.all_short_runs as f32 * est_shelf_h
                    + self.all_short_gaps as f32 * (18.0 + row_spacing_y)
                    + self.all_regular_count as f32 * thumb_h
                    + self.all_regular_gaps as f32 * (16.0 + row_spacing_y)
            }
            _ => {
                let n = self.entries.len();
                n as f32 * thumb_h + n.saturating_sub(1) as f32 * (16.0 + row_spacing_y)
            }
        }
    }
}

fn history_entry_action_menu_ui(
    ui: &mut egui::Ui,
    in_watch_later: bool,
    pt: bool,
    palette: &ThemePalette,
    action: &mut HistoryRowAction,
) {
    ui.set_min_width(248.0);
    ui.spacing_mut().item_spacing.y = 2.0;

    if search_menu_item_ui(
        ui,
        SearchMenuIcon::Queue,
        if pt {
            "Adicionar \u{00e0} fila"
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
            "Remover de \"Assistir mais tarde\""
        } else {
            "Remove from \"Watch later\""
        }
    } else if pt {
        "Salvar em \"Assistir mais tarde\""
    } else {
        "Save to \"Watch later\""
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

    ui.menu_button(
        if pt {
            "\u{2b07} Baixar"
        } else {
            "\u{2b07} Download"
        },
        |ui| {
            for fmt in downloads::DownloadFormat::ALL {
                let lbl = if pt { fmt.label_pt() } else { fmt.label_en() };
                if ui.button(lbl).clicked() {
                    action.download_format = Some(fmt);
                    ui.close();
                }
            }
        },
    );

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
        SearchMenuIcon::Trash,
        if pt {
            "Remover do hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o"
        } else {
            "Remove from watch history"
        },
        palette,
    )
    .clicked()
    {
        action.remove_clicked = true;
        ui.close();
    }
}

fn history_rail_action_button(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    palette: &ThemePalette,
    paint_icon: impl FnOnce(&egui::Painter, egui::Pos2, Color32),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 19.0, palette.header_button_bg);
    }
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 18.0, rect.center().y),
        palette.header_button_fg,
    );
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 40.0, rect.top()),
        egui::pos2(rect.right() - 8.0, rect.bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(label)
                        .size(14.0)
                        .strong()
                        .color(palette.header_button_fg),
                )
                .truncate()
                .selectable(false),
            );
        });
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl FastTube {
    pub(crate) fn history_entry_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        entry: &HistoryEntry,
        compact: bool,
        is_playing: bool,
    ) -> (bool, bool, bool) {
        let mut play_clicked = false;
        let mut remove_clicked = false;
        let mut enqueue_clicked = false;
        let pt = self.ui_pt();
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
        let card_height: f32 = if compact { 82.0 } else { 86.0 };
        let inner_height: f32 = card_height - 16.0;
        let thumb_h: f32 = (inner_height - 2.0).max(48.0);
        let thumb_w: f32 = (thumb_h * 16.0 / 9.0).round();
        let thumb_url = format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", entry.video_id);
        let progress_ratio = entry.duration_ms.filter(|&d| d > 0).map(|d| {
            if history::is_near_end(entry.position_ms, Some(d)) {
                1.0_f32
            } else {
                (entry.position_ms as f32 / d as f32).clamp(0.0, 1.0)
            }
        });
        let ctx = ui.ctx().clone();
        let frame_resp = egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(inner_height);
                ui.set_max_height(inner_height);
                ui.horizontal_top(|ui| {
                    let thumb_resp = self.thumbnails.show_rounded(
                        ui,
                        &ctx,
                        &thumb_url,
                        egui::vec2(thumb_w, thumb_h),
                        8.0,
                    );
                    if let Some(ratio) = progress_ratio {
                        let bar_h = 3.0;
                        let bar_rect = egui::Rect::from_min_max(
                            egui::pos2(thumb_resp.rect.left(), thumb_resp.rect.bottom() - bar_h),
                            thumb_resp.rect.max,
                        );
                        ui.painter().rect_filled(
                            bar_rect,
                            0.0,
                            Color32::from_rgba_unmultiplied(255, 255, 255, 70),
                        );
                        let fill_w = (bar_rect.width() * ratio.clamp(0.0, 1.0)).max(2.0);
                        let fill_rect =
                            egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_w, bar_h));
                        ui.painter()
                            .rect_filled(fill_rect, 0.0, Color32::from_rgb(255, 0, 51));
                    }
                    if thumb_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(if pt {
                            "Reproduzir vídeo a partir do histórico"
                        } else {
                            "Play video from history"
                        })
                        .clicked()
                    {
                        play_clicked = true;
                    }
                    ui.add_space(10.0);
                    let content_width = (ui.available_width() - 36.0).max(60.0);
                    ui.vertical(|ui| {
                        ui.set_width(content_width);
                        ui.set_max_width(content_width);
                        let title_size = if compact { 13.5 } else { 14.5 };
                        let title_button = egui::Button::new(
                            RichText::new(&entry.title).size(title_size).strong(),
                        )
                        .frame(false)
                        .truncate();
                        if ui
                            .add(title_button)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&entry.title)
                            .clicked()
                        {
                            play_clicked = true;
                        }
                        ui.horizontal_wrapped(|ui| {
                            if !entry.channel.is_empty() {
                                let ch_limit = if compact { 24 } else { 40 };
                                ui.label(
                                    RichText::new(truncate_text_chars(&entry.channel, ch_limit))
                                        .small()
                                        .weak(),
                                );
                                ui.separator();
                            }
                            let status = format_history_status_i18n(entry, pt);
                            let status_color = if entry.resume_position_ms().is_some() {
                                palette.resume_status_fg
                            } else {
                                palette.neutral_status_fg
                            };
                            ui.label(RichText::new(status).small().color(status_color));
                            if styled_compact_button(
                                ui,
                                if pt { "+ Fila" } else { "+ Queue" },
                                false,
                                &palette,
                            )
                            .on_hover_text(if pt {
                                "Adicionar vídeo à fila"
                            } else {
                                "Add video to queue"
                            })
                            .clicked()
                            {
                                enqueue_clicked = true;
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        if styled_icon_chip_button(ui, true, "✕", &palette)
                            .on_hover_text(if pt {
                                "Remover este vídeo do histórico"
                            } else {
                                "Remove this video from history"
                            })
                            .clicked()
                        {
                            remove_clicked = true;
                        }
                    });
                });
            });
        paint_library_card_hover_glow(
            ui,
            frame_resp.response.rect,
            10.0,
            ui.make_persistent_id(("history_card_hover", &entry.video_id)),
            &palette,
        );
        (play_clicked, remove_clicked, enqueue_clicked)
    }

    fn history_youtube_row_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        entry: &HistoryEntry,
        row_width: f32,
        thumb_size: egui::Vec2,
        is_playing: bool,
    ) -> (HistoryRowAction, Option<Video>) {
        let mut action = HistoryRowAction::default();
        let pt = self.ui_pt();
        let palette = self.palette();
        let known_video = self.video_for_history_entry(entry);
        let canonical_url = history::youtube_watch_url(&entry.video_id)
            .or_else(|| known_video.as_ref().map(|v| v.url.clone()));
        let metadata = known_video
            .as_ref()
            .and_then(|v| self.metadata_for(&v.url))
            .or_else(|| canonical_url.as_deref().and_then(|u| self.metadata_for(u)))
            .cloned();
        let channel_target = known_video
            .as_ref()
            .and_then(|v| self.channel_target_for_video(v));
        let thumb_url: std::borrow::Cow<'_, str> = entry
            .thumbnail_url
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .or_else(|| {
                known_video
                    .as_ref()
                    .map(|v| v.thumbnail.trim())
                    .filter(|t| !t.is_empty())
            })
            .map(std::borrow::Cow::Borrowed)
            .unwrap_or_else(|| {
                std::borrow::Cow::Owned(format!(
                    "https://i.ytimg.com/vi/{}/hqdefault.jpg",
                    entry.video_id
                ))
            });
        let duration_text = entry
            .duration_ms
            .filter(|&ms| ms > 0)
            .map(|ms| format_time(ms as f64 / 1000.0))
            .or_else(|| {
                known_video
                    .as_ref()
                    .map(|v| v.duration.trim().to_owned())
                    .filter(|d| !d.is_empty())
            })
            .unwrap_or_default();
        let progress_ratio = entry
            .duration_ms
            .filter(|&d| d > 0 && entry.position_ms > 0)
            .map(|d| {
                if history::is_near_end(entry.position_ms, Some(d)) {
                    1.0_f32
                } else {
                    (entry.position_ms as f32 / d as f32).clamp(0.05, 1.0)
                }
            });
        let in_watch_later = self.library.data.is_in_watch_later(&entry.video_id)
            || canonical_url
                .as_deref()
                .is_some_and(|u| self.library.data.is_in_watch_later(u));
        let in_queue = canonical_url.as_deref().is_some_and(|url| {
            self.queue
                .data
                .videos()
                .iter()
                .any(|v| Self::same_video_target(&v.url, url))
        });

        ui.allocate_ui_with_layout(
            egui::vec2(row_width, thumb_size.y),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.set_min_height(thumb_size.y);
                ui.set_max_height(thumb_size.y);
                ui.spacing_mut().item_spacing.x = 16.0;

                let thumb_resp =
                    self.thumbnails
                        .show_rounded(ui, ctx, thumb_url.as_ref(), thumb_size, 12.0);
                let hover_t = ctx.animate_bool_with_time(
                    ui.make_persistent_id(("yt_history_thumb_hover", &entry.video_id)),
                    thumb_resp.hovered(),
                    0.14,
                );
                paint_card_hover_highlight(ui, thumb_resp.rect, 12.0, hover_t, &palette);
                paint_thumbnail_overlays(
                    ui,
                    thumb_resp.rect,
                    &duration_text,
                    progress_ratio,
                    entry.is_music(),
                );
                if in_queue {
                    paint_queued_badge(ui, thumb_resp.rect, false);
                }
                if thumb_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(&entry.title)
                    .clicked()
                {
                    action.play_clicked = true;
                }

                let details_w = (row_width - thumb_size.x - 16.0).max(120.0);
                ui.vertical(|ui| {
                    ui.set_width(details_w);
                    ui.set_max_width(details_w);
                    ui.spacing_mut().item_spacing.y = 0.0;

                    ui.horizontal_top(|ui| {
                        let title_w = (details_w - 68.0).max(80.0);
                        ui.vertical(|ui| {
                            ui.set_width(title_w);
                            ui.set_max_width(title_w);
                            let title_color = if is_playing {
                                palette.info_accent_fg
                            } else {
                                palette.header_button_fg
                            };
                            let title_resp = ui.add(
                                egui::Label::new(
                                    RichText::new(truncate_text_chars(&entry.title, 96))
                                        .size(18.0)
                                        .color(title_color),
                                )
                                .wrap()
                                .sense(egui::Sense::click()),
                            );
                            if title_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(&entry.title)
                                .clicked()
                            {
                                action.play_clicked = true;
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            let more_resp = paint_more_dots_button(ui, &palette);
                            egui::Popup::menu(&more_resp).show(|ui| {
                                history_entry_action_menu_ui(
                                    ui,
                                    in_watch_later,
                                    pt,
                                    &palette,
                                    &mut action,
                                );
                            });
                            if styled_icon_chip_button(ui, true, "✕", &palette)
                                .on_hover_text(if pt {
                                    "Remover do histórico de exibição"
                                } else {
                                    "Remove from watch history"
                                })
                                .clicked()
                            {
                                action.remove_clicked = true;
                            }
                        });
                    });

                    let views_label = entry
                        .view_count_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(|text| format!("\u{25b7} {text}"))
                        .or_else(|| {
                            metadata.as_ref().and_then(|meta| {
                                if let Some(views) = meta.view_count {
                                    Some(format!(
                                        "\u{25b7} {}",
                                        recommendations::format_youtube_compact_views(views)
                                    ))
                                } else {
                                    meta.view_count_text
                                        .as_deref()
                                        .map(str::trim)
                                        .filter(|text| !text.is_empty())
                                        .map(|text| format!("\u{25b7} {text}"))
                                }
                            })
                        });
                    let published_label = metadata
                        .as_ref()
                        .and_then(|meta| meta.upload_date.as_deref())
                        .and_then(|date| recommendations::format_youtube_relative_time(date, None))
                        .or_else(|| {
                            entry
                                .published_time_text
                                .as_deref()
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .map(str::to_owned)
                        })
                        .or_else(|| {
                            metadata
                                .as_ref()
                                .and_then(|meta| meta.published_time_text.as_deref())
                                .map(str::trim)
                                .filter(|text| !text.is_empty())
                                .map(str::to_owned)
                        });
                    let has_channel = !entry.channel.trim().is_empty();
                    if has_channel || views_label.is_some() || published_label.is_some() {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(5.0, 2.0);
                            if has_channel {
                                let ch_text =
                                    RichText::new(truncate_text_chars(&entry.channel, 42))
                                        .size(12.5)
                                        .color(palette.neutral_status_fg);
                                if let Some(target) = &channel_target {
                                    let ch_resp = ui.add(
                                        egui::Label::new(ch_text)
                                            .sense(egui::Sense::click())
                                            .selectable(false),
                                    );
                                    if ch_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        action.open_channel = Some(target.clone());
                                    }
                                } else {
                                    ui.label(ch_text);
                                }
                                let is_verified = entry.channel_verified == Some(true)
                                    || metadata.as_ref().is_some_and(|m| {
                                        m.channel_verified
                                            || m.view_count.is_some_and(|v| v >= 100_000)
                                    });
                                if is_verified {
                                    paint_verified_badge_sized(ui, &palette, 13.0);
                                }
                            }
                            if let Some(views) = &views_label {
                                ui.label(
                                    RichText::new(views)
                                        .size(12.5)
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            if let Some(published) = &published_label {
                                let sep_prefix = if has_channel || views_label.is_some() {
                                    "· "
                                } else {
                                    ""
                                };
                                ui.label(
                                    RichText::new(format!("{sep_prefix}{published}"))
                                        .size(12.5)
                                        .color(palette.neutral_status_fg),
                                );
                            }
                        });
                    }
                    if let Some(snippet) = metadata
                        .as_ref()
                        .and_then(|m| crate::description_snippet(m.description.as_deref(), 140))
                    {
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(snippet)
                                    .size(12.0)
                                    .color(palette.neutral_status_fg),
                            )
                            .truncate(),
                        );
                    }
                });
            },
        );

        (action, known_video)
    }

    fn history_management_rail_ui(
        &mut self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        rail_width: f32,
        pt: bool,
        palette: &ThemePalette,
    ) -> HistoryRailOutcome {
        let mut outcome = HistoryRailOutcome::default();
        let history_paused = self.settings.prefs.value.pause_watch_history;
        let search_w = rail_width.min(268.0);
        let action_w = rail_width.min(306.0);

        ui.vertical(|ui| {
            ui.set_width(rail_width);
            ui.set_max_width(rail_width);
            ui.spacing_mut().item_spacing.y = 0.0;

            ui.allocate_ui_with_layout(
                egui::vec2(search_w, 34.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    paint_history_search_icon(
                        ui.painter(),
                        icon_rect.center(),
                        palette.neutral_status_fg,
                    );
                    let clear_reserve = if self.history.search_query.is_empty() {
                        0.0
                    } else {
                        24.0
                    };
                    let edit_w = (search_w - 32.0 - clear_reserve).max(96.0);
                    let search_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.history.search_query)
                            .id_salt((scroll_id, "yt_history_search_input"))
                            .frame(egui::Frame::NONE)
                            .hint_text(
                                RichText::new(if pt {
                                    "Pesquise no hist\u{00f3}rico de exi..."
                                } else {
                                    "Search watch history"
                                })
                                .size(14.0)
                                .color(palette.neutral_status_fg),
                            )
                            .text_color(palette.header_button_fg)
                            .font(egui::FontId::proportional(14.0))
                            .desired_width(edit_w),
                    );
                    if !self.history.search_query.is_empty()
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new("\u{2715}")
                                        .size(12.5)
                                        .color(palette.neutral_status_fg),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                    {
                        self.history.search_query.clear();
                    }
                    let line_color = if search_resp.has_focus() {
                        palette.header_button_fg
                    } else {
                        palette.neutral_status_fg.gamma_multiply(0.75)
                    };
                    let line_thickness = if search_resp.has_focus() { 1.5 } else { 1.0 };
                    let underline_y = icon_rect.bottom().max(search_resp.rect.bottom()) + 6.0;
                    ui.painter().line_segment(
                        [
                            egui::pos2(icon_rect.left(), underline_y),
                            egui::pos2(icon_rect.left() + search_w, underline_y),
                        ],
                        egui::Stroke::new(line_thickness, line_color),
                    );
                },
            );
            ui.add_space(26.0);

            let clear_label = if pt {
                "Limpar todo o hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o"
            } else {
                "Clear all watch history"
            };
            if history_rail_action_button(
                ui,
                action_w,
                clear_label,
                palette,
                paint_history_trash_icon,
            )
            .clicked()
            {
                outcome.clear_clicked = true;
            }
            if self.history.remote.confirm_remote_clear {
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(action_w - 24.0);
                        ui.label(
                            RichText::new(if pt {
                                "Limpar hist\u{00f3}rico local e da conta YouTube?"
                            } else {
                                "Clear local and YouTube watch history?"
                            })
                            .size(12.5)
                            .color(palette.header_button_fg),
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if styled_danger_button(
                                ui,
                                if pt { "Confirmar" } else { "Confirm" },
                                palette,
                            )
                            .clicked()
                            {
                                outcome.confirm_clear_clicked = true;
                            }
                            if styled_ghost_button(
                                ui,
                                if pt { "Cancelar" } else { "Cancel" },
                                palette,
                            )
                            .clicked()
                            {
                                outcome.cancel_clear_clicked = true;
                            }
                        });
                    });
            }
            ui.add_space(8.0);

            let pause_label = match (history_paused, pt) {
                (true, true) => "Ativar o hist\u{00f3}rico de visualiza\u{00e7}\u{00f5}es",
                (true, false) => "Turn on watch history",
                (false, true) => "Pausar o hist\u{00f3}rico de visualiza\u{00e7}\u{00f5}es",
                (false, false) => "Pause watch history",
            };
            if history_rail_action_button(ui, action_w, pause_label, palette, |p, c, col| {
                if history_paused {
                    paint_history_play_icon(p, c, col);
                } else {
                    paint_history_pause_bars_icon(p, c, col);
                }
            })
            .clicked()
            {
                outcome.toggle_pause_clicked = true;
            }
            ui.add_space(8.0);

            let manage_label = if pt {
                "Gerenciar hist\u{00f3}rico"
            } else {
                "Manage all history"
            };
            if history_rail_action_button(ui, action_w, manage_label, palette, |p, c, col| {
                paint_yt_icon(p, c, YtSidebarIcon::Settings, col, false);
            })
            .clicked()
            {
                outcome.manage_clicked = true;
            }
            ui.add_space(10.0);

            let sub_links = [
                (
                    if pt { "Coment\u{00e1}rios" } else { "Comments" },
                    "https://myactivity.google.com/page?page=youtube_comments",
                ),
                (
                    if pt { "Postagens" } else { "Posts" },
                    "https://www.youtube.com/feed/history/community_history",
                ),
                (
                    if pt { "Chat ao vivo" } else { "Live chat" },
                    "https://myactivity.google.com/page?page=youtube_live_chat",
                ),
            ];
            for (label, url) in sub_links {
                ui.horizontal(|ui| {
                    ui.add_space(40.0);
                    let resp = ui.add(
                        egui::Label::new(
                            RichText::new(label)
                                .size(14.0)
                                .color(palette.neutral_status_fg),
                        )
                        .sense(egui::Sense::click())
                        .selectable(false),
                    );
                    if resp.hovered() {
                        ui.painter().text(
                            resp.rect.left_center(),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(14.0),
                            palette.header_button_fg,
                        );
                    }
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        outcome.external_url = Some(url);
                    }
                });
                ui.add_space(12.0);
            }

            ui.add_space(6.0);
            paint_section_hairline(ui, palette);
            ui.add_space(10.0);

            let synced_count = self
                .history
                .remote
                .feeds
                .get(&history::HistoryFilter::All)
                .map_or(0, |f| f.seen_ids.len())
                .max(self.history.data.entries().len());
            if self.history.remote.sync_rx.is_some() {
                ui.label(
                    RichText::new(if pt {
                        format!("Sincronizando com YouTube... ({synced_count})")
                    } else {
                        format!("Syncing with YouTube... ({synced_count})")
                    })
                    .size(12.5)
                    .color(palette.neutral_status_fg),
                );
            } else if self.history.remote.loaded && !self.history.remote.needs_connect {
                ui.label(
                    RichText::new(if pt {
                        format!("Sincronizado com YouTube \u{00b7} {synced_count} v\u{00ed}deos")
                    } else {
                        format!("Synced with YouTube \u{00b7} {synced_count} videos")
                    })
                    .size(12.0)
                    .color(palette.neutral_status_fg),
                );
                ui.add_space(6.0);
                if styled_compact_button(
                    ui,
                    if pt { "Sincronizar agora" } else { "Sync now" },
                    false,
                    palette,
                )
                .clicked()
                {
                    outcome.sync_now_clicked = true;
                }
            } else {
                if styled_compact_button(
                    ui,
                    if pt {
                        "Sincronizar com YouTube"
                    } else {
                        "Sync with YouTube"
                    },
                    true,
                    palette,
                )
                .clicked()
                {
                    outcome.connect_sync_clicked = true;
                }
            }
            if let Some(err) = &self.history.remote.error {
                ui.add_space(6.0);
                ui.label(RichText::new(err).size(11.5).color(palette.warning_fg));
            }
        });

        outcome
    }

    fn history_shorts_shelf_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        shelf_key: &str,
        items: &[HistoryEntry],
        feed_col_w: f32,
        current_video_id: Option<&str>,
    ) -> HistoryShortsShelfOutcome {
        let mut outcome = HistoryShortsShelfOutcome::default();
        if items.is_empty() {
            return outcome;
        }
        let pt = self.ui_pt();
        let palette = self.palette();

        ui.horizontal(|ui| {
            paint_shorts_badge_icon(ui);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Shorts")
                    .size(18.0)
                    .strong()
                    .color(palette.header_button_fg),
            );
        });
        ui.add_space(10.0);

        let cols: usize = if feed_col_w >= 540.0 { 3 } else { 2 };
        let card_gap = 12.0;
        let card_w = ((feed_col_w - card_gap * (cols.saturating_sub(1) as f32)) / cols as f32)
            .clamp(130.0, 236.0);
        let card_h = (card_w * 16.0 / 9.0).round();

        let max_offset = items.len().saturating_sub(cols);
        let mut offset = self
            .history
            .remote
            .shorts_shelf_offsets
            .get(shelf_key)
            .copied()
            .unwrap_or(0)
            .min(max_offset);
        let visible_end = (offset + cols).min(items.len());

        let row_resp = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = card_gap;
            for entry in &items[offset..visible_end] {
                let is_playing = current_video_id == Some(entry.video_id.as_str());
                let known_video = self.video_for_history_entry(entry);
                let canonical_url = history::youtube_watch_url(&entry.video_id)
                    .or_else(|| known_video.as_ref().map(|v| v.url.clone()));
                let metadata = known_video
                    .as_ref()
                    .and_then(|v| self.metadata_for(&v.url))
                    .or_else(|| canonical_url.as_deref().and_then(|u| self.metadata_for(u)))
                    .cloned();
                let thumb_url: String = entry
                    .thumbnail_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .or_else(|| {
                        known_video
                            .as_ref()
                            .map(|v| v.thumbnail.trim())
                            .filter(|t| !t.is_empty())
                    })
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", entry.video_id)
                    });
                let in_watch_later = self.library.data.is_in_watch_later(&entry.video_id)
                    || canonical_url
                        .as_deref()
                        .is_some_and(|u| self.library.data.is_in_watch_later(u));

                ui.push_id(("yt_hist_short", shelf_key, &entry.video_id), |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        ui.set_max_width(card_w);
                        let thumb_resp = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            &thumb_url,
                            egui::vec2(card_w, card_h),
                            12.0,
                        );
                        let hover_t = ctx.animate_bool_with_time(
                            ui.make_persistent_id(("yt_hist_short_hover", &entry.video_id)),
                            thumb_resp.hovered(),
                            0.14,
                        );
                        paint_card_hover_highlight(ui, thumb_resp.rect, 12.0, hover_t, &palette);
                        if let Some(ratio) = entry
                            .duration_ms
                            .filter(|&d| d > 0 && entry.position_ms > 0)
                            .map(|d| {
                                if history::is_near_end(entry.position_ms, Some(d)) {
                                    1.0_f32
                                } else {
                                    (entry.position_ms as f32 / d as f32).clamp(0.05, 1.0)
                                }
                            })
                        {
                            paint_thumbnail_overlays(ui, thumb_resp.rect, "", Some(ratio), false);
                        }
                        if thumb_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&entry.title)
                            .clicked()
                            && outcome.play_video.is_none()
                        {
                            outcome.play_video = known_video.clone();
                        }

                        ui.add_space(8.0);
                        let mut row_action = HistoryRowAction::default();
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let title_w = (card_w - 28.0).max(60.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(title_w, 0.0),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    let title_color = if is_playing {
                                        palette.info_accent_fg
                                    } else {
                                        palette.header_button_fg
                                    };
                                    let title_resp = ui.add(
                                        egui::Button::new(
                                            RichText::new(truncate_text_chars(&entry.title, 44))
                                                .size(13.5)
                                                .strong()
                                                .color(title_color),
                                        )
                                        .frame(false)
                                        .wrap(),
                                    );
                                    if title_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&entry.title)
                                        .clicked()
                                    {
                                        row_action.play_clicked = true;
                                    }
                                },
                            );
                            let more_resp = paint_more_dots_button(ui, &palette);
                            egui::Popup::menu(&more_resp).show(|ui| {
                                history_entry_action_menu_ui(
                                    ui,
                                    in_watch_later,
                                    pt,
                                    &palette,
                                    &mut row_action,
                                );
                            });
                        });

                        let sub_label = entry
                            .view_count_text
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(|s| {
                                if pt {
                                    format!("{s} de visualiza\u{00e7}\u{00f5}es")
                                } else {
                                    format!("{s} views")
                                }
                            })
                            .or_else(|| {
                                metadata.as_ref().and_then(|m| {
                                    m.view_count.map(|v| {
                                        let compact =
                                            recommendations::format_youtube_compact_views(v);
                                        if pt {
                                            format!("{compact} de visualiza\u{00e7}\u{00f5}es")
                                        } else {
                                            format!("{compact} views")
                                        }
                                    })
                                })
                            })
                            .or_else(|| {
                                let ch = entry.channel.trim();
                                (!ch.is_empty()).then(|| ch.to_owned())
                            });
                        if let Some(sub) = sub_label {
                            ui.label(
                                RichText::new(sub)
                                    .size(12.0)
                                    .color(palette.neutral_status_fg),
                            );
                        }

                        if row_action.play_clicked && outcome.play_video.is_none() {
                            outcome.play_video = known_video.clone();
                        }
                        if row_action.enqueue_clicked {
                            outcome.enqueue_video = known_video.clone();
                        }
                        if row_action.toggle_watch_later {
                            outcome.toggle_watch_later = known_video.clone();
                        }
                        if row_action.open_save_modal {
                            outcome.open_save_modal = known_video.clone();
                        }
                        if row_action.share_clicked {
                            outcome.open_share_modal = known_video.clone();
                        }
                        if let Some(fmt) = row_action.download_format
                            && let Some(video) = known_video
                        {
                            outcome.download_req = Some((video, fmt));
                        }
                        if row_action.remove_clicked {
                            outcome.remove_video_id = Some(entry.video_id.clone());
                        }
                    });
                });
            }
        });

        let row_rect = row_resp.response.rect;
        let arrow_y = row_rect.top() + card_h * 0.5;
        let arrow_fill = Color32::from_rgba_unmultiplied(33, 33, 33, 235);
        if offset > 0 {
            let left_rect = egui::Rect::from_center_size(
                egui::pos2(row_rect.left() + 18.0, arrow_y),
                egui::vec2(34.0, 34.0),
            );
            let resp = ui.put(
                left_rect,
                egui::Button::new(RichText::new("<").size(15.0).strong().color(Color32::WHITE))
                    .fill(arrow_fill)
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(72, 72, 76)))
                    .corner_radius(17),
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                offset = offset.saturating_sub(1);
            }
        }
        if visible_end < items.len() {
            let right_rect = egui::Rect::from_center_size(
                egui::pos2(row_rect.right() - 18.0, arrow_y),
                egui::vec2(34.0, 34.0),
            );
            let resp = ui.put(
                right_rect,
                egui::Button::new(RichText::new(">").size(15.0).strong().color(Color32::WHITE))
                    .fill(arrow_fill)
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(72, 72, 76)))
                    .corner_radius(17),
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                offset = (offset + 1).min(max_offset);
            }
        }
        if offset == 0 {
            self.history.remote.shorts_shelf_offsets.remove(shelf_key);
        } else {
            self.history
                .remote
                .shorts_shelf_offsets
                .insert(shelf_key.to_owned(), offset);
        }

        outcome
    }

    fn history_display_cache_key(&self, pt: bool) -> HistoryDisplayCacheKey {
        let entries = self.history.data.entries();
        let local_first_sig = entries.first().map(|e| {
            (
                e.video_id.clone(),
                e.position_ms,
                e.duration_ms,
                e.watched_at_ms,
            )
        });
        let local_last_sig = entries.last().map(|e| (e.video_id.clone(), e.position_ms));
        let active_filter = self.history.filter;
        let remote_feed = self.history.remote.feeds.get(&active_filter);
        let (
            remote_loaded,
            remote_page_count,
            remote_sections_len,
            remote_items_len,
            remote_seen_len,
            remote_first_id,
            remote_last_id,
        ) = match remote_feed {
            Some(feed) => (
                feed.loaded,
                feed.page_count,
                feed.sections.len(),
                feed.sections.iter().map(|s| s.items.len()).sum(),
                feed.seen_ids.len(),
                feed.sections
                    .first()
                    .and_then(|s| s.items.first())
                    .map(|i| i.video_id.clone()),
                feed.sections
                    .last()
                    .and_then(|s| s.items.last())
                    .map(|i| i.video_id.clone()),
            ),
            None => (false, 0, 0, 0, 0, None, None),
        };
        let local_minute_bucket = history::current_unix_epoch_ms() / 60_000;
        HistoryDisplayCacheKey {
            filter: active_filter,
            search_query: self.history.search_query.trim().to_owned(),
            pt,
            local_len: entries.len(),
            local_first_sig,
            local_last_sig,
            local_minute_bucket,
            remote_loaded,
            remote_page_count,
            remote_sections_len,
            remote_items_len,
            remote_seen_len,
            remote_first_id,
            remote_last_id,
        }
    }

    pub(crate) fn history_entry_is_live(
        &self,
        entry: &HistoryEntry,
        remote_is_live: Option<bool>,
    ) -> bool {
        if remote_is_live == Some(true) {
            return true;
        }
        if entry
            .view_count_text
            .as_deref()
            .is_some_and(crate::is_live_duration_label)
        {
            return true;
        }
        if let Some(url) = history::youtube_watch_url(&entry.video_id)
            && self.metadata_for(&url).is_some_and(|m| m.is_live)
        {
            return true;
        }
        if let Some(video) = self.video_for_history_entry(entry)
            && (crate::is_live_duration_label(&video.duration)
                || self.metadata_for(&video.url).is_some_and(|m| m.is_live))
        {
            return true;
        }
        let title_lower = entry.title.to_lowercase();
        title_lower.contains("[ao vivo]") || title_lower.contains("[live]")
    }

    fn build_history_display_sections(
        &self,
        pt: bool,
        live_only: bool,
    ) -> (Vec<HistoryDisplaySection>, bool) {
        let active_filter = self.history.filter;
        let q = self.history.search_query.trim().to_lowercase();

        if let Some(remote_feed) = self.history.remote.feeds.get(&active_filter)
            && (remote_feed.loaded || !remote_feed.sections.is_empty())
        {
            let local_entries = self.history.data.entries();
            let local_by_id: HashMap<&str, &HistoryEntry> = local_entries
                .iter()
                .map(|entry| (entry.video_id.as_str(), entry))
                .collect();
            let mut sections = Vec::with_capacity(remote_feed.sections.len());
            for sec in &remote_feed.sections {
                let mut entries = Vec::with_capacity(sec.items.len());
                for item in &sec.items {
                    if !q.is_empty()
                        && !item.title.to_lowercase().contains(&q)
                        && !item.channel.to_lowercase().contains(&q)
                    {
                        continue;
                    }
                    let mut entry = item.to_history_entry(None, active_filter);
                    if let Some(local) = local_by_id.get(item.video_id.as_str()) {
                        if entry.duration_ms.is_none() {
                            entry.duration_ms = local.duration_ms;
                        }
                        entry.position_ms = local.position_ms.max(entry.position_ms);
                        if let Some(dur) = entry.duration_ms {
                            entry.position_ms = entry.position_ms.min(dur);
                        }
                    }
                    if live_only && !self.history_entry_is_live(&entry, Some(item.is_live)) {
                        continue;
                    }
                    entries.push(entry);
                }
                if !entries.is_empty() {
                    let date_label = if sec.date_label.trim().is_empty() {
                        if pt { "Mais antigos" } else { "Older" }.to_owned()
                    } else {
                        sec.date_label.clone()
                    };
                    sections.push(HistoryDisplaySection::from_entries(date_label, entries));
                }
            }
            return (sections, true);
        }

        let entries = self.history.data.entries();
        let filtered: Vec<&HistoryEntry> =
            history::filter_entries(entries, &self.history.search_query, active_filter)
                .into_iter()
                .filter(|entry| !live_only || self.history_entry_is_live(entry, None))
                .collect();
        let now_ms = history::current_unix_epoch_ms();
        let offset_minutes = history::local_utc_offset_minutes();

        if filtered.iter().any(|e| e.date_label.is_some()) {
            let mut grouped: Vec<(String, Vec<HistoryEntry>)> = Vec::new();
            for entry in filtered {
                let label = entry
                    .date_label
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        history::date_group_at(entry.watched_at_ms, now_ms, offset_minutes)
                            .label(pt)
                            .to_owned()
                    });
                if let Some(last) = grouped.last_mut()
                    && last.0 == label
                {
                    last.1.push(entry.clone());
                } else {
                    grouped.push((label, vec![entry.clone()]));
                }
            }
            let sections = grouped
                .into_iter()
                .map(|(date_label, entries)| {
                    HistoryDisplaySection::from_entries(date_label, entries)
                })
                .collect();
            return (sections, false);
        }

        let groups = [
            history::DateGroup::Today,
            history::DateGroup::Yesterday,
            history::DateGroup::ThisWeek,
            history::DateGroup::Older,
        ];
        let mut buckets: [Vec<HistoryEntry>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for entry in filtered {
            let slot = match history::date_group_at(entry.watched_at_ms, now_ms, offset_minutes) {
                history::DateGroup::Today => 0,
                history::DateGroup::Yesterday => 1,
                history::DateGroup::ThisWeek => 2,
                history::DateGroup::Older => 3,
            };
            buckets[slot].push(entry.clone());
        }
        let sections = buckets
            .into_iter()
            .enumerate()
            .filter(|(_, bucket)| !bucket.is_empty())
            .map(|(slot_idx, entries)| {
                HistoryDisplaySection::from_entries(groups[slot_idx].label(pt).to_owned(), entries)
            })
            .collect();
        (sections, false)
    }

    fn history_youtube_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        max_card_width: Option<f32>,
        max_height: Option<f32>,
    ) -> Option<Video> {
        self.ensure_history_synchronized();
        let pt = self.ui_pt();
        let palette = self.palette();
        let ctx = ui.ctx().clone();
        let entry_count = self.history.data.entries().len();
        let history_paused = self.settings.prefs.value.pause_watch_history;
        let current_video_id = self
            .selected
            .as_ref()
            .and_then(|video| history::youtube_video_id(&video.url));

        let active_filter = self.history.filter;
        let live_chip_id = egui::Id::new("fasttube_history_live_chip");
        let mut live_only = ctx.data(|d| d.get_temp::<bool>(live_chip_id).unwrap_or(false));
        let hist_cache_id = egui::Id::new(("fasttube_history_display_cache", live_only));
        let hist_cache_key = self.history_display_cache_key(pt);
        let existing_hist = ctx.data(|d| d.get_temp::<Arc<CachedHistoryDisplay>>(hist_cache_id));
        let cached_history =
            resolve_history_display_cache(existing_hist.as_ref(), hist_cache_key, || {
                self.build_history_display_sections(pt, live_only)
            });
        if existing_hist
            .as_ref()
            .is_none_or(|prev| !Arc::ptr_eq(prev, &cached_history))
        {
            ctx.data_mut(|d| d.insert_temp(hist_cache_id, Arc::clone(&cached_history)));
        }
        let display_sections = &cached_history.sections;
        let from_remote = cached_history.from_remote;
        let remote_has_items = self
            .history
            .remote
            .feeds
            .get(&active_filter)
            .is_some_and(|f| !f.sections.is_empty())
            || self
                .history
                .remote
                .feeds
                .get(&history::HistoryFilter::All)
                .is_some_and(|f| !f.sections.is_empty());
        let is_remote_loading = (self.history.remote.sync_rx.is_some()
            && !self.history.remote.loaded)
            || (self.history.remote.chip_rx.is_some()
                && self.history.remote.chip_active_filter == Some(active_filter));
        let has_next_cont = self
            .history
            .remote
            .feeds
            .get(&active_filter)
            .is_some_and(|f| f.next_continuation.is_some());
        let is_loading_more = self
            .history
            .remote
            .feeds
            .get(&active_filter)
            .is_some_and(|f| f.loading_more);

        let mut chosen: Option<Video> = None;
        let mut to_remove: Option<String> = None;
        let mut to_enqueue: Option<Video> = None;
        let mut toggle_watch_later: Option<Video> = None;
        let mut open_save_modal: Option<Video> = None;
        let mut open_share_modal: Option<Video> = None;
        let mut download_req: Option<(Video, downloads::DownloadFormat)> = None;
        let mut open_channel: Option<ChannelTarget> = None;
        let mut clear_filter_clicked = false;
        let mut clicked_chip: Option<history::HistoryFilter> = None;
        let mut toggle_live_chip = false;
        let mut load_more_filter: Option<history::HistoryFilter> = None;
        let mut rail_outcome = HistoryRailOutcome::default();

        let mut render_feed_column = |app: &mut Self, ui: &mut egui::Ui, feed_col_w: f32| {
            ui.set_width(feed_col_w);
            ui.set_max_width(feed_col_w);
            ui.add_space(12.0);
            ui.label(
                RichText::new(if pt {
                    "Hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o"
                } else {
                    "Watch history"
                })
                .size(34.0)
                .strong()
                .color(palette.header_button_fg),
            );
            if history_paused {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(if pt {
                        "O hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o est\u{00e1} pausado."
                    } else {
                        "Watch history is paused."
                    })
                    .size(12.5)
                    .color(palette.warning_fg),
                );
            }
            ui.add_space(16.0);

            let filter_options = [
                (history::HistoryFilter::All, if pt { "Tudo" } else { "All" }),
                (
                    history::HistoryFilter::Videos,
                    if pt { "V\u{00ed}deos" } else { "Videos" },
                ),
                (history::HistoryFilter::Shorts, "Shorts"),
                (history::HistoryFilter::Podcasts, "Podcasts"),
                (
                    history::HistoryFilter::Music,
                    if pt { "M\u{00fa}sica" } else { "Music" },
                ),
            ];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                for (filter, label) in filter_options {
                    let selected = app.history.filter == filter && !live_only;
                    let (bg, fg) = if selected {
                        (palette.active_button_bg, palette.active_button_fg)
                    } else {
                        (palette.header_button_bg, palette.header_button_fg)
                    };
                    let text = if selected {
                        RichText::new(label).size(13.5).strong().color(fg)
                    } else {
                        RichText::new(label).size(13.5).color(fg)
                    };
                    let btn = egui::Button::new(text)
                        .fill(bg)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(8)
                        .min_size(egui::vec2(0.0, 32.0));
                    if ui
                        .add(btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        clicked_chip = Some(filter);
                    }
                }
                let live_label = if pt { "Ao vivo" } else { "Live" };
                let (live_bg, live_fg) = if live_only {
                    (palette.active_button_bg, palette.active_button_fg)
                } else {
                    (palette.header_button_bg, palette.header_button_fg)
                };
                let live_text = if live_only {
                    RichText::new(live_label).size(13.5).strong().color(live_fg)
                } else {
                    RichText::new(live_label).size(13.5).color(live_fg)
                };
                let live_btn = egui::Button::new(live_text)
                    .fill(live_bg)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(8)
                    .min_size(egui::vec2(0.0, 32.0));
                if ui
                    .add(live_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    toggle_live_chip = true;
                }
            });
            ui.add_space(20.0);

            if is_remote_loading && display_sections.is_empty() {
                let dark_theme = app.is_dark_theme();
                for _ in 0..4 {
                    ui.horizontal_top(|ui| {
                        let _ = skeleton::skeleton_box(
                            ui,
                            egui::vec2(246.0_f32.min(feed_col_w * 0.44), 138.0),
                            12.0,
                            dark_theme,
                        );
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            let _ = skeleton::skeleton_box(
                                ui,
                                egui::vec2((feed_col_w - 280.0).max(120.0), 20.0),
                                6.0,
                                dark_theme,
                            );
                            ui.add_space(10.0);
                            let _ = skeleton::skeleton_box(
                                ui,
                                egui::vec2(160.0, 14.0),
                                6.0,
                                dark_theme,
                            );
                        });
                    });
                    ui.add_space(16.0);
                }
                return;
            }

            if entry_count == 0 && !remote_has_items && !from_remote {
                empty_state_card_ui(
                    ui,
                    if pt {
                        "Nenhum v\u{00ed}deo nesta lista."
                    } else {
                        "This list has no videos."
                    },
                    if pt {
                        "Os v\u{00ed}deos que voc\u{00ea} assistir aparecer\u{00e3}o aqui."
                    } else {
                        "Videos you watch will appear here."
                    },
                    &palette,
                );
                return;
            }

            if display_sections.is_empty() {
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(18, 16))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(if pt {
                                    "Nenhum v\u{00ed}deo corresponde ao filtro."
                                } else {
                                    "No videos match the filter."
                                })
                                .size(14.5)
                                .strong()
                                .color(palette.channel_fg),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(if pt {
                                    "Tente outro termo de busca ou redefina os filtros ativos."
                                } else {
                                    "Try another search term or reset active filters."
                                })
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                            );
                            ui.add_space(8.0);
                            if styled_ghost_button(
                                ui,
                                if pt { "Limpar filtro" } else { "Clear filter" },
                                &palette,
                            )
                            .clicked()
                            {
                                clear_filter_clicked = true;
                            }
                        });
                    });
                return;
            }

            let thumb_w = if feed_col_w >= 480.0 {
                246.0
            } else {
                (feed_col_w * 0.44).clamp(150.0, 246.0)
            };
            let thumb_h = (thumb_w * 9.0 / 16.0).round();
            let visible_rect = ui
                .clip_rect()
                .expand2(egui::vec2(0.0, thumbnails::VIEWPORT_OVERSCAN_PX + 240.0));

            let apply_shelf_outcome =
                |shelf: HistoryShortsShelfOutcome,
                 chosen: &mut Option<Video>,
                 to_enqueue: &mut Option<Video>,
                 toggle_watch_later: &mut Option<Video>,
                 open_save_modal: &mut Option<Video>,
                 open_share_modal: &mut Option<Video>,
                 download_req: &mut Option<(Video, downloads::DownloadFormat)>,
                 to_remove: &mut Option<String>| {
                    if shelf.play_video.is_some() && chosen.is_none() {
                        *chosen = shelf.play_video;
                    }
                    if shelf.enqueue_video.is_some() {
                        *to_enqueue = shelf.enqueue_video;
                    }
                    if shelf.toggle_watch_later.is_some() {
                        *toggle_watch_later = shelf.toggle_watch_later;
                    }
                    if shelf.open_save_modal.is_some() {
                        *open_save_modal = shelf.open_save_modal;
                    }
                    if shelf.open_share_modal.is_some() {
                        *open_share_modal = shelf.open_share_modal;
                    }
                    if shelf.download_req.is_some() {
                        *download_req = shelf.download_req;
                    }
                    if shelf.remove_video_id.is_some() {
                        *to_remove = shelf.remove_video_id;
                    }
                };

            app.thumbnails.begin_pass();
            let est_shelf_h = history_shorts_shelf_estimated_height(feed_col_w);
            let row_spacing_y = ui.spacing().item_spacing.y;
            let flush_skipped_height = |ui: &mut egui::Ui, pending: &mut f32| {
                if *pending > 0.0 {
                    ui.allocate_exact_size(egui::vec2(feed_col_w, *pending), egui::Sense::hover());
                    *pending = 0.0;
                }
            };
            for (section_idx, section) in display_sections.iter().enumerate() {
                if section_idx > 0 {
                    ui.add_space(22.0);
                }

                // Estimate full section height in O(1) using precomputed block counts so
                // completely offscreen sections (headers, Shorts shelves, rows) cull cheaply.
                let section_header_h = 26.0 + 14.0 + row_spacing_y;
                let section_body_h = section.estimated_body_height(
                    active_filter,
                    thumb_h,
                    est_shelf_h,
                    row_spacing_y,
                );
                let section_total_h = section_header_h + section_body_h;
                let section_top = ui.cursor().top();
                if section_top + section_total_h < visible_rect.top()
                    || section_top > visible_rect.bottom()
                {
                    ui.allocate_exact_size(
                        egui::vec2(feed_col_w, section_total_h),
                        egui::Sense::hover(),
                    );
                    continue;
                }

                ui.label(
                    RichText::new(&section.date_label)
                        .size(20.0)
                        .strong()
                        .color(palette.header_button_fg),
                );
                ui.add_space(14.0);

                if active_filter == history::HistoryFilter::Shorts {
                    let shelf_top = ui.cursor().top();
                    if shelf_top + est_shelf_h < visible_rect.top()
                        || shelf_top > visible_rect.bottom()
                    {
                        ui.allocate_exact_size(
                            egui::vec2(feed_col_w, est_shelf_h),
                            egui::Sense::hover(),
                        );
                    } else {
                        let shelf_key = format!("shorts:{section_idx}");
                        let shelf = app.history_shorts_shelf_ui(
                            ui,
                            &ctx,
                            &shelf_key,
                            &section.entries,
                            feed_col_w,
                            current_video_id.as_deref(),
                        );
                        apply_shelf_outcome(
                            shelf,
                            &mut chosen,
                            &mut to_enqueue,
                            &mut toggle_watch_later,
                            &mut open_save_modal,
                            &mut open_share_modal,
                            &mut download_req,
                            &mut to_remove,
                        );
                    }
                    continue;
                }

                let mut idx = 0usize;
                let mut run_idx = 0usize;
                let mut skipped_rows_h = 0.0_f32;
                while idx < section.entries.len() {
                    if active_filter == history::HistoryFilter::All
                        && section.entries[idx].is_short()
                    {
                        flush_skipped_height(ui, &mut skipped_rows_h);
                        let start = idx;
                        while idx < section.entries.len() && section.entries[idx].is_short() {
                            idx += 1;
                        }
                        let shelf_top = ui.cursor().top();
                        if shelf_top + est_shelf_h < visible_rect.top()
                            || shelf_top > visible_rect.bottom()
                        {
                            ui.allocate_exact_size(
                                egui::vec2(feed_col_w, est_shelf_h),
                                egui::Sense::hover(),
                            );
                        } else {
                            let shelf_key = format!("all:{section_idx}:{run_idx}");
                            let shelf = app.history_shorts_shelf_ui(
                                ui,
                                &ctx,
                                &shelf_key,
                                &section.entries[start..idx],
                                feed_col_w,
                                current_video_id.as_deref(),
                            );
                            apply_shelf_outcome(
                                shelf,
                                &mut chosen,
                                &mut to_enqueue,
                                &mut toggle_watch_later,
                                &mut open_save_modal,
                                &mut open_share_modal,
                                &mut download_req,
                                &mut to_remove,
                            );
                        }
                        run_idx += 1;
                        if idx < section.entries.len() {
                            ui.add_space(18.0);
                        }
                        continue;
                    }

                    let entry = &section.entries[idx];
                    let cursor_top = ui.cursor().top() + skipped_rows_h;
                    let has_next = idx + 1 < section.entries.len();
                    if cursor_top + thumb_h < visible_rect.top()
                        || cursor_top > visible_rect.bottom()
                    {
                        skipped_rows_h +=
                            thumb_h + if has_next { 16.0 + row_spacing_y } else { 0.0 };
                    } else {
                        flush_skipped_height(ui, &mut skipped_rows_h);
                        let is_playing =
                            current_video_id.as_deref() == Some(entry.video_id.as_str());
                        let (row_action, known_video) = ui
                            .push_id(("yt_hist_row", section_idx, &entry.video_id), |ui| {
                                app.history_youtube_row_ui(
                                    ui,
                                    &ctx,
                                    entry,
                                    feed_col_w,
                                    egui::vec2(thumb_w, thumb_h),
                                    is_playing,
                                )
                            })
                            .inner;
                        if row_action.play_clicked && chosen.is_none() {
                            chosen = known_video.clone();
                        }
                        if row_action.enqueue_clicked {
                            to_enqueue = known_video.clone();
                        }
                        if row_action.toggle_watch_later {
                            toggle_watch_later = known_video.clone();
                        }
                        if row_action.open_save_modal {
                            open_save_modal = known_video.clone();
                        }
                        if row_action.share_clicked {
                            open_share_modal = known_video.clone();
                        }
                        if let Some(fmt) = row_action.download_format
                            && let Some(video) = known_video
                        {
                            download_req = Some((video, fmt));
                        }
                        if row_action.open_channel.is_some() {
                            open_channel = row_action.open_channel;
                        }
                        if row_action.remove_clicked {
                            to_remove = Some(entry.video_id.clone());
                        }
                        if has_next {
                            ui.add_space(16.0);
                        }
                    }
                    idx += 1;
                }
                flush_skipped_height(ui, &mut skipped_rows_h);
            }
            app.thumbnails.end_pass(&ctx);

            if has_next_cont {
                ui.add_space(18.0);
                if is_loading_more {
                    ui.label(
                        RichText::new(if pt {
                            "Carregando mais v\u{00ed}deos..."
                        } else {
                            "Loading more videos..."
                        })
                        .size(12.5)
                        .color(palette.neutral_status_fg),
                    );
                } else {
                    let sentinel_rect =
                        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(feed_col_w, 32.0));
                    if styled_ghost_button(
                        ui,
                        if pt { "Carregar mais" } else { "Load more" },
                        &palette,
                    )
                    .clicked()
                        || (ui.is_rect_visible(sentinel_rect)
                            && !cfg!(test)
                            && app.history.remote.sync_rx.is_none())
                    {
                        load_more_filter = Some(active_filter);
                    }
                }
            }
            ui.add_space(24.0);
        };

        let saved_style = (*ui.style()).clone();
        let dark_theme = self.is_dark_theme();
        let bar_idle = if dark_theme {
            Color32::from_rgb(113, 113, 113)
        } else {
            Color32::from_rgb(160, 160, 160)
        };
        let bar_hover = if dark_theme {
            Color32::from_rgb(155, 155, 155)
        } else {
            Color32::from_rgb(120, 120, 120)
        };

        ui.scope(|ui| {
            {
                let visuals = &mut ui.style_mut().visuals;
                visuals.widgets.inactive.bg_fill = bar_idle;
                visuals.widgets.hovered.bg_fill = bar_hover;
                visuals.widgets.active.bg_fill = bar_hover;
            }
            let mut scroll = egui::ScrollArea::vertical()
                .id_salt(scroll_id)
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
            if let Some(limit) = max_height {
                scroll = scroll.max_height(limit);
            }
            scroll.show(ui, |ui| {
                ui.set_style(saved_style);
                let viewport_top = ui.clip_rect().top();
                let outer_w = ui.available_width().max(260.0);
                let content_w = max_card_width
                    .map(|limit| (outer_w - 48.0).clamp(260.0, limit))
                    .unwrap_or_else(|| outer_w.max(260.0));
                let outer_side_pad = ((outer_w - content_w) * 0.5).max(0.0);
                let two_column = content_w >= 760.0;
                let (left_inset, feed_col_w, gap, rail_w) = if two_column {
                    let left_inset = if content_w > 1180.0 {
                        ((content_w - 1180.0) * 0.25).clamp(16.0, 88.0)
                    } else if content_w >= 960.0 {
                        16.0
                    } else {
                        0.0
                    };
                    let rail_w = if content_w >= 960.0 {
                        310.0
                    } else {
                        (content_w * 0.34).clamp(240.0, 310.0)
                    };
                    let remaining_w = (content_w - left_inset).max(1.0);
                    let feed_col_w = (remaining_w - rail_w - 48.0).clamp(380.0, 760.0);
                    let gap = (remaining_w - feed_col_w - rail_w).clamp(24.0, 280.0);
                    (left_inset, feed_col_w, gap, rail_w)
                } else {
                    (0.0, content_w, 0.0, content_w)
                };

                if two_column {
                    let row_top = ui.cursor().top();
                    let feed_x = ui.max_rect().left() + outer_side_pad + left_inset;
                    let rail_x = feed_x + feed_col_w + gap;
                    let feed_rect = egui::Rect::from_min_max(
                        egui::pos2(feed_x, row_top),
                        egui::pos2(feed_x + feed_col_w, row_top + 1_000_000.0),
                    );
                    let mut feed_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt((scroll_id, "history_feed_col"))
                            .max_rect(feed_rect),
                    );
                    render_feed_column(self, &mut feed_ui, feed_col_w);
                    let feed_bottom = feed_ui.min_rect().bottom();

                    let rail_h_id = ui.make_persistent_id((scroll_id, "history_rail_h"));
                    let rail_min_h = ui
                        .ctx()
                        .data(|d| d.get_temp::<f32>(rail_h_id))
                        .unwrap_or(460.0);
                    let total_bottom = feed_bottom.max(row_top + 56.0 + rail_min_h);
                    let max_rail_top = (total_bottom - rail_min_h).max(row_top + 56.0);
                    let sticky_y = (viewport_top + 56.0).clamp(row_top + 56.0, max_rail_top);
                    let rail_rect = egui::Rect::from_min_max(
                        egui::pos2(rail_x, sticky_y),
                        egui::pos2(rail_x + rail_w, total_bottom + 200.0),
                    );
                    let mut rail_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt((scroll_id, "history_rail_col"))
                            .max_rect(rail_rect),
                    );
                    rail_outcome = self.history_management_rail_ui(
                        &mut rail_ui,
                        scroll_id,
                        rail_w,
                        pt,
                        &palette,
                    );
                    let measured_rail_h = rail_ui.min_rect().height().max(200.0);
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(rail_h_id, measured_rail_h));
                    let final_bottom = total_bottom.max(rail_ui.min_rect().bottom());
                    ui.allocate_space(egui::vec2(outer_w, (final_bottom - row_top).max(1.0)));
                } else {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if outer_side_pad > 0.0 {
                            ui.add_space(outer_side_pad);
                        }
                        ui.vertical(|ui| {
                            ui.set_width(content_w);
                            ui.set_max_width(content_w);
                            ui.add_space(8.0);
                            rail_outcome = self
                                .history_management_rail_ui(ui, scroll_id, rail_w, pt, &palette);
                            ui.add_space(10.0);
                            render_feed_column(self, ui, feed_col_w);
                        });
                    });
                }
            });
        });

        if toggle_live_chip {
            live_only = !live_only;
            ctx.data_mut(|d| d.insert_temp(live_chip_id, live_only));
            if live_only {
                self.history.filter = history::HistoryFilter::All;
            }
        }
        if let Some(filter) = clicked_chip {
            if live_only {
                live_only = false;
                ctx.data_mut(|d| d.insert_temp(live_chip_id, false));
            }
            self.history.filter = filter;
            if !cfg!(test)
                && filter != history::HistoryFilter::All
                && self.history.remote.loaded
                && !self
                    .history
                    .remote
                    .feeds
                    .get(&filter)
                    .is_some_and(|f| f.loaded)
                && self.history.remote.chips.contains_key(&filter)
            {
                self.start_history_chip_fetch(filter, false, &ctx);
            }
        }
        if let Some(filter) = load_more_filter {
            self.start_history_chip_fetch(filter, true, &ctx);
        }
        if clear_filter_clicked {
            self.history.search_query.clear();
            self.history.filter = history::HistoryFilter::All;
            if live_only {
                ctx.data_mut(|d| d.insert_temp(live_chip_id, false));
            }
        }
        if rail_outcome.toggle_pause_clicked {
            let mut next = self.settings.prefs.value.clone();
            next.pause_watch_history = !next.pause_watch_history;
            let now_paused = next.pause_watch_history;
            self.settings.apply_app_preferences(next);
            if !cfg!(test)
                && let Some(token) = self.history.remote.actions.pause_feedback_token.clone()
            {
                self.dispatch_history_feedback(token, &ctx);
                self.history.remote.stale = true;
            }
            self.toasts.info(match (now_paused, pt) {
                (true, true) => "Hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o pausado.",
                (true, false) => "Watch history paused.",
                (false, true) => "Hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o ativado.",
                (false, false) => "Watch history resumed.",
            });
        }
        if rail_outcome.clear_clicked {
            if self.history.remote.actions.clear_feedback_token.is_some() && !cfg!(test) {
                self.history.remote.confirm_remote_clear = true;
            } else {
                self.clear_history();
            }
        }
        if rail_outcome.confirm_clear_clicked {
            self.clear_history_with_remote(true, &ctx);
        }
        if rail_outcome.cancel_clear_clicked {
            self.history.remote.confirm_remote_clear = false;
        }
        if rail_outcome.sync_now_clicked {
            self.start_history_sync(true, false, &ctx);
        }
        if rail_outcome.connect_sync_clicked {
            self.start_history_sync(true, true, &ctx);
        }
        if rail_outcome.manage_clicked {
            self.ui.settings_open = true;
            self.settings.active_tab = crate::app_state::SettingsTab::Privacy;
        }
        if let Some(url) = rail_outcome.external_url {
            open_external_url(url);
        }
        if let Some(video) = to_enqueue
            && self.enqueue_video(video).is_ok()
        {
            let msg = self.pick("Adicionado \u{00e0} fila.", "Added to queue.");
            self.message = msg.into();
            self.toasts.success(msg);
        }
        if let Some(video) = toggle_watch_later {
            self.library.data.toggle_watch_later(video);
            self.save_library();
            let msg = self.pick(
                "Lista \"Assistir mais tarde\" atualizada.",
                "\"Watch Later\" list updated.",
            );
            self.message = msg.into();
            self.toasts.success(msg);
        }
        if let Some(video) = open_save_modal {
            self.feed.search_save_modal_video = Some(video);
            if self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
                && !self.auth.remote_playlists_task.is_running()
            {
                self.start_fetch_remote_playlists(&ctx);
            }
        }
        if let Some(video) = open_share_modal {
            self.open_share_modal(&video);
        }
        if let Some((video, fmt)) = download_req {
            self.start_download_video(video, fmt, &ctx);
        }
        if let Some(target) = open_channel {
            self.open_channel(target, &ctx);
        }
        if let Some(video_id) = to_remove {
            self.remove_history_entry_with_remote(&video_id, Some(&ctx));
        }
        chosen
    }

    pub(crate) fn history_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        max_height: Option<f32>,
    ) -> Option<Video> {
        if !compact {
            return self.history_youtube_page_ui(ui, scroll_id, max_card_width, max_height);
        }

        self.ensure_history_synchronized();
        let entry_count = self.history.data.entries().len();
        let pt = self.ui_pt();
        let palette = self.palette();

        let history_paused = self.settings.prefs.value.pause_watch_history;
        let mut clear_clicked = false;
        let mut toggle_pause_clicked = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.pick(
                    "Hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o",
                    "Watch history",
                ))
                .size(20.0)
                .strong(),
            );
            ui.label(
                RichText::new(format!("({entry_count} {})", self.pick("vídeos", "videos")))
                    .size(12.5)
                    .color(palette.neutral_status_fg),
            );
            if history_paused {
                ui.label(
                    RichText::new(if pt {
                        "\u{00b7} Pausado"
                    } else {
                        "\u{00b7} Paused"
                    })
                    .size(12.5)
                    .strong()
                    .color(palette.warning_fg),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if entry_count > 0
                    && styled_danger_button(
                        ui,
                        self.pick("Limpar hist\u{00f3}rico", "Clear watch history"),
                        &palette,
                    )
                    .on_hover_text(self.pick(
                        "Apagar todos os itens do hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o",
                        "Delete all watch history items",
                    ))
                    .clicked()
                {
                    clear_clicked = true;
                }
                let pause_label = match (history_paused, pt) {
                    (true, true) => "Ativar hist\u{00f3}rico",
                    (true, false) => "Resume watch history",
                    (false, true) => "Pausar hist\u{00f3}rico",
                    (false, false) => "Pause watch history",
                };
                if styled_compact_button(ui, pause_label, history_paused, &palette)
                    .on_hover_text(self.pick(
                        "Pausar ou retomar o registro de v\u{00ed}deos assistidos",
                        "Pause or resume recording watched videos",
                    ))
                    .clicked()
                {
                    toggle_pause_clicked = true;
                }
            });
        });
        ui.add_space(4.0);
        paint_section_hairline(ui, &palette);
        if toggle_pause_clicked {
            let mut next = self.settings.prefs.value.clone();
            next.pause_watch_history = !next.pause_watch_history;
            let now_paused = next.pause_watch_history;
            self.settings.apply_app_preferences(next);
            self.toasts.info(match (now_paused, pt) {
                (true, true) => "Hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o pausado.",
                (true, false) => "Watch history paused.",
                (false, true) => "Hist\u{00f3}rico de exibi\u{00e7}\u{00e3}o ativado.",
                (false, false) => "Watch history resumed.",
            });
        }
        if clear_clicked {
            self.clear_history();
            return None;
        }
        ui.add_space(6.0);

        if entry_count == 0 {
            empty_state_card_ui(
                ui,
                self.pick("Histórico vazio", "History is empty"),
                self.pick(
                    "Os vídeos que você assistir aparecerão aqui com retomada automática.",
                    "Videos you watch will appear here with automatic resume.",
                ),
                &palette,
            );
            return None;
        }

        let current_video_id = self
            .selected
            .as_ref()
            .and_then(|video| history::youtube_video_id(&video.url));

        let search_hint = self.pick("Filtrar por título ou canal", "Filter by title or channel");
        let filter_options = [
            (history::HistoryFilter::All, self.pick("Todos", "All")),
            (
                history::HistoryFilter::InProgress,
                self.pick("Em andamento", "In progress"),
            ),
            (
                history::HistoryFilter::Completed,
                self.pick("Concluídos", "Completed"),
            ),
        ];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            let search_w = if compact { 150.0 } else { 220.0 };
            ui.add_sized(
                [search_w, 26.0],
                egui::TextEdit::singleline(&mut self.history.search_query).hint_text(search_hint),
            );
            if !self.history.search_query.is_empty()
                && styled_icon_chip_button(ui, true, "✕", &palette).clicked()
            {
                self.history.search_query.clear();
            }
            for (filter, label) in filter_options {
                let selected = self.history.filter == filter;
                if styled_compact_button(ui, label, selected, &palette).clicked() {
                    self.history.filter = filter;
                }
            }
        });
        ui.add_space(6.0);

        let ctx = ui.ctx().clone();
        let compact_cache_id = egui::Id::new("fasttube_compact_history_display_cache");
        let compact_key = {
            let entries = self.history.data.entries();
            CompactHistoryCacheKey {
                filter: self.history.filter,
                search_query: self.history.search_query.trim().to_owned(),
                local_len: entries.len(),
                local_first_sig: entries.first().map(|e| {
                    (
                        e.video_id.clone(),
                        e.position_ms,
                        e.duration_ms,
                        e.watched_at_ms,
                    )
                }),
                local_last_sig: entries.last().map(|e| (e.video_id.clone(), e.position_ms)),
                local_minute_bucket: history::current_unix_epoch_ms() / 60_000,
            }
        };
        let cached_compact = ctx
            .data(|d| d.get_temp::<Arc<CachedCompactHistory>>(compact_cache_id))
            .filter(|c| c.key == compact_key)
            .unwrap_or_else(|| {
                let entries = self.history.data.entries();
                let filtered = history::filter_entries(
                    entries,
                    &self.history.search_query,
                    self.history.filter,
                );
                let now_ms = history::current_unix_epoch_ms();
                let offset_minutes = history::local_utc_offset_minutes();
                let groups = [
                    history::DateGroup::Today,
                    history::DateGroup::Yesterday,
                    history::DateGroup::ThisWeek,
                    history::DateGroup::Older,
                ];
                let mut buckets: [Vec<&HistoryEntry>; 4] =
                    [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
                let filtered_len = filtered.len();
                for entry in filtered {
                    let group = history::date_group_at(entry.watched_at_ms, now_ms, offset_minutes);
                    let slot = match group {
                        history::DateGroup::Today => 0,
                        history::DateGroup::Yesterday => 1,
                        history::DateGroup::ThisWeek => 2,
                        history::DateGroup::Older => 3,
                    };
                    buckets[slot].push(entry);
                }
                let mut rows: Vec<CompactHistoryRow> = Vec::with_capacity(filtered_len + 4);
                for (slot_idx, bucket) in buckets.iter().enumerate() {
                    if bucket.is_empty() {
                        continue;
                    }
                    rows.push(CompactHistoryRow::Header(groups[slot_idx]));
                    for entry in bucket {
                        rows.push(CompactHistoryRow::Item(Box::new((*entry).clone())));
                    }
                }
                let built = Arc::new(CachedCompactHistory {
                    key: compact_key,
                    rows,
                });
                ctx.data_mut(|d| d.insert_temp(compact_cache_id, Arc::clone(&built)));
                built
            });
        let rows = &cached_compact.rows;
        if rows.is_empty() {
            let mut clear_filter_clicked = false;
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12.0)
                .inner_margin(egui::Margin::symmetric(18, 16))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(if pt {
                                "Nenhum vídeo corresponde ao filtro."
                            } else {
                                "No videos match the filter."
                            })
                            .size(14.5)
                            .strong()
                            .color(palette.channel_fg),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(if pt {
                                "Tente outro termo de busca ou redefina os filtros ativos."
                            } else {
                                "Try another search term or reset active filters."
                            })
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                        );
                        ui.add_space(8.0);
                        if styled_ghost_button(
                            ui,
                            if pt { "Limpar filtro" } else { "Clear filter" },
                            &palette,
                        )
                        .clicked()
                        {
                            clear_filter_clicked = true;
                        }
                    });
                });
            if clear_filter_clicked {
                self.history.search_query.clear();
                self.history.filter = history::HistoryFilter::All;
            }
            return None;
        }

        let mut chosen = None;
        let mut to_remove = None;
        let mut to_enqueue = None;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, true]);
        if let Some(limit) = max_height {
            scroll = scroll.max_height(limit);
        }
        let row_height = if compact { 82.0 } else { 86.0 };
        self.thumbnails.begin_pass();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
                let target_width = max_card_width
                    .map(|limit| ui.available_width().min(limit))
                    .unwrap_or_else(|| ui.available_width());
                for row_idx in range {
                    match &rows[row_idx] {
                        CompactHistoryRow::Header(group) => {
                            ui.push_id(("hist_group", row_idx), |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(target_width, row_height),
                                    egui::Layout::bottom_up(egui::Align::Min),
                                    |ui| {
                                        ui.set_min_height(row_height);
                                        ui.set_max_height(row_height);
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(group.label(pt))
                                                    .size(12.5)
                                                    .strong()
                                                    .color(palette.neutral_status_fg),
                                            );
                                            ui.add_space(8.0);
                                            paint_section_hairline(ui, &palette);
                                        });
                                    },
                                );
                            });
                        }
                        CompactHistoryRow::Item(entry) => {
                            let is_playing =
                                current_video_id.as_deref() == Some(entry.video_id.as_str());
                            ui.push_id(&entry.video_id, |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(target_width, row_height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_min_height(row_height);
                                        ui.set_max_height(row_height);
                                        let (play_clicked, remove_clicked, enqueue_clicked) = self
                                            .history_entry_card_ui(ui, entry, compact, is_playing);
                                        if play_clicked {
                                            chosen = self.video_for_history_entry(entry);
                                        }
                                        if enqueue_clicked {
                                            to_enqueue = self.video_for_history_entry(entry);
                                        }
                                        if remove_clicked {
                                            to_remove = Some(entry.video_id.clone());
                                        }
                                    },
                                );
                            });
                        }
                    }
                }
            });
        });
        self.thumbnails.end_pass(&ctx);
        if let Some(video) = to_enqueue {
            let _ = self.enqueue_video(video);
        }
        if let Some(video_id) = to_remove {
            self.remove_history_entry(&video_id);
        }
        chosen
    }
}
