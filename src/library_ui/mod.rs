mod downloads_view;
mod history_view;
mod insights_view;
mod playlists_view;
mod queue_view;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use downloads_view::parse_direct_youtube_input;
#[cfg(test)]
pub(crate) use history_view::{
    HistoryDisplayCacheKey, HistoryDisplaySection, resolve_history_display_cache,
};
#[cfg(test)]
pub(crate) use insights_view::{LocalInsightsCacheKey, resolve_local_insights_cache};
#[cfg(test)]
pub(crate) use playlists_view::derive_playlist_gradient_stops;
#[cfg(test)]
use playlists_view::remove_liked_video_from_state;

use crate::browse_ui::{compute_uniform_visible_row_range, compute_variable_visible_range};
use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, RichText};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

fn fnv1a_mix(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

pub(super) fn bucket_revision_cache_tag(
    bucket_rev: u64,
    first_url: Option<&str>,
    last_url: Option<&str>,
) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    fnv1a_mix(&mut hash, &bucket_rev.to_le_bytes());
    if let Some(url) = first_url {
        fnv1a_mix(&mut hash, url.as_bytes());
    }
    fnv1a_mix(&mut hash, &[0xff]);
    if let Some(url) = last_url {
        fnv1a_mix(&mut hash, url.as_bytes());
    }
    hash
}

pub(super) fn collections_revision_cache_tag(
    collections_rev: u64,
    collections: &[library::Collection],
) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    fnv1a_mix(&mut hash, &collections_rev.to_le_bytes());
    fnv1a_mix(&mut hash, &collections.len().to_le_bytes());
    let total_videos: usize = collections.iter().map(|c| c.videos.len()).sum();
    fnv1a_mix(&mut hash, &total_videos.to_le_bytes());
    if let Some(first) = collections.first() {
        fnv1a_mix(&mut hash, first.name.as_bytes());
        fnv1a_mix(&mut hash, &first.videos.len().to_le_bytes());
    }
    if let Some(last) = collections.last() {
        fnv1a_mix(&mut hash, last.name.as_bytes());
        fnv1a_mix(&mut hash, &last.videos.len().to_le_bytes());
    }
    hash
}

pub(super) fn str_fingerprint(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    fnv1a_mix(&mut hash, input.as_bytes());
    hash
}

pub(crate) fn library_uniform_visible_row_range(
    total_rows: usize,
    row_height: f32,
    row_spacing: f32,
    list_top: f32,
    clip_top: f32,
    clip_bottom: f32,
    overscan_rows: usize,
) -> (Range<usize>, f32, f32) {
    compute_uniform_visible_row_range(
        total_rows,
        row_height,
        row_spacing,
        list_top,
        clip_top,
        clip_bottom,
        overscan_rows,
    )
}

pub(crate) fn library_variable_visible_range(
    item_heights: &[f32],
    spacing: f32,
    list_top: f32,
    clip_top: f32,
    clip_bottom: f32,
    overscan_items: usize,
) -> (Range<usize>, f32, f32) {
    compute_variable_visible_range(
        item_heights,
        spacing,
        list_top,
        clip_top,
        clip_bottom,
        overscan_items,
    )
}

#[derive(Clone, PartialEq, Eq)]
struct LibraryOverviewCacheKey {
    library_revision: u64,
    bucket_revisions: (u64, u64, u64, u64),
    counts: (usize, usize, usize, usize),
    remote_revisions: (u64, u64, usize, usize),
    history_len: usize,
    history_first_sig: Option<(String, u64, Option<u64>, Option<u64>)>,
    lib_query: String,
    bm_query: String,
}

#[derive(Clone)]
struct CachedLibraryOverview {
    key: LibraryOverviewCacheKey,
    watch_later: Vec<Video>,
    favorites: Vec<Video>,
    collections: Vec<(usize, String, Vec<Video>, usize)>,
    bookmarks: Vec<(usize, library::Bookmark)>,
    recent_history: Vec<HistoryEntry>,
}

pub(crate) fn clamp_shelf_offset(
    current: usize,
    total_items: usize,
    page_size: usize,
) -> (usize, usize) {
    let page = page_size.max(1);
    let max_offset = total_items.saturating_sub(page);
    let start = current.min(max_offset);
    let end = (start + page).min(total_items);
    (start, end)
}

pub(crate) fn paint_section_hairline(ui: &mut egui::Ui, palette: &ThemePalette) {
    let width = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.center().y),
            egui::pos2(rect.right(), rect.center().y),
        ],
        egui::Stroke::new(1.0, palette.card_alt_stroke.gamma_multiply(0.65)),
    );
}

pub(super) fn paint_library_card_hover_glow(
    ui: &egui::Ui,
    rect: egui::Rect,
    corner_radius: f32,
    id: egui::Id,
    palette: &ThemePalette,
) {
    let pointer_pos = ui.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
    let hovered = pointer_pos.is_some_and(|pos| rect.contains(pos));
    let hover_t = ui.ctx().animate_bool_with_time(id, hovered, 0.14);
    if hover_t > 0.01 {
        ui.painter().rect_stroke(
            rect,
            corner_radius,
            egui::Stroke::new(
                1.25,
                palette
                    .info_accent_fg
                    .gamma_multiply(0.35 * hover_t.clamp(0.0, 1.0)),
            ),
            egui::StrokeKind::Inside,
        );
    }
}

pub(super) fn empty_state_card_ui(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    palette: &ThemePalette,
) {
    egui::Frame::new()
        .fill(palette.card_alt_bg)
        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(title)
                        .size(14.5)
                        .strong()
                        .color(palette.channel_fg),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(hint)
                        .size(12.5)
                        .color(palette.neutral_status_fg),
                );
            });
        });
}

impl FastTube {
    pub(crate) fn library_video_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        title: &str,
        description: &str,
        videos: &[Video],
        scroll_id: &str,
    ) -> (Option<Video>, Option<String>) {
        let pt = self.ui_pt();
        let palette = self.palette();
        let mut chosen = None;
        let mut remove_url = None;
        let mut play_all_clicked = false;
        let mut enqueue_all_clicked = false;
        let shelf_page_size = 8usize;
        let shelf_offset_id = egui::Id::new(("fasttube_lib_shelf_offset", scroll_id));
        let raw_offset = ctx.data(|d| d.get_temp::<usize>(shelf_offset_id).unwrap_or(0));
        let (mut shelf_offset, _) = clamp_shelf_offset(raw_offset, videos.len(), shelf_page_size);
        let mut next_offset = shelf_offset;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(title).size(20.0).strong());
            ui.label(
                RichText::new(format!(
                    "({} {})",
                    videos.len(),
                    if pt { "itens" } else { "items" }
                ))
                .size(12.5)
                .color(palette.neutral_status_fg),
            );
            if !videos.is_empty() {
                if styled_pill_button(
                    ui,
                    if pt {
                        "▶ Reproduzir tudo"
                    } else {
                        "▶ Play all"
                    },
                    true,
                    &palette,
                )
                .on_hover_text(if pt {
                    "Reproduzir o primeiro vídeo e enfileirar os restantes"
                } else {
                    "Play the first video and enqueue the rest"
                })
                .clicked()
                {
                    play_all_clicked = true;
                }
                if styled_pill_button(
                    ui,
                    if pt { "+ Fila (tudo)" } else { "+ Queue (all)" },
                    false,
                    &palette,
                )
                .on_hover_text(if pt {
                    "Adicionar todos os vídeos desta lista à fila"
                } else {
                    "Add all videos from this list to the queue"
                })
                .clicked()
                {
                    enqueue_all_clicked = true;
                }
                let can_prev = shelf_offset > 0;
                let can_next = shelf_offset + shelf_page_size < videos.len();
                if styled_icon_chip_button(ui, can_prev, "‹", &palette)
                    .on_hover_text(if pt {
                        "Página anterior da estante"
                    } else {
                        "Previous shelf page"
                    })
                    .clicked()
                {
                    next_offset = clamp_shelf_offset(
                        shelf_offset.saturating_sub(4),
                        videos.len(),
                        shelf_page_size,
                    )
                    .0;
                }
                if styled_icon_chip_button(ui, can_next, "›", &palette)
                    .on_hover_text(if pt {
                        "Próxima página da estante"
                    } else {
                        "Next shelf page"
                    })
                    .clicked()
                {
                    next_offset =
                        clamp_shelf_offset(shelf_offset + 4, videos.len(), shelf_page_size).0;
                }
            }
        });
        if next_offset != raw_offset {
            shelf_offset = next_offset;
            ctx.data_mut(|d| d.insert_temp(shelf_offset_id, shelf_offset));
        }
        ui.add_space(4.0);
        paint_section_hairline(ui, &palette);
        if play_all_clicked {
            if let Some(first) = self.play_all_videos(videos) {
                chosen = Some(first);
            }
        } else if enqueue_all_clicked {
            let (added, rejected) = self.enqueue_all_videos(videos);
            if added > 0 || rejected > 0 {
                self.library.notice = self
                    .queue
                    .notice
                    .clone()
                    .or_else(|| self.queue.error.clone());
            }
        }
        ui.add_space(6.0);

        if videos.is_empty() {
            empty_state_card_ui(ui, title, description, &palette);
            return (chosen, None);
        }

        let visible_end = (shelf_offset + shelf_page_size).min(videos.len());
        let videos = &videos[shelf_offset..visible_end];

        self.thumbnails.begin_pass();
        let avail = ui.available_width().max(260.0);
        let cols: usize = if avail >= 740.0 { 2 } else { 1 };
        let col_gap = 12.0;
        let card_w = ((avail - col_gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(240.0);
        let total_rows = videos.len().div_ceil(cols);
        let row_height = if cols == 1 { 156.0 } else { 148.0 };
        let row_gap = 10.0;
        let inner_spacing_y = ui.spacing().item_spacing.y;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let list_top = ui.cursor().top();
            let clip = ui.clip_rect();
            let (visible_rows, top_spacer_h, bottom_spacer_h) = library_uniform_visible_row_range(
                total_rows,
                row_height,
                row_gap,
                list_top,
                clip.top(),
                clip.bottom(),
                2,
            );
            if top_spacer_h > 0.0 {
                ui.allocate_exact_size(egui::vec2(avail, top_spacer_h), egui::Sense::hover());
            }
            let rendered_end = visible_rows.end;
            for row_idx in visible_rows {
                let slice_start = row_idx * cols;
                let slice_end = (slice_start + cols).min(videos.len());
                ui.allocate_ui_with_layout(
                    egui::vec2(avail, row_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = inner_spacing_y;
                        ui.set_min_height(row_height);
                        ui.set_max_height(row_height);
                        ui.spacing_mut().item_spacing.x = col_gap;
                        for video in &videos[slice_start..slice_end] {
                            ui.push_id((scroll_id, &video.url), |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(card_w, row_height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_min_height(row_height);
                                        ui.set_max_height(row_height);
                                        let frame_resp = egui::Frame::new()
                                            .fill(palette.card_alt_bg)
                                            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                            .corner_radius(10.0)
                                            .inner_margin(egui::Margin::symmetric(12, 8))
                                            .show(ui, |ui| {
                                                let inner_w = (card_w - 24.0).max(160.0);
                                                let inner_h = (row_height - 16.0).max(80.0);
                                                ui.set_min_size(egui::vec2(inner_w, inner_h));
                                                ui.set_max_size(egui::vec2(inner_w, inner_h));
                                                ui.with_layout(
                                                    egui::Layout::bottom_up(egui::Align::Min),
                                                    |ui| {
                                                        if styled_ghost_button(
                                                            ui,
                                                            if pt {
                                                                "✕ Remover"
                                                            } else {
                                                                "✕ Remove"
                                                            },
                                                            &palette,
                                                        )
                                                        .on_hover_text(if pt {
                                                            "Remover este vídeo da lista"
                                                        } else {
                                                            "Remove this video from the list"
                                                        })
                                                        .clicked()
                                                        {
                                                            remove_url = Some(video.url.clone());
                                                        }
                                                        ui.add_space(4.0);
                                                        ui.with_layout(
                                                            egui::Layout::top_down(
                                                                egui::Align::Min,
                                                            ),
                                                            |ui| {
                                                                if self
                                                                    .watch_recommendation_card_ui(
                                                                        ui, ctx, video, true,
                                                                    )
                                                                {
                                                                    chosen = Some(video.clone());
                                                                }
                                                            },
                                                        );
                                                    },
                                                );
                                            });
                                        paint_library_card_hover_glow(
                                            ui,
                                            frame_resp.response.rect,
                                            10.0,
                                            ui.make_persistent_id((
                                                "lib_video_card_hover",
                                                scroll_id,
                                                &video.url,
                                            )),
                                            &palette,
                                        );
                                    },
                                );
                            });
                        }
                    },
                );
                if row_idx + 1 < rendered_end {
                    ui.add_space(row_gap);
                }
            }
            if bottom_spacer_h > 0.0 {
                ui.allocate_exact_size(egui::vec2(avail, bottom_spacer_h), egui::Sense::hover());
            }
        });
        self.thumbnails.end_pass(ctx);
        (chosen, remove_url)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn library_header_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        watch_later_count: usize,
        favorites_count: usize,
        collections_count: usize,
        bookmarks_count: usize,
        palette: ThemePalette,
    ) {
        let pt = self.ui_pt();
        let mut open_history = false;
        let mut open_downloads = false;
        let mut open_liked = false;
        let mut open_playlists = false;
        let mut open_watch_later = false;
        let mut open_my_channel = false;

        let display_name = self
            .auth
            .my_channel_title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| if pt { "Você" } else { "You" }.to_owned());
        let handle_label = match &self.auth.my_channel {
            Some(ChannelTarget::Handle(h)) => h.clone(),
            Some(ChannelTarget::ChannelId(id)) => id.clone(),
            None => "@voce".to_owned(),
        };

        let my_avatar = self.auth.my_channel_avatar_url.clone();
        ui.horizontal_top(|ui| {
            let _ = self.paint_avatar_or_initials(
                ui,
                ctx,
                my_avatar.as_deref(),
                &display_name,
                56.0,
                false,
            );
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        RichText::new(&display_name)
                            .size(26.0)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    if styled_compact_button(
                        ui,
                        if pt { "Histórico" } else { "History" },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        open_history = true;
                    }
                    if styled_compact_button(ui, "Playlists", false, &palette).clicked() {
                        open_playlists = true;
                    }
                    if styled_compact_button(
                        ui,
                        if pt {
                            "Assistir mais tarde"
                        } else {
                            "Watch later"
                        },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        open_watch_later = true;
                    }
                    if styled_compact_button(
                        ui,
                        if pt {
                            "Vídeos com \"Gostei\""
                        } else {
                            "Liked videos"
                        },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        open_liked = true;
                    }
                    if styled_compact_button(ui, "Downloads", false, &palette).clicked() {
                        open_downloads = true;
                    }
                });
                ui.add_space(2.0);
                let mut stats = vec![
                    if pt {
                        format!("{watch_later_count} em Assistir mais tarde")
                    } else {
                        format!("{watch_later_count} in Watch later")
                    },
                    format!("{collections_count} playlists"),
                ];
                if favorites_count > 0 {
                    stats.push(if pt {
                        format!("{favorites_count} favoritos")
                    } else {
                        format!("{favorites_count} favorites")
                    });
                }
                if bookmarks_count > 0 {
                    stats.push(if pt {
                        format!("{bookmarks_count} marcadores")
                    } else {
                        format!("{bookmarks_count} bookmarks")
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let link_label = if pt {
                        format!("{handle_label} · Ver canal ›")
                    } else {
                        format!("{handle_label} · View channel ›")
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(link_label)
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            )
                            .frame(false),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        open_my_channel = true;
                    }
                    ui.label(
                        RichText::new(format!("·  {}", stats.join(" · ")))
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                    );
                });
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [300.0, 28.0],
                egui::TextEdit::singleline(&mut self.library.search_query).hint_text(if pt {
                    "Filtrar playlists e salvos por título ou canal..."
                } else {
                    "Filter playlists and saved videos..."
                }),
            );
            if !self.library.search_query.is_empty()
                && styled_ghost_button(ui, if pt { "✕ Limpar" } else { "✕ Clear" }, &palette)
                    .clicked()
            {
                self.library.search_query.clear();
            }
        });
        let open_history = if open_history {
            self.on_enter_history_section(ctx);
            true
        } else {
            false
        };
        if open_history {
            self.ui.browse_section = BrowseSection::History;
        } else if open_downloads {
            self.ui.browse_section = BrowseSection::Downloads;
        } else if open_playlists {
            self.library.selected_collection_idx = None;
            self.library.active_playlist_detail.playlist_id = None;
            self.ui.browse_section = BrowseSection::Playlists;
            self.on_enter_playlists_section(ctx);
        } else if open_watch_later {
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = BrowseSection::WatchLater;
            self.on_enter_watch_later_section(ctx);
        } else if open_liked {
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = BrowseSection::LikedVideos;
            self.start_liked_videos_fetch(false, ctx);
        } else if open_my_channel {
            if let Some(target) = self.auth.my_channel.clone() {
                self.open_channel(target, ctx);
            } else if self.auth.authenticated {
                let placeholder = search::ChannelTarget::Handle("@voce".to_owned());
                let mut page = search::empty_channel_page_for_target(&placeholder);
                page.title = self
                    .auth
                    .my_channel_title
                    .clone()
                    .or_else(|| Some(if pt { "Seu canal" } else { "Your channel" }.to_owned()));
                page.avatar_url = self.auth.my_channel_avatar_url.clone();
                page.subscriber_count = self.auth.my_channel_subscribers;
                self.apply_channel_page(ctx, page);
                self.start_fetch_my_channel(true, ctx);
            } else {
                let local_target = search::ChannelTarget::Handle("@voce".to_owned());
                let mut page = search::empty_channel_page_for_target(&local_target);
                page.title = Some(if pt { "Seu canal" } else { "Your channel" }.to_owned());
                self.apply_channel_page(ctx, page);
            }
        }
        if let Some(error) = &self.library.error {
            ui.add_space(6.0);
            ui.colored_label(palette.warning_fg, error);
        }
        if let Some(notice) = self.library.notice.clone() {
            ui.add_space(6.0);
            let mut dismiss = false;
            egui::Frame::new()
                .fill(palette.notice_bg)
                .stroke(egui::Stroke::new(1.0, palette.notice_stroke))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(notice).small().color(palette.notice_fg));
                        if styled_icon_chip_button(ui, true, "✕", &palette).clicked() {
                            dismiss = true;
                        }
                    });
                });
            if dismiss {
                self.library.notice = None;
            }
        }
        ui.add_space(10.0);
        paint_section_hairline(ui, &palette);
        ui.add_space(12.0);
    }

    pub(crate) fn library_collections_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        collections: &[(usize, String, Vec<Video>, usize)],
        total_collections: usize,
        has_filter: bool,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Playlists").size(20.0).strong());
            ui.label(
                RichText::new(format!("({total_collections})"))
                    .size(12.5)
                    .color(palette.neutral_status_fg),
            );
        });
        ui.add_space(4.0);
        paint_section_hairline(ui, &palette);
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [240.0, 34.0],
                        egui::TextEdit::singleline(&mut self.library.collection_name).hint_text(
                            if pt {
                                "Nome da nova coleção"
                            } else {
                                "New collection name"
                            },
                        ),
                    );
                    let create = egui::Button::new(
                        RichText::new(if pt { "Criar" } else { "Create" })
                            .size(13.0)
                            .strong()
                            .color(palette.active_button_fg),
                    )
                    .fill(palette.active_button_bg)
                    .corner_radius(18);
                    if ui.add_sized([72.0, 34.0], create).clicked() {
                        let name_input = self.library.collection_name.clone();
                        match self.library.data.create_collection(&name_input) {
                            Ok(col_idx) => {
                                let created = self.library.data.collections[col_idx].name.clone();
                                self.library.collection_name.clear();
                                self.library.error = None;
                                self.library.notice = Some(if pt {
                                    format!("Coleção \"{created}\" criada.")
                                } else {
                                    format!("Collection \"{created}\" created.")
                                });
                                self.save_library();
                            }
                            Err(err) => {
                                self.library.error = Some(err.to_owned());
                            }
                        }
                    }
                });
            });
        ui.add_space(10.0);

        if collections.is_empty() {
            let (empty_title, empty_hint) = if has_filter {
                (
                    if pt {
                        "Nenhuma coleção encontrada"
                    } else {
                        "No collections found"
                    },
                    if pt {
                        "Nenhuma coleção corresponde ao filtro."
                    } else {
                        "No collections match the filter."
                    },
                )
            } else {
                (
                    if pt {
                        "Nenhuma coleção criada"
                    } else {
                        "No collections yet"
                    },
                    if pt {
                        "Crie coleções para organizar vídeos por assunto ou projeto."
                    } else {
                        "Create collections to organize videos by topic or project."
                    },
                )
            };
            empty_state_card_ui(ui, empty_title, empty_hint, &palette);
        } else {
            for (col_idx, name, videos, total_in_col) in collections {
                let frame_resp = egui::Frame::new()
                    .fill(palette.info_box_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.push_id(("library_col_controls", *col_idx), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if self.library.renaming_collection == Some(*col_idx) {
                                    ui.add_sized(
                                        [200.0, 28.0],
                                        egui::TextEdit::singleline(&mut self.library.rename_draft)
                                            .hint_text(if pt {
                                                "Novo nome da coleção"
                                            } else {
                                                "New collection name"
                                            }),
                                    );
                                    if styled_compact_button(
                                        ui,
                                        if pt { "Salvar" } else { "Save" },
                                        true,
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        let draft = self.library.rename_draft.clone();
                                        match self.library.data.rename_collection(*col_idx, &draft)
                                        {
                                            Ok(()) => {
                                                self.library.renaming_collection = None;
                                                self.library.rename_draft.clear();
                                                self.library.error = None;
                                                self.library.notice = Some(
                                                    if pt {
                                                        "Coleção renomeada."
                                                    } else {
                                                        "Collection renamed."
                                                    }
                                                    .to_owned(),
                                                );
                                                self.save_library();
                                            }
                                            Err(err) => {
                                                self.library.error = Some(err.to_owned());
                                            }
                                        }
                                    }
                                    if styled_ghost_button(
                                        ui,
                                        if pt { "Cancelar" } else { "Cancel" },
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        self.library.renaming_collection = None;
                                        self.library.rename_draft.clear();
                                    }
                                } else if self.library.confirm_delete_collection == Some(*col_idx) {
                                    ui.label(
                                        RichText::new(if pt {
                                            format!(
                                                "Excluir coleção \"{name}\" ({total_in_col} itens)?"
                                            )
                                        } else {
                                            format!(
                                                "Delete collection \"{name}\" ({total_in_col} items)?"
                                            )
                                        })
                                        .size(12.5)
                                        .color(palette.warning_fg),
                                    );
                                    if styled_danger_button(
                                        ui,
                                        if pt {
                                            "Confirmar exclusão"
                                        } else {
                                            "Confirm delete"
                                        },
                                        &palette,
                                    )
                                    .clicked()
                                        && self.library.data.delete_collection(*col_idx).is_some()
                                    {
                                        self.library.confirm_delete_collection = None;
                                        self.library.renaming_collection = None;
                                        self.library.error = None;
                                        self.library.notice = Some(if pt {
                                            format!("Coleção \"{name}\" excluída.")
                                        } else {
                                            format!("Collection \"{name}\" deleted.")
                                        });
                                        self.save_library();
                                    }
                                    if styled_ghost_button(
                                        ui,
                                        if pt { "Cancelar" } else { "Cancel" },
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        self.library.confirm_delete_collection = None;
                                    }
                                } else {
                                    if styled_compact_button(
                                        ui,
                                        if pt { "Renomear" } else { "Rename" },
                                        false,
                                        &palette,
                                    )
                                    .on_hover_text(if pt {
                                        "Renomear esta coleção"
                                    } else {
                                        "Rename this collection"
                                    })
                                    .clicked()
                                    {
                                        self.library.renaming_collection = Some(*col_idx);
                                        self.library.rename_draft = name.clone();
                                        self.library.confirm_delete_collection = None;
                                    }
                                    if styled_danger_button(
                                        ui,
                                        if pt {
                                            "Excluir coleção"
                                        } else {
                                            "Delete collection"
                                        },
                                        &palette,
                                    )
                                    .on_hover_text(if pt {
                                        "Excluir esta coleção da biblioteca"
                                    } else {
                                        "Delete this collection from library"
                                    })
                                    .clicked()
                                    {
                                        self.library.confirm_delete_collection = Some(*col_idx);
                                        self.library.renaming_collection = None;
                                    }
                                }
                            });
                        });
                        ui.add_space(6.0);
                        let empty_msg = if has_filter {
                            if pt {
                                "Nenhum vídeo desta coleção corresponde ao filtro."
                            } else {
                                "No videos in this collection match the filter."
                            }
                        } else if pt {
                            "Esta coleção ainda não tem vídeos."
                        } else {
                            "This collection has no videos yet."
                        };
                        let (section_chosen, removed_url) = self.library_video_section_ui(
                            ui,
                            ctx,
                            name,
                            empty_msg,
                            videos,
                            &format!("library_collection_{col_idx}"),
                        );
                        if let Some(video) = section_chosen {
                            chosen = Some(video);
                        }
                        if let Some(url) = removed_url
                            && self.library.data.remove_from_collection(*col_idx, &url)
                        {
                            self.library.notice = Some(if pt {
                                format!("Vídeo removido de \"{name}\".")
                            } else {
                                format!("Video removed from \"{name}\".")
                            });
                            self.save_library();
                        }
                    });
                paint_library_card_hover_glow(
                    ui,
                    frame_resp.response.rect,
                    10.0,
                    ui.make_persistent_id(("lib_collection_card_hover", *col_idx)),
                    &palette,
                );
                ui.add_space(12.0);
            }
        }
        chosen
    }

    pub(crate) fn library_bookmarks_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        bookmarks: &[(usize, library::Bookmark)],
        total_bookmarks: usize,
        palette: ThemePalette,
    ) -> Option<library::Bookmark> {
        let pt = self.ui_pt();
        let mut bookmark_to_open = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if pt { "Marcadores" } else { "Bookmarks" })
                    .size(20.0)
                    .strong(),
            );
            ui.label(
                RichText::new(format!("({}/{})", bookmarks.len(), total_bookmarks))
                    .size(12.5)
                    .color(palette.neutral_status_fg),
            );
            if styled_compact_button_enabled(
                ui,
                !bookmarks.is_empty(),
                if pt {
                    "Copiar marcadores (Markdown)"
                } else {
                    "Copy bookmarks (Markdown)"
                },
                false,
                &palette,
            )
            .on_hover_text(if pt {
                "Copiar marcadores filtrados no formato Markdown"
            } else {
                "Copy filtered bookmarks as Markdown"
            })
            .clicked()
            {
                let visible_marks: Vec<library::Bookmark> =
                    bookmarks.iter().map(|(_, mark)| mark.clone()).collect();
                let md = library::format_bookmarks_markdown(&visible_marks);
                let _ = copy_text_to_clipboard(ctx, &md);
                self.library.notice = Some(if pt {
                    format!(
                        "{} marcador(es) copiado(s) em Markdown.",
                        visible_marks.len()
                    )
                } else {
                    format!("{} bookmark(s) copied as Markdown.", visible_marks.len())
                });
            }
        });
        ui.add_space(4.0);
        paint_section_hairline(ui, &palette);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [260.0, 28.0],
                egui::TextEdit::singleline(&mut self.library.bookmark_filter).hint_text(if pt {
                    "Filtrar marcadores por nota ou vídeo..."
                } else {
                    "Filter bookmarks by note or video..."
                }),
            );
            if !self.library.bookmark_filter.is_empty()
                && styled_ghost_button(ui, if pt { "✕ Limpar" } else { "✕ Clear" }, &palette)
                    .clicked()
            {
                self.library.bookmark_filter.clear();
            }
        });
        ui.add_space(6.0);
        if bookmarks.is_empty() {
            let (empty_title, empty_msg) = if total_bookmarks > 0 {
                (
                    if pt {
                        "Nenhum marcador encontrado"
                    } else {
                        "No bookmarks found"
                    },
                    if pt {
                        "Nenhum marcador corresponde ao filtro."
                    } else {
                        "No bookmarks match the filter."
                    },
                )
            } else {
                (
                    if pt {
                        "Nenhum marcador salvo"
                    } else {
                        "No bookmarks saved"
                    },
                    if pt {
                        "Marcadores criados durante a reprodução aparecerão aqui."
                    } else {
                        "Bookmarks created during playback will appear here."
                    },
                )
            };
            empty_state_card_ui(ui, empty_title, empty_msg, &palette);
        } else {
            let est_row_h = 62.0_f32;
            let row_gap = 6.0_f32;
            let row_spacing_y = ui.spacing().item_spacing.y;
            let visible_rect = ui.clip_rect().expand2(egui::vec2(0.0, 260.0));
            let avail_w = ui.available_width().max(220.0);
            let mut skipped_h = 0.0_f32;
            for (orig_idx, mark) in bookmarks {
                let is_editing = self.library.editing_bookmark_idx == Some(*orig_idx);
                let cursor_top = ui.cursor().top() + skipped_h;
                if !is_editing
                    && (cursor_top + est_row_h < visible_rect.top()
                        || cursor_top > visible_rect.bottom())
                {
                    skipped_h += est_row_h + row_gap + row_spacing_y;
                    continue;
                }
                if skipped_h > 0.0 {
                    ui.allocate_exact_size(egui::vec2(avail_w, skipped_h), egui::Sense::hover());
                    skipped_h = 0.0;
                }
                ui.push_id(("library_bookmark_row", *orig_idx), |ui| {
                    let frame_resp = egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(10.0)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            let timestamp = format_time(mark.time_ms as f64 / 1000.0);
                            if is_editing {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(&timestamp)
                                            .size(13.0)
                                            .strong()
                                            .color(palette.header_button_fg),
                                    );
                                    ui.add_sized(
                                        [220.0, 26.0],
                                        egui::TextEdit::singleline(
                                            &mut self.library.editing_bookmark_note,
                                        )
                                        .hint_text(if pt {
                                            "Nota do marcador"
                                        } else {
                                            "Bookmark note"
                                        }),
                                    );
                                    if styled_compact_button(
                                        ui,
                                        if pt { "Salvar" } else { "Save" },
                                        true,
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        let new_note = self.library.editing_bookmark_note.clone();
                                        match self
                                            .library
                                            .data
                                            .update_bookmark_note(*orig_idx, &new_note)
                                        {
                                            Ok(()) => {
                                                self.library.editing_bookmark_idx = None;
                                                self.library.editing_bookmark_note.clear();
                                                self.library.error = None;
                                                self.library.notice = Some(
                                                    if pt {
                                                        "Nota do marcador atualizada."
                                                    } else {
                                                        "Bookmark note updated."
                                                    }
                                                    .to_owned(),
                                                );
                                                self.save_library();
                                            }
                                            Err(err) => {
                                                self.library.error = Some(err.to_owned());
                                            }
                                        }
                                    }
                                    if styled_ghost_button(
                                        ui,
                                        if pt { "Cancelar" } else { "Cancel" },
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        self.library.editing_bookmark_idx = None;
                                        self.library.editing_bookmark_note.clear();
                                    }
                                });
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(format!(
                                                    "{timestamp}  {}",
                                                    mark.note
                                                ))
                                                .size(13.0)
                                                .strong()
                                                .color(palette.header_button_fg),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_text(&mark.title)
                                        .clicked()
                                    {
                                        bookmark_to_open = Some(mark.clone());
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if styled_icon_chip_button(ui, true, "✕", &palette)
                                                .on_hover_text(if pt {
                                                    "Remover marcador"
                                                } else {
                                                    "Remove bookmark"
                                                })
                                                .clicked()
                                                && self
                                                    .library
                                                    .data
                                                    .remove_bookmark(*orig_idx)
                                                    .is_some()
                                            {
                                                if let Some(edit_i) =
                                                    self.library.editing_bookmark_idx
                                                {
                                                    if edit_i == *orig_idx {
                                                        self.library.editing_bookmark_idx = None;
                                                        self.library.editing_bookmark_note.clear();
                                                    } else if edit_i > *orig_idx {
                                                        self.library.editing_bookmark_idx =
                                                            Some(edit_i - 1);
                                                    }
                                                }
                                                self.save_library();
                                            }
                                            if styled_compact_button(
                                                ui,
                                                if pt { "Editar" } else { "Edit" },
                                                false,
                                                &palette,
                                            )
                                            .on_hover_text(if pt {
                                                "Editar nota do marcador"
                                            } else {
                                                "Edit bookmark note"
                                            })
                                            .clicked()
                                            {
                                                self.library.editing_bookmark_idx = Some(*orig_idx);
                                                self.library.editing_bookmark_note =
                                                    mark.note.clone();
                                            }
                                        },
                                    );
                                });
                            }
                            ui.label(
                                RichText::new(&mark.title)
                                    .size(12.0)
                                    .color(palette.neutral_status_fg),
                            );
                        });
                    paint_library_card_hover_glow(
                        ui,
                        frame_resp.response.rect,
                        10.0,
                        ui.make_persistent_id(("lib_bookmark_card_hover", *orig_idx)),
                        &palette,
                    );
                });
                ui.add_space(6.0);
            }
            if skipped_h > 0.0 {
                ui.allocate_exact_size(egui::vec2(avail_w, skipped_h), egui::Sense::hover());
            }
        }
        bookmark_to_open
    }

    pub(crate) fn resolve_video_for_bookmark(&self, mark: &library::Bookmark) -> Option<Video> {
        let target_id = mark.video_id.as_str();
        let matches_id =
            |video: &Video| history::youtube_video_id(&video.url).as_deref() == Some(target_id);
        if let Some(video) = self.feed.videos.iter().find(|v| matches_id(v)) {
            return Some(video.clone());
        }
        if let Some(record) = self
            .library
            .data
            .watch_later
            .iter()
            .find(|r| matches_id(&r.video))
        {
            return Some(record.video.clone());
        }
        if let Some(record) = self
            .library
            .data
            .favorites
            .iter()
            .find(|r| matches_id(&r.video))
        {
            return Some(record.video.clone());
        }
        for col in &self.library.data.collections {
            if let Some(record) = col.videos.iter().find(|r| matches_id(&r.video)) {
                return Some(record.video.clone());
            }
        }
        let url = history::youtube_watch_url(target_id)?;
        let history_entry = self.history.data.get(target_id);
        Some(Video {
            title: history_entry
                .map(|e| e.title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| mark.title.clone()),
            channel: history_entry.map(|e| e.channel.clone()).unwrap_or_default(),
            duration: String::new(),
            url,
            thumbnail: format!("https://i.ytimg.com/vi/{target_id}/hqdefault.jpg"),
        })
    }

    pub(crate) fn open_library_bookmark(&mut self, mark: &library::Bookmark) -> Option<Video> {
        let currently_playing = self
            .selected
            .as_ref()
            .and_then(|v| history::youtube_video_id(&v.url))
            .as_deref()
            == Some(mark.video_id.as_str());
        if currently_playing {
            self.seek_to_seconds(mark.time_ms as f64 / 1000.0);
            self.library.notice = Some(format!(
                "Avançado para {} em \"{}\".",
                format_time(mark.time_ms as f64 / 1000.0),
                mark.title
            ));
            return None;
        }
        let video = self.resolve_video_for_bookmark(mark)?;
        self.library.pending_bookmark_seek = Some((mark.video_id.clone(), mark.time_ms));
        Some(video)
    }

    pub(crate) fn browse_library_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        self.ensure_watch_later_merged_cache();
        self.ensure_liked_videos_merged_cache();
        let mut chosen = None;
        let mut bookmark_to_open = None;
        let trimmed_lib_q = self.library.search_query.trim().to_owned();
        let trimmed_bm_q = self.library.bookmark_filter.trim().to_owned();
        let has_lib_filter = !trimmed_lib_q.is_empty();

        let watch_later_total = self
            .watch_later_display_total()
            .max(self.library.data.watch_later.len());
        let favorites_total = self
            .liked_videos_display_total()
            .max(self.library.data.favorites.len());
        let collections_total =
            self.library.data.collections.len() + self.library.remote_playlists.items.len();
        let bookmarks_total = self.library.data.bookmarks.len();

        let overview_cache_id = egui::Id::new("fasttube_library_overview_cache");
        let overview_key = {
            let hist_entries = self.history.data.entries();
            LibraryOverviewCacheKey {
                library_revision: self.library.revision,
                bucket_revisions: (
                    self.library.data.favorites_revision(),
                    self.library.data.watch_later_revision(),
                    self.library.data.collections_revision(),
                    self.library.data.bookmarks_revision(),
                ),
                counts: (
                    favorites_total,
                    watch_later_total,
                    collections_total,
                    bookmarks_total,
                ),
                remote_revisions: (
                    self.library.watch_later_remote.revision,
                    self.library.liked_videos.revision,
                    self.library.remote_playlists.items.len(),
                    self.history
                        .remote
                        .feeds
                        .get(&history::HistoryFilter::All)
                        .map_or(0, |f| f.page_count),
                ),
                history_len: hist_entries.len(),
                history_first_sig: hist_entries.first().map(|e| {
                    (
                        e.video_id.clone(),
                        e.position_ms,
                        e.duration_ms,
                        e.watched_at_ms,
                    )
                }),
                lib_query: trimmed_lib_q.clone(),
                bm_query: trimmed_bm_q.clone(),
            }
        };
        let cached_overview = ctx
            .data(|d| d.get_temp::<Arc<CachedLibraryOverview>>(overview_cache_id))
            .filter(|c| c.key == overview_key)
            .unwrap_or_else(|| {
                let lib_q = trimmed_lib_q.to_lowercase();
                let bm_q = trimmed_bm_q.to_lowercase();
                let video_matches = |v: &Video| -> bool {
                    lib_q.is_empty()
                        || v.title.to_lowercase().contains(&lib_q)
                        || v.channel.to_lowercase().contains(&lib_q)
                };
                let watch_later: Vec<Video> = self
                    .library
                    .watch_later_remote
                    .merged_videos
                    .iter()
                    .filter(|video| video_matches(video))
                    .cloned()
                    .collect();
                let favorites: Vec<Video> = self
                    .library
                    .liked_videos
                    .merged_videos
                    .iter()
                    .filter(|video| video_matches(video))
                    .cloned()
                    .collect();
                let collections: Vec<(usize, String, Vec<Video>, usize)> = self
                    .library
                    .data
                    .collections
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, collection)| {
                        let name_matches =
                            lib_q.is_empty() || collection.name.to_lowercase().contains(&lib_q);
                        let filtered_videos: Vec<Video> = if name_matches {
                            collection.videos.iter().map(|r| r.video.clone()).collect()
                        } else {
                            collection
                                .videos
                                .iter()
                                .filter(|r| video_matches(&r.video))
                                .map(|r| r.video.clone())
                                .collect()
                        };
                        if name_matches || !filtered_videos.is_empty() {
                            Some((
                                idx,
                                collection.name.clone(),
                                filtered_videos,
                                collection.videos.len(),
                            ))
                        } else {
                            None
                        }
                    })
                    .collect();
                let bookmarks: Vec<(usize, library::Bookmark)> = self
                    .library
                    .data
                    .bookmarks
                    .iter()
                    .enumerate()
                    .filter(|(_, mark)| {
                        let matches_lib = lib_q.is_empty()
                            || mark.title.to_lowercase().contains(&lib_q)
                            || mark.note.to_lowercase().contains(&lib_q);
                        let matches_bm = bm_q.is_empty()
                            || mark.title.to_lowercase().contains(&bm_q)
                            || mark.note.to_lowercase().contains(&bm_q);
                        matches_lib && matches_bm
                    })
                    .map(|(idx, mark)| (idx, mark.clone()))
                    .collect();
                let mut seen_hist = HashSet::new();
                let mut recent_history: Vec<HistoryEntry> = self
                    .history
                    .data
                    .entries()
                    .iter()
                    .filter(|entry| {
                        (lib_q.is_empty()
                            || entry.title.to_lowercase().contains(&lib_q)
                            || entry.channel.to_lowercase().contains(&lib_q))
                            && seen_hist.insert(entry.video_id.clone())
                    })
                    .take(12)
                    .cloned()
                    .collect();
                if recent_history.len() < 12
                    && let Some(feed) = self.history.remote.feeds.get(&history::HistoryFilter::All)
                {
                    for sec in &feed.sections {
                        for item in &sec.items {
                            if recent_history.len() >= 12 {
                                break;
                            }
                            if (lib_q.is_empty()
                                || item.title.to_lowercase().contains(&lib_q)
                                || item.channel.to_lowercase().contains(&lib_q))
                                && seen_hist.insert(item.video_id.clone())
                            {
                                recent_history
                                    .push(item.to_history_entry(None, history::HistoryFilter::All));
                            }
                        }
                    }
                }
                let built = Arc::new(CachedLibraryOverview {
                    key: overview_key,
                    watch_later,
                    favorites,
                    collections,
                    bookmarks,
                    recent_history,
                });
                ctx.data_mut(|d| d.insert_temp(overview_cache_id, Arc::clone(&built)));
                built
            });
        let watch_later = &cached_overview.watch_later;
        let favorites = &cached_overview.favorites;
        let collections = &cached_overview.collections;
        let bookmarks = &cached_overview.bookmarks;
        let recent_history = &cached_overview.recent_history;

        egui::ScrollArea::vertical()
            .id_salt("browse_library_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.library_header_ui(
                    ui,
                    ctx,
                    watch_later_total,
                    favorites_total,
                    collections_total,
                    bookmarks_total,
                    palette,
                );

                if !recent_history.is_empty() {
                    let mut open_full_history = false;
                    let hist_shelf_id = egui::Id::new("fasttube_lib_hist_shelf_offset");
                    let hist_page_size = 3usize;
                    let raw_hist_off =
                        ctx.data(|d| d.get_temp::<usize>(hist_shelf_id).unwrap_or(0));
                    let (mut hist_offset, _) =
                        clamp_shelf_offset(raw_hist_off, recent_history.len(), hist_page_size);
                    let mut next_hist_off = hist_offset;
                    let total_hist_count =
                        self.history.data.entries().len().max(recent_history.len());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if pt { "Histórico" } else { "History" })
                                .size(20.0)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("({total_hist_count})"))
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if styled_pill_button(
                                ui,
                                if pt { "Ver tudo" } else { "View all" },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                open_full_history = true;
                            }
                            let can_next = hist_offset + hist_page_size < recent_history.len();
                            let can_prev = hist_offset > 0;
                            if styled_icon_chip_button(ui, can_next, "›", &palette).clicked() {
                                next_hist_off = clamp_shelf_offset(
                                    hist_offset + hist_page_size,
                                    recent_history.len(),
                                    hist_page_size,
                                )
                                .0;
                            }
                            if styled_icon_chip_button(ui, can_prev, "‹", &palette).clicked() {
                                next_hist_off = clamp_shelf_offset(
                                    hist_offset.saturating_sub(hist_page_size),
                                    recent_history.len(),
                                    hist_page_size,
                                )
                                .0;
                            }
                        });
                    });
                    if next_hist_off != raw_hist_off {
                        hist_offset = next_hist_off;
                        ctx.data_mut(|d| d.insert_temp(hist_shelf_id, hist_offset));
                    }
                    ui.add_space(4.0);
                    paint_section_hairline(ui, &palette);
                    ui.add_space(6.0);
                    let current_video_id = self
                        .selected
                        .as_ref()
                        .and_then(|video| history::youtube_video_id(&video.url));
                    let mut history_to_remove: Option<String> = None;
                    let mut history_to_enqueue: Option<Video> = None;
                    let hist_end = (hist_offset + hist_page_size).min(recent_history.len());
                    self.thumbnails.begin_pass();
                    for entry in &recent_history[hist_offset..hist_end] {
                        let is_playing =
                            current_video_id.as_deref() == Some(entry.video_id.as_str());
                        ui.push_id(("lib_recent_hist", &entry.video_id), |ui| {
                            let (play_clicked, remove_clicked, enqueue_clicked) =
                                self.history_entry_card_ui(ui, entry, true, is_playing);
                            if play_clicked && chosen.is_none() {
                                chosen = self.video_for_history_entry(entry);
                            }
                            if enqueue_clicked {
                                history_to_enqueue = self.video_for_history_entry(entry);
                            }
                            if remove_clicked {
                                history_to_remove = Some(entry.video_id.clone());
                            }
                        });
                        ui.add_space(6.0);
                    }
                    self.thumbnails.end_pass(ctx);
                    if let Some(video) = history_to_enqueue {
                        let _ = self.enqueue_video(video);
                    }
                    if let Some(video_id) = history_to_remove {
                        self.remove_history_entry_with_remote(&video_id, Some(ctx));
                    }
                    if open_full_history {
                        self.on_enter_history_section(ctx);
                        self.ui.browse_section = BrowseSection::History;
                    }
                    ui.add_space(16.0);
                }

                let wl_empty = if has_lib_filter && watch_later_total > 0 {
                    if pt {
                        "Nenhum item em Assistir mais tarde corresponde ao filtro."
                    } else {
                        "No Watch later items match the filter."
                    }
                } else if pt {
                    "Os vídeos salvos para assistir mais tarde aparecerão aqui."
                } else {
                    "Videos saved to Watch later will appear here."
                };
                let (wl_chosen, wl_remove) = self.library_video_section_ui(
                    ui,
                    ctx,
                    if pt {
                        "Assistir mais tarde"
                    } else {
                        "Watch later"
                    },
                    wl_empty,
                    watch_later,
                    "library_watch_later",
                );
                if let Some(video) = wl_chosen
                    && chosen.is_none()
                {
                    chosen = Some(video);
                }
                if let Some(url) = wl_remove {
                    let removed_local = self.library.data.remove_from_watch_later(&url);
                    if removed_local {
                        self.save_library();
                    }
                    if let Some(remote_item) = self
                        .library
                        .watch_later_remote
                        .items
                        .iter()
                        .find(|it| it.result.video.url == url)
                        .cloned()
                        && let Some(vid_id) = history::youtube_video_id(&url)
                    {
                        self.dispatch_playlist_remove_video(
                            "WL".into(),
                            vid_id,
                            remote_item.set_video_id,
                            ctx,
                        );
                    }
                    if removed_local {
                        self.library.notice = Some(
                            if pt {
                                "Vídeo removido de Assistir mais tarde."
                            } else {
                                "Video removed from Watch later."
                            }
                            .to_owned(),
                        );
                    }
                }

                ui.add_space(18.0);
                if let Some(video) = self.library_collections_section_ui(
                    ui,
                    ctx,
                    collections,
                    collections_total,
                    has_lib_filter,
                    palette,
                ) && chosen.is_none()
                {
                    chosen = Some(video);
                }

                if favorites_total > 0 {
                    ui.add_space(16.0);
                    let fav_empty = if has_lib_filter {
                        if pt {
                            "Nenhum item em Favoritos corresponde ao filtro."
                        } else {
                            "No Favorites match the filter."
                        }
                    } else if pt {
                        "Nenhum vídeo favoritado."
                    } else {
                        "No favorited videos."
                    };
                    let (fav_chosen, fav_remove) = self.library_video_section_ui(
                        ui,
                        ctx,
                        if pt { "Favoritos" } else { "Favorites" },
                        fav_empty,
                        favorites,
                        "library_favorites",
                    );
                    if let Some(video) = fav_chosen
                        && chosen.is_none()
                    {
                        chosen = Some(video);
                    }
                    if let Some(url) = fav_remove
                        && self.library.data.remove_from_favorites(&url)
                    {
                        self.library.notice = Some(
                            if pt {
                                "Vídeo removido dos Favoritos."
                            } else {
                                "Video removed from Favorites."
                            }
                            .to_owned(),
                        );
                        self.save_library();
                    }
                }

                if bookmarks_total > 0 {
                    ui.add_space(12.0);
                    bookmark_to_open = self.library_bookmarks_section_ui(
                        ui,
                        ctx,
                        bookmarks,
                        bookmarks_total,
                        palette,
                    );
                }
            });

        if let Some(mark) = bookmark_to_open
            && let Some(video) = self.open_library_bookmark(&mark)
            && chosen.is_none()
        {
            chosen = Some(video);
        }
        chosen
    }
}
