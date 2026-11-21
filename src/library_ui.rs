use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};

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

fn paint_library_card_hover_glow(
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

fn empty_state_card_ui(ui: &mut egui::Ui, title: &str, hint: &str, palette: &ThemePalette) {
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
    toggle_pause_clicked: bool,
    manage_clicked: bool,
    external_url: Option<&'static str>,
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
    pub(crate) fn queue_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        max_height: Option<f32>,
    ) {
        let queued: Vec<(u64, Video)> = self
            .queue
            .data
            .videos()
            .iter()
            .enumerate()
            .filter_map(|(idx, video)| {
                self.queue
                    .data
                    .entry_id(idx)
                    .map(|entry_id| (entry_id, video.clone()))
            })
            .collect();
        let len = self.queue.data.len();
        let limit = self.queue.data.limit();
        let duration_summary = self.queue.data.format_duration_summary();
        let palette = self.palette();
        let actions = Self::queue_heading_ui(
            ui,
            compact,
            len,
            limit,
            &duration_summary,
            !queued.is_empty(),
            self.queue.repeat_mode,
            self.ui_pt(),
            &palette,
        );
        if let Some(error) = &self.queue.persistence_error {
            ui.colored_label(Color32::RED, error);
        }
        if let Some(error) = &self.queue.error {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(error).small().color(palette.error_banner_fg));
                });
        }
        if let Some(notice) = &self.queue.notice {
            ui.add_space(4.0);
            egui::Frame::new()
                .fill(palette.notice_bg)
                .stroke(egui::Stroke::new(1.0, palette.notice_stroke))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(notice).small().color(palette.notice_fg));
                });
        }
        ui.add_space(6.0);
        if queued.is_empty() {
            empty_state_card_ui(
                ui,
                self.pick("Fila de reprodução vazia", "Playback queue is empty"),
                self.pick(
                    "Fila vazia. Adicione resultados com '+ Fila' e avance manualmente com '⏭ Próximo'.",
                    "Queue is empty. Add videos with '+ Queue' and advance manually with '⏭ Next'.",
                ),
                &palette,
            );
            if actions.cycle_repeat {
                self.cycle_repeat_mode();
            }
            if actions.advance {
                self.request_queue_advance();
            }
            return;
        }
        let mut save_as_collection_clicked = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            let input_w = if compact { 140.0 } else { 210.0 };
            let col_hint = self.pick("Nome da coleção (opcional)", "Collection name (optional)");
            ui.add_sized(
                [input_w, 26.0],
                egui::TextEdit::singleline(&mut self.queue.save_collection_name)
                    .hint_text(col_hint),
            );
            if styled_compact_button(
                ui,
                self.pick("Salvar fila como coleção", "Save queue as collection"),
                false,
                &palette,
            )
            .on_hover_text(self.pick(
                "Criar uma coleção na Biblioteca com os vídeos desta fila",
                "Create a Library collection from the videos in this queue",
            ))
            .clicked()
            {
                save_as_collection_clicked = true;
            }
        });
        ui.add_space(6.0);

        let mut play_now_action: Option<u64> = None;
        let mut move_top_action: Option<usize> = None;
        let mut reorder_action: Option<(usize, usize)> = None;
        let mut remove_action: Option<usize> = None;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, true]);
        if let Some(limit_h) = max_height {
            scroll = scroll.max_height(limit_h);
        }
        let row_height = 82.0;
        let inner_height = row_height - 16.0;
        let ctx = ui.ctx().clone();
        self.thumbnails.begin_pass();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            scroll.show_rows(ui, row_height, queued.len(), |ui, range| {
                let target_width = max_card_width
                    .map(|limit_w| ui.available_width().min(limit_w))
                    .unwrap_or_else(|| ui.available_width());
                for index in range {
                    let (entry_id, item) = &queued[index];
                    let thumb_url: std::borrow::Cow<'_, str> = if !item.thumbnail.trim().is_empty()
                    {
                        std::borrow::Cow::Borrowed(item.thumbnail.as_str())
                    } else if let Some(vid) = history::youtube_video_id(&item.url) {
                        std::borrow::Cow::Owned(format!(
                            "https://i.ytimg.com/vi/{vid}/hqdefault.jpg"
                        ))
                    } else {
                        std::borrow::Cow::Borrowed("")
                    };
                    ui.push_id((entry_id, index), |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(target_width, row_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_height(row_height);
                                ui.set_max_height(row_height);
                                let is_next = index == 0;
                                let fill = if is_next {
                                    palette.queue_next_bg
                                } else {
                                    palette.card_alt_bg
                                };
                                let frame_resp = egui::Frame::new()
                                    .fill(fill)
                                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
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
                                                thumb_url.as_ref(),
                                                egui::vec2(100.0, 56.0),
                                                8.0,
                                            );
                                            if thumb_resp
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text(&item.title)
                                                .clicked()
                                            {
                                                play_now_action = Some(*entry_id);
                                            }
                                            ui.add_space(8.0);
                                            ui.vertical(|ui| {
                                                ui.set_width(ui.available_width().max(80.0));
                                                ui.horizontal(|ui| {
                                                    let badge = if is_next {
                                                        format!(
                                                            "{} #{}",
                                                            self.pick("Próximo", "Next"),
                                                            index + 1
                                                        )
                                                    } else {
                                                        format!("#{}", index + 1)
                                                    };
                                                    ui.label(RichText::new(badge).small().strong());
                                                    let title_w =
                                                        (ui.available_width() - 104.0).max(60.0);
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(title_w, 24.0),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if ui
                                                                .add(
                                                                    egui::Button::new(
                                                                        RichText::new(&item.title)
                                                                            .small()
                                                                            .strong(),
                                                                    )
                                                                    .frame(false)
                                                                    .truncate(),
                                                                )
                                                                .on_hover_cursor(
                                                                    egui::CursorIcon::PointingHand,
                                                                )
                                                                .on_hover_text(&item.title)
                                                                .clicked()
                                                            {
                                                                play_now_action = Some(*entry_id);
                                                            }
                                                        },
                                                    );
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if styled_icon_chip_button(
                                                                ui, true, "✕", &palette,
                                                            )
                                                            .on_hover_text(self.pick(
                                                                "Remover da fila",
                                                                "Remove from queue",
                                                            ))
                                                            .clicked()
                                                            {
                                                                remove_action = Some(index);
                                                            }
                                                            if styled_icon_chip_button(
                                                                ui,
                                                                index + 1 < queued.len(),
                                                                "↓",
                                                                &palette,
                                                            )
                                                            .on_hover_text(self.pick(
                                                                "Mover para baixo na fila",
                                                                "Move down in queue",
                                                            ))
                                                            .clicked()
                                                            {
                                                                reorder_action =
                                                                    Some((index, index + 1));
                                                            }
                                                            if styled_icon_chip_button(
                                                                ui,
                                                                index > 0,
                                                                "↑",
                                                                &palette,
                                                            )
                                                            .on_hover_text(self.pick(
                                                                "Mover para cima na fila",
                                                                "Move up in queue",
                                                            ))
                                                            .clicked()
                                                            {
                                                                reorder_action =
                                                                    Some((index, index - 1));
                                                            }
                                                        },
                                                    );
                                                });
                                                ui.add_space(4.0);
                                                ui.horizontal(|ui| {
                                                    let meta_w = if index > 0 {
                                                        (ui.available_width() - 128.0).max(60.0)
                                                    } else {
                                                        ui.available_width().max(60.0)
                                                    };
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(meta_w, 26.0),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if !item.channel.is_empty()
                                                                || !item.duration.is_empty()
                                                            {
                                                                let sub: std::borrow::Cow<'_, str> =
                                                                    if item.channel.is_empty() {
                                                                        std::borrow::Cow::Borrowed(
                                                                            item.duration.as_str(),
                                                                        )
                                                                    } else if item
                                                                        .duration
                                                                        .is_empty()
                                                                    {
                                                                        std::borrow::Cow::Borrowed(
                                                                            item.channel.as_str(),
                                                                        )
                                                                    } else {
                                                                        std::borrow::Cow::Owned(
                                                                            format!(
                                                                                "{} • {}",
                                                                                item.channel,
                                                                                item.duration
                                                                            ),
                                                                        )
                                                                    };
                                                                ui.add(
                                                                    egui::Label::new(
                                                                        RichText::new(sub.as_ref())
                                                                            .small()
                                                                            .weak(),
                                                                    )
                                                                    .truncate(),
                                                                );
                                                            }
                                                        },
                                                    );
                                                    if index > 0 {
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                if styled_compact_button(
                                                                    ui,
                                                                    self.pick(
                                                                        "Tocar em seguida",
                                                                        "Play next",
                                                                    ),
                                                                    false,
                                                                    &palette,
                                                                )
                                                                .on_hover_text(self.pick(
                                                                    "Mover para o topo da fila",
                                                                    "Move to the top of the queue",
                                                                ))
                                                                .clicked()
                                                                {
                                                                    move_top_action = Some(index);
                                                                }
                                                            },
                                                        );
                                                    }
                                                });
                                            });
                                        });
                                    });
                                paint_library_card_hover_glow(
                                    ui,
                                    frame_resp.response.rect,
                                    10.0,
                                    ui.make_persistent_id((
                                        "queue_card_hover",
                                        scroll_id,
                                        *entry_id,
                                    )),
                                    &palette,
                                );
                            },
                        );
                    });
                }
            });
        });
        self.thumbnails.end_pass(&ctx);
        if save_as_collection_clicked {
            let name = self.queue.save_collection_name.clone();
            let _ = self.save_queue_as_collection(&name);
        }
        if let Some(entry_id) = play_now_action {
            self.request_play_queue_entry(entry_id);
        }
        if let Some(index) = move_top_action {
            self.move_queue_item_to_top(index);
        }
        if let Some((from, to)) = reorder_action {
            self.reorder_queue(from, to);
        }
        if let Some(index) = remove_action {
            self.remove_from_queue(index);
        }
        if actions.cycle_repeat {
            self.cycle_repeat_mode();
        }
        if actions.shuffle {
            self.shuffle_queue();
        }
        if actions.deduplicate {
            let _ = self.deduplicate_queue();
        }
        if actions.clear {
            self.clear_queue();
        }
        if actions.advance {
            self.request_queue_advance();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn queue_heading_ui(
        ui: &mut egui::Ui,
        _compact: bool,
        len: usize,
        limit: usize,
        duration_summary: &str,
        has_queued: bool,
        repeat_mode: queue::RepeatMode,
        pt: bool,
        palette: &ThemePalette,
    ) -> QueueHeadingActions {
        let mut actions = QueueHeadingActions::default();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if pt {
                    "Fila de reprodução"
                } else {
                    "Playback queue"
                })
                .size(20.0)
                .strong(),
            );
            if has_queued {
                ui.label(
                    RichText::new(format!("{duration_summary} ({len}/{limit})"))
                        .size(12.5)
                        .color(palette.neutral_status_fg),
                );
            } else {
                ui.label(
                    RichText::new(format!("({len}/{limit})"))
                        .size(12.5)
                        .color(palette.neutral_status_fg),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if styled_compact_button_enabled(
                    ui,
                    has_queued,
                    if pt { "⏭ Próximo" } else { "⏭ Next" },
                    false,
                    palette,
                )
                .on_hover_text(if pt {
                    "Avançar manualmente para o próximo vídeo da fila"
                } else {
                    "Advance manually to the next video in the queue"
                })
                .clicked()
                {
                    actions.advance = true;
                }
                let repeat_label = if pt {
                    repeat_mode.label_pt()
                } else {
                    repeat_mode.label_en()
                };
                if styled_compact_button(
                    ui,
                    repeat_label,
                    !matches!(repeat_mode, queue::RepeatMode::Off),
                    palette,
                )
                .on_hover_text(if pt {
                    "Alternar repetição: Desligado -> Fila toda -> Vídeo atual"
                } else {
                    "Cycle repeat mode: Off -> All -> One"
                })
                .clicked()
                {
                    actions.cycle_repeat = true;
                }
                if has_queued {
                    if styled_danger_button(
                        ui,
                        if pt { "Limpar fila" } else { "Clear queue" },
                        palette,
                    )
                    .on_hover_text(if pt {
                        "Remover todos os itens da fila"
                    } else {
                        "Remove all items from the queue"
                    })
                    .clicked()
                    {
                        actions.clear = true;
                    }
                    if styled_compact_button_enabled(
                        ui,
                        len > 1,
                        if pt {
                            "Remover duplicados"
                        } else {
                            "Deduplicate"
                        },
                        false,
                        palette,
                    )
                    .on_hover_text(if pt {
                        "Remover ocorrências duplicadas mantendo a primeira"
                    } else {
                        "Remove duplicate entries keeping the first occurrence"
                    })
                    .clicked()
                    {
                        actions.deduplicate = true;
                    }
                    if styled_compact_button_enabled(
                        ui,
                        len > 1,
                        if pt { "Embaralhar" } else { "Shuffle" },
                        false,
                        palette,
                    )
                    .on_hover_text(if pt {
                        "Embaralhar a ordem atual da fila"
                    } else {
                        "Shuffle the current queue order"
                    })
                    .clicked()
                    {
                        actions.shuffle = true;
                    }
                }
            });
        });
        ui.add_space(4.0);
        paint_section_hairline(ui, palette);
        actions
    }

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
        let thumb_url: std::borrow::Cow<'_, str> = known_video
            .as_ref()
            .map(|v| v.thumbnail.trim())
            .filter(|t| !t.is_empty())
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
        let progress_ratio = entry.duration_ms.filter(|&d| d > 0).map(|d| {
            if history::is_near_end(entry.position_ms, Some(d)) || entry.position_ms == 0 {
                1.0_f32
            } else {
                (entry.position_ms as f32 / d as f32).clamp(0.05, 1.0)
            }
        });
        let in_watch_later = self.library.data.watch_later.iter().any(|v| {
            history::youtube_video_id(&v.video.url).as_deref() == Some(entry.video_id.as_str())
                || canonical_url.as_deref() == Some(v.video.url.as_str())
        });
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
                        let title_w = (details_w - 32.0).max(80.0);
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
                                    &palette,
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
                                if search_menu_item_ui(
                                    ui,
                                    SearchMenuIcon::Clock,
                                    wl_label,
                                    &palette,
                                )
                                .clicked()
                                {
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
                                    &palette,
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
                                            let lbl =
                                                if pt { fmt.label_pt() } else { fmt.label_en() };
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
                                    &palette,
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
                                    &palette,
                                )
                                .clicked()
                                {
                                    action.remove_clicked = true;
                                    ui.close();
                                }
                            });
                        });
                    });

                    let views_label = metadata.as_ref().and_then(|meta| {
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
                    });
                    let has_channel = !entry.channel.trim().is_empty();
                    if has_channel || views_label.is_some() {
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
                                let is_verified = metadata.as_ref().is_some_and(|m| {
                                    m.channel_verified || m.view_count.is_some_and(|v| v >= 100_000)
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
                        });
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
        });

        outcome
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

        let total_avail = max_card_width
            .map(|limit| ui.available_width().min(limit))
            .unwrap_or_else(|| ui.available_width())
            .max(260.0);
        let two_column = total_avail >= 760.0;
        let (left_inset, feed_col_w, gap, rail_w) = if two_column {
            let left_inset = if total_avail > 1180.0 {
                ((total_avail - 1180.0) * 0.25).clamp(16.0, 88.0)
            } else if total_avail >= 960.0 {
                16.0
            } else {
                0.0
            };
            let rail_w = if total_avail >= 960.0 {
                310.0
            } else {
                (total_avail * 0.34).clamp(240.0, 310.0)
            };
            let remaining_w = (total_avail - left_inset).max(1.0);
            let feed_col_w = (remaining_w - rail_w - 48.0).clamp(380.0, 760.0);
            let gap = (remaining_w - feed_col_w - rail_w).clamp(24.0, 280.0);
            (left_inset, feed_col_w, gap, rail_w)
        } else {
            (0.0, total_avail, 0.0, total_avail)
        };

        let entries: Vec<HistoryEntry> = self.history.data.entries().to_vec();
        let filtered: Vec<&HistoryEntry> =
            history::filter_entries(&entries, &self.history.search_query, self.history.filter);
        let now_ms = history::current_unix_epoch_ms();
        let offset_minutes = history::local_utc_offset_minutes();
        let groups = [
            history::DateGroup::Today,
            history::DateGroup::Yesterday,
            history::DateGroup::ThisWeek,
            history::DateGroup::Older,
        ];
        let mut buckets: [Vec<&HistoryEntry>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for entry in &filtered {
            let slot = match history::date_group_at(entry.watched_at_ms, now_ms, offset_minutes) {
                history::DateGroup::Today => 0,
                history::DateGroup::Yesterday => 1,
                history::DateGroup::ThisWeek => 2,
                history::DateGroup::Older => 3,
            };
            buckets[slot].push(entry);
        }

        let mut chosen: Option<Video> = None;
        let mut to_remove: Option<String> = None;
        let mut to_enqueue: Option<Video> = None;
        let mut toggle_watch_later: Option<Video> = None;
        let mut open_save_modal: Option<Video> = None;
        let mut open_share_modal: Option<Video> = None;
        let mut download_req: Option<(Video, downloads::DownloadFormat)> = None;
        let mut open_channel: Option<ChannelTarget> = None;
        let mut clear_filter_clicked = false;
        let mut rail_outcome = HistoryRailOutcome::default();

        let mut render_feed_column = |app: &mut Self, ui: &mut egui::Ui| {
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
                    let selected = app.history.filter == filter;
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
                        app.history.filter = filter;
                    }
                }
            });
            ui.add_space(20.0);

            if entry_count == 0 {
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

            if filtered.is_empty() {
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
            let mut scroll = egui::ScrollArea::vertical()
                .id_salt(scroll_id)
                .auto_shrink([false, false]);
            if let Some(limit) = max_height {
                scroll = scroll.max_height(limit);
            }

            app.thumbnails.begin_pass();
            scroll.show(ui, |ui| {
                ui.set_width(feed_col_w);
                let visible_rect = ui
                    .clip_rect()
                    .expand2(egui::vec2(0.0, thumbnails::VIEWPORT_OVERSCAN_PX + 180.0));
                let mut rendered_group = false;
                for (slot_idx, bucket) in buckets.iter().enumerate() {
                    if bucket.is_empty() {
                        continue;
                    }
                    if rendered_group {
                        ui.add_space(22.0);
                    }
                    rendered_group = true;
                    ui.label(
                        RichText::new(groups[slot_idx].label(pt))
                            .size(20.0)
                            .strong()
                            .color(palette.header_button_fg),
                    );
                    ui.add_space(14.0);
                    for (item_idx, entry) in bucket.iter().enumerate() {
                        let cursor_top = ui.cursor().top();
                        if cursor_top + thumb_h < visible_rect.top()
                            || cursor_top > visible_rect.bottom()
                        {
                            ui.allocate_exact_size(
                                egui::vec2(feed_col_w, thumb_h),
                                egui::Sense::hover(),
                            );
                        } else {
                            let is_playing =
                                current_video_id.as_deref() == Some(entry.video_id.as_str());
                            let (row_action, known_video) = ui
                                .push_id(("yt_hist_row", &entry.video_id), |ui| {
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
                        }
                        if item_idx + 1 < bucket.len() {
                            ui.add_space(16.0);
                        }
                    }
                }
                ui.add_space(24.0);
            });
            app.thumbnails.end_pass(&ctx);
        };

        if two_column {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if left_inset > 0.0 {
                    ui.add_space(left_inset);
                }
                ui.vertical(|ui| render_feed_column(self, ui));
                ui.add_space(gap);
                ui.vertical(|ui| {
                    ui.add_space(56.0);
                    rail_outcome =
                        self.history_management_rail_ui(ui, scroll_id, rail_w, pt, &palette);
                });
            });
        } else {
            ui.vertical(|ui| {
                ui.add_space(8.0);
                rail_outcome = self.history_management_rail_ui(ui, scroll_id, rail_w, pt, &palette);
                ui.add_space(10.0);
                render_feed_column(self, ui);
            });
        }

        if clear_filter_clicked {
            self.history.search_query.clear();
            self.history.filter = history::HistoryFilter::All;
        }
        if rail_outcome.toggle_pause_clicked {
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
        if rail_outcome.clear_clicked {
            self.clear_history();
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
                && self.auth.remote_playlists_rx.is_none()
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
            self.remove_history_entry(&video_id);
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

        enum HistoryDisplayRow<'a> {
            Header(history::DateGroup),
            Item(&'a HistoryEntry),
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

        let entries: Vec<HistoryEntry> = self.history.data.entries().to_vec();
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

        let filtered =
            history::filter_entries(&entries, &self.history.search_query, self.history.filter);
        if filtered.is_empty() {
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

        let now_ms = history::current_unix_epoch_ms();
        let offset_minutes = history::local_utc_offset_minutes();
        let groups = [
            history::DateGroup::Today,
            history::DateGroup::Yesterday,
            history::DateGroup::ThisWeek,
            history::DateGroup::Older,
        ];
        let mut buckets: [Vec<&HistoryEntry>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
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

        let mut rows: Vec<HistoryDisplayRow<'_>> = Vec::with_capacity(filtered_len + 4);
        for (slot_idx, bucket) in buckets.iter().enumerate() {
            if bucket.is_empty() {
                continue;
            }
            rows.push(HistoryDisplayRow::Header(groups[slot_idx]));
            for entry in bucket {
                rows.push(HistoryDisplayRow::Item(entry));
            }
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
        let ctx = ui.ctx().clone();
        self.thumbnails.begin_pass();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            scroll.show_rows(ui, row_height, rows.len(), |ui, range| {
                let target_width = max_card_width
                    .map(|limit| ui.available_width().min(limit))
                    .unwrap_or_else(|| ui.available_width());
                for row_idx in range {
                    match &rows[row_idx] {
                        HistoryDisplayRow::Header(group) => {
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
                        HistoryDisplayRow::Item(entry) => {
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
            }
        });
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

        self.thumbnails.begin_pass();
        let avail = ui.available_width().max(260.0);
        let cols: usize = if avail >= 740.0 { 2 } else { 1 };
        let col_gap = 12.0;
        let card_w = ((avail - col_gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(240.0);
        let total_rows = videos.len().div_ceil(cols);
        let row_height = if cols == 1 { 156.0 } else { 148.0 };
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            for row_idx in 0..total_rows {
                let slice_start = row_idx * cols;
                let slice_end = (slice_start + cols).min(videos.len());
                ui.allocate_ui_with_layout(
                    egui::vec2(avail, row_height),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
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
                        RichText::new(if pt { "Você" } else { "You" })
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
        if open_history {
            self.ui.browse_section = BrowseSection::History;
        } else if open_downloads {
            self.ui.browse_section = BrowseSection::Downloads;
        } else if open_playlists {
            self.library.selected_collection_idx = None;
            self.ui.browse_section = BrowseSection::Playlists;
            if self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
                && self.auth.remote_playlists_rx.is_none()
            {
                self.start_fetch_remote_playlists(ctx);
            }
        } else if open_watch_later {
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = BrowseSection::WatchLater;
        } else if open_liked {
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = BrowseSection::LikedVideos;
            if self.auth.authenticated {
                self.start_liked_videos_fetch(false, ctx);
            }
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
        let visible_marks: Vec<library::Bookmark> =
            bookmarks.iter().map(|(_, mark)| mark.clone()).collect();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if pt { "Marcadores" } else { "Bookmarks" })
                    .size(20.0)
                    .strong(),
            );
            ui.label(
                RichText::new(format!("({}/{})", visible_marks.len(), total_bookmarks))
                    .size(12.5)
                    .color(palette.neutral_status_fg),
            );
            if styled_compact_button_enabled(
                ui,
                !visible_marks.is_empty(),
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
            for (orig_idx, mark) in bookmarks {
                let is_editing = self.library.editing_bookmark_idx == Some(*orig_idx);
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
        let mut chosen = None;
        let mut bookmark_to_open = None;
        let lib_q = self.library.search_query.trim().to_lowercase();
        let bm_q = self.library.bookmark_filter.trim().to_lowercase();
        let has_lib_filter = !lib_q.is_empty();
        let video_matches = |v: &Video| -> bool {
            lib_q.is_empty()
                || v.title.to_lowercase().contains(&lib_q)
                || v.channel.to_lowercase().contains(&lib_q)
        };

        let watch_later_total = self.library.data.watch_later.len();
        let favorites_total = self.library.data.favorites.len();
        let collections_total = self.library.data.collections.len();
        let bookmarks_total = self.library.data.bookmarks.len();

        let watch_later: Vec<Video> = self
            .library
            .data
            .watch_later
            .iter()
            .filter(|record| video_matches(&record.video))
            .map(|record| record.video.clone())
            .collect();
        let favorites: Vec<Video> = self
            .library
            .data
            .favorites
            .iter()
            .filter(|record| video_matches(&record.video))
            .map(|record| record.video.clone())
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

                let recent_history: Vec<HistoryEntry> = self
                    .history
                    .data
                    .entries()
                    .iter()
                    .filter(|entry| {
                        lib_q.is_empty()
                            || entry.title.to_lowercase().contains(&lib_q)
                            || entry.channel.to_lowercase().contains(&lib_q)
                    })
                    .take(3)
                    .cloned()
                    .collect();
                if !recent_history.is_empty() {
                    let mut open_full_history = false;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if pt { "Histórico" } else { "History" })
                                .size(20.0)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("({})", self.history.data.entries().len()))
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
                        });
                    });
                    ui.add_space(4.0);
                    paint_section_hairline(ui, &palette);
                    ui.add_space(6.0);
                    let current_video_id = self
                        .selected
                        .as_ref()
                        .and_then(|video| history::youtube_video_id(&video.url));
                    let mut history_to_remove: Option<String> = None;
                    let mut history_to_enqueue: Option<Video> = None;
                    self.thumbnails.begin_pass();
                    for entry in &recent_history {
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
                        self.remove_history_entry(&video_id);
                    }
                    if open_full_history {
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
                    &watch_later,
                    "library_watch_later",
                );
                if let Some(video) = wl_chosen
                    && chosen.is_none()
                {
                    chosen = Some(video);
                }
                if let Some(url) = wl_remove
                    && self.library.data.remove_from_watch_later(&url)
                {
                    self.library.notice = Some(
                        if pt {
                            "Vídeo removido de Assistir mais tarde."
                        } else {
                            "Video removed from Watch later."
                        }
                        .to_owned(),
                    );
                    self.save_library();
                }

                ui.add_space(18.0);
                if let Some(video) = self.library_collections_section_ui(
                    ui,
                    ctx,
                    &collections,
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
                        &favorites,
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
                        &bookmarks,
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

    pub(crate) fn downloads_section_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let palette = self.palette();
        let pt = self.ui_pt();
        let effective_dir = downloads::DownloadManager::effective_download_dir(
            self.settings.prefs.value.download_dir_override.as_deref(),
        );
        let avail_w = ui.available_width().max(320.0);
        let two_col = avail_w >= 860.0;

        ui.label(
            RichText::new("Downloads")
                .size(28.0)
                .strong()
                .color(palette.channel_fg),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(if pt {
                "Salvos localmente via yt-dlp · Sem necessidade de assinatura"
            } else {
                "Saved offline via yt-dlp · No subscription required"
            })
            .size(13.0)
            .color(palette.neutral_status_fg),
        );
        ui.add_space(10.0);

        let chips = if pt {
            [
                (0_u8, "Tudo"),
                (1, "Vídeos"),
                (2, "Áudio"),
                (3, "Em andamento"),
                (4, "Concluídos"),
            ]
        } else {
            [
                (0_u8, "All"),
                (1, "Videos"),
                (2, "Audio"),
                (3, "In progress"),
                (4, "Completed"),
            ]
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (tab, lbl) in chips {
                if styled_pill_button(ui, lbl, self.downloads.filter_tab == tab, &palette).clicked()
                {
                    self.downloads.filter_tab = tab;
                }
            }
        });
        ui.add_space(10.0);
        paint_section_hairline(ui, &palette);
        ui.add_space(12.0);

        if let Some(notice) = self.downloads.notice.clone() {
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
                self.downloads.notice = None;
            }
            ui.add_space(8.0);
        }

        if !two_col {
            self.downloads_direct_rail_ui(ui, ctx, &effective_dir, &palette);
            ui.add_space(14.0);
            self.downloads_list_column_ui(ui, ctx, &palette);
        } else {
            let right_w = 320.0;
            let left_w = (avail_w - right_w - 24.0).max(340.0);
            let col_h = ui.available_height().max(480.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(left_w);
                        self.downloads_list_column_ui(ui, ctx, &palette);
                    },
                );
                ui.add_space(24.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(right_w);
                        self.downloads_direct_rail_ui(ui, ctx, &effective_dir, &palette);
                    },
                );
            });
        }
    }

    fn downloads_direct_rail_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        effective_dir: &std::path::Path,
        palette: &ThemePalette,
    ) {
        let pt = self.ui_pt();
        let mut submit_download = false;

        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new(if pt {
                        "Baixar vídeo ou áudio"
                    } else {
                        "Download video or audio"
                    })
                    .size(15.5)
                    .strong()
                    .color(palette.channel_fg),
                );
                ui.add_space(8.0);
                let resp = ui.add_sized(
                    [ui.available_width(), 32.0],
                    egui::TextEdit::singleline(&mut self.downloads.direct_url_input).hint_text(
                        if pt {
                            "Cole uma URL ou ID do YouTube..."
                        } else {
                            "Paste YouTube URL or video ID..."
                        },
                    ),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit_download = true;
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for fmt in downloads::DownloadFormat::ALL {
                        let short_label = match fmt {
                            downloads::DownloadFormat::VideoMp4Best => "MP4",
                            downloads::DownloadFormat::Video1080p => "1080p",
                            downloads::DownloadFormat::Video720p => "720p",
                            downloads::DownloadFormat::Video480p => "480p",
                            downloads::DownloadFormat::AudioM4a => "M4A",
                            downloads::DownloadFormat::AudioOpus => "Opus",
                            downloads::DownloadFormat::AudioMp3 => "MP3",
                        };
                        if styled_pill_button(
                            ui,
                            short_label,
                            self.downloads.selected_format == fmt,
                            palette,
                        )
                        .clicked()
                        {
                            self.downloads.selected_format = fmt;
                        }
                    }
                });
                ui.add_space(10.0);
                if styled_pill_button(
                    ui,
                    if pt { "⬇ Baixar" } else { "⬇ Download" },
                    true,
                    palette,
                )
                .clicked()
                {
                    submit_download = true;
                }
                ui.add_space(10.0);
                paint_section_hairline(ui, palette);
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        if pt {
                            "Pasta de destino"
                        } else {
                            "Destination folder"
                        },
                        effective_dir.display()
                    ))
                    .size(12.0)
                    .color(palette.neutral_status_fg),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                    if styled_compact_button(
                        ui,
                        if pt { "Abrir pasta" } else { "Open folder" },
                        false,
                        palette,
                    )
                    .on_hover_text(effective_dir.to_string_lossy().as_ref())
                    .clicked()
                    {
                        let _ = std::fs::create_dir_all(effective_dir);
                        if let Err(err) = settings::open_in_explorer(effective_dir) {
                            self.message = err.clone();
                            self.toasts.error(err);
                        }
                    }
                    let has_terminal = self
                        .downloads
                        .manager
                        .items
                        .iter()
                        .any(|item| item.status.is_terminal());
                    if has_terminal
                        && styled_ghost_button(
                            ui,
                            if pt {
                                "Limpar concluídos"
                            } else {
                                "Clear finished"
                            },
                            palette,
                        )
                        .clicked()
                    {
                        self.downloads.manager.clear_finished();
                    }
                    if self.downloads.manager.has_active()
                        && styled_danger_button(
                            ui,
                            if pt { "Cancelar todos" } else { "Cancel all" },
                            palette,
                        )
                        .clicked()
                    {
                        self.downloads.manager.cancel_all();
                    }
                });
                if !downloads::has_ffmpeg() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(if pt {
                            "Modo direto ativo (sem dependência de FFmpeg ou assinatura)"
                        } else {
                            "Direct stream mode active (no FFmpeg or subscription needed)"
                        })
                        .size(11.5)
                        .color(palette.neutral_status_fg),
                    );
                }
            });

        if submit_download {
            let raw = self.downloads.direct_url_input.trim().to_owned();
            if let Some(video) = parse_direct_youtube_input(&raw) {
                self.downloads.direct_url_input.clear();
                self.downloads.notice = None;
                self.start_download_video(video, self.downloads.selected_format, ctx);
            } else {
                self.downloads.notice = Some(
                    if pt {
                        "Informe um link ou ID válido de vídeo do YouTube."
                    } else {
                        "Enter a valid YouTube video URL or ID."
                    }
                    .to_owned(),
                );
            }
        }
    }

    fn downloads_list_column_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &ThemePalette,
    ) {
        let pt = self.ui_pt();
        let tab = self.downloads.filter_tab;
        let filtered: Vec<downloads::DownloadItem> = self
            .downloads
            .manager
            .items
            .iter()
            .rev()
            .filter(|item| match tab {
                1 => !download_format_is_audio(item.format),
                2 => download_format_is_audio(item.format),
                3 => matches!(
                    item.status,
                    downloads::DownloadStatus::Queued | downloads::DownloadStatus::Running { .. }
                ),
                4 => matches!(item.status, downloads::DownloadStatus::Completed { .. }),
                _ => true,
            })
            .cloned()
            .collect();

        if self.downloads.manager.items.is_empty() {
            empty_state_card_ui(
                ui,
                if pt {
                    "Nenhum download na fila"
                } else {
                    "No downloads yet"
                },
                if pt {
                    "Cole um link no painel ao lado ou use o menu '...' de qualquer vídeo para baixar MP4 ou áudio."
                } else {
                    "Paste a link on the right rail or use the '...' menu on any video card to download MP4 or audio."
                },
                palette,
            );
            return;
        }

        if filtered.is_empty() {
            empty_state_card_ui(
                ui,
                if pt {
                    "Nenhum download neste filtro"
                } else {
                    "No downloads match this filter"
                },
                if pt {
                    "Altere os chips acima ou selecione 'Tudo'."
                } else {
                    "Change the filter chips above or select 'All'."
                },
                palette,
            );
            return;
        }

        let mut cancel_id: Option<u64> = None;
        let mut retry_id: Option<u64> = None;
        let mut remove_id: Option<u64> = None;
        let mut open_path: Option<std::path::PathBuf> = None;
        let thumb_w: f32 = if ui.available_width() >= 640.0 {
            196.0
        } else {
            144.0
        };
        let thumb_h = (thumb_w * 9.0 / 16.0).round();

        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt("browse_downloads_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for item in &filtered {
                    let fmt_label = if pt {
                        item.format.label_pt()
                    } else {
                        item.format.label_en()
                    };
                    let thumb_url = download_item_thumb_url(item);
                    let row_resp = ui.horizontal_top(|ui| {
                        ui.set_min_width(ui.available_width());
                        let _ = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            thumb_url.as_ref(),
                            egui::vec2(thumb_w, thumb_h),
                            10.0,
                        );
                        ui.add_space(14.0);
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - 150.0).max(120.0));
                            ui.label(
                                RichText::new(&item.video.title)
                                    .size(15.5)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                            ui.add_space(3.0);
                            ui.label(
                                RichText::new(format!("{} · {fmt_label}", item.video.channel))
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            );
                            ui.add_space(6.0);
                            match &item.status {
                                downloads::DownloadStatus::Queued => {
                                    ui.label(
                                        RichText::new(if pt {
                                            "Na fila aguardando..."
                                        } else {
                                            "Queued..."
                                        })
                                        .small()
                                        .color(palette.neutral_status_fg),
                                    );
                                }
                                downloads::DownloadStatus::Running {
                                    progress_percent,
                                    speed,
                                    eta,
                                } => {
                                    let frac = (*progress_percent / 100.0).clamp(0.0, 1.0);
                                    let mut details = vec![format!("{progress_percent:.1}%")];
                                    if let Some(spd) = speed {
                                        details.push(spd.clone());
                                    }
                                    if let Some(rem) = eta {
                                        details.push(format!("ETA {rem}"));
                                    }
                                    ui.add(
                                        egui::ProgressBar::new(frac)
                                            .desired_width(ui.available_width().min(360.0))
                                            .text(details.join(" · ")),
                                    );
                                }
                                downloads::DownloadStatus::Completed { output_path } => {
                                    let label = if let Some(path) = output_path {
                                        format!(
                                            "{}: {}",
                                            if pt { "Concluído" } else { "Completed" },
                                            path.file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("arquivo")
                                        )
                                    } else if pt {
                                        "Concluído".to_owned()
                                    } else {
                                        "Completed".to_owned()
                                    };
                                    ui.label(
                                        RichText::new(label)
                                            .small()
                                            .color(palette.watched_status_fg),
                                    );
                                }
                                downloads::DownloadStatus::Failed(err) => {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}: {err}",
                                            if pt { "Falhou" } else { "Failed" }
                                        ))
                                        .small()
                                        .color(palette.error_banner_fg),
                                    );
                                }
                                downloads::DownloadStatus::Cancelled => {
                                    ui.label(
                                        RichText::new(if pt { "Cancelado" } else { "Cancelled" })
                                            .small()
                                            .color(palette.neutral_status_fg),
                                    );
                                }
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            match &item.status {
                                downloads::DownloadStatus::Queued
                                | downloads::DownloadStatus::Running { .. } => {
                                    if styled_danger_button(
                                        ui,
                                        if pt { "Cancelar" } else { "Cancel" },
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        cancel_id = Some(item.id);
                                    }
                                }
                                downloads::DownloadStatus::Completed { output_path } => {
                                    if styled_icon_chip_button(ui, true, "✕", palette)
                                        .on_hover_text(if pt {
                                            "Remover da lista"
                                        } else {
                                            "Remove from list"
                                        })
                                        .clicked()
                                    {
                                        remove_id = Some(item.id);
                                    }
                                    let folder_target = output_path
                                        .as_deref()
                                        .and_then(|p| p.parent())
                                        .map(|p| p.to_path_buf())
                                        .unwrap_or_else(|| item.output_dir.clone());
                                    if styled_compact_button(
                                        ui,
                                        if pt {
                                            "Mostrar na pasta"
                                        } else {
                                            "Show in folder"
                                        },
                                        false,
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        open_path = Some(folder_target);
                                    }
                                }
                                downloads::DownloadStatus::Failed(_)
                                | downloads::DownloadStatus::Cancelled => {
                                    if styled_icon_chip_button(ui, true, "✕", palette)
                                        .on_hover_text(if pt {
                                            "Remover da lista"
                                        } else {
                                            "Remove from list"
                                        })
                                        .clicked()
                                    {
                                        remove_id = Some(item.id);
                                    }
                                    if styled_compact_button(
                                        ui,
                                        if pt {
                                            "↻ Tentar novamente"
                                        } else {
                                            "↻ Retry"
                                        },
                                        false,
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        retry_id = Some(item.id);
                                    }
                                }
                            }
                        });
                    });
                    paint_library_card_hover_glow(
                        ui,
                        row_resp.response.rect.expand2(egui::vec2(6.0, 4.0)),
                        10.0,
                        ui.make_persistent_id(("download_card_hover", item.id)),
                        palette,
                    );
                    ui.add_space(12.0);
                }
            });
        self.thumbnails.end_pass(ctx);

        if let Some(id) = cancel_id {
            self.downloads.manager.cancel(id, ctx);
        }
        if let Some(id) = retry_id {
            self.downloads.manager.retry(id, ctx);
        }
        if let Some(id) = remove_id {
            self.downloads.manager.remove(id);
        }
        if let Some(path) = open_path
            && let Err(err) = settings::open_in_explorer(&path)
        {
            self.message = err.clone();
            self.toasts.error(err);
        }
    }

    pub(crate) fn ensure_liked_videos_merged_cache(&mut self) {
        let key = Some((
            self.library.revision,
            self.library.data.favorites.len(),
            self.library.liked_videos.revision,
            self.library.liked_videos.items.len(),
        ));
        if self.library.liked_videos.merged_cache_key == key {
            return;
        }
        self.library.liked_videos.merged_videos.clear();
        self.library.liked_videos.merged_is_short.clear();
        self.library.liked_videos.extra_local_count = 0;
        let mut seen = std::collections::HashSet::new();
        for item in &self.library.liked_videos.items {
            let k = history::youtube_video_id(&item.video.url)
                .unwrap_or_else(|| item.video.url.clone());
            if seen.insert(k) {
                let is_short = item.metadata.kind == search::SearchResultKind::Short
                    || item.video.url.contains("/shorts/");
                self.library
                    .liked_videos
                    .merged_videos
                    .push(item.video.clone());
                self.library.liked_videos.merged_is_short.push(is_short);
            }
        }
        for rec in &self.library.data.favorites {
            let k =
                history::youtube_video_id(&rec.video.url).unwrap_or_else(|| rec.video.url.clone());
            if seen.insert(k) {
                let is_short = self
                    .video_metadata
                    .get(&rec.video.url)
                    .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                    || video_is_short(&rec.video);
                self.library
                    .liked_videos
                    .merged_videos
                    .push(rec.video.clone());
                self.library.liked_videos.merged_is_short.push(is_short);
                self.library.liked_videos.extra_local_count += 1;
            }
        }
        self.library.liked_videos.merged_cache_key = key;
    }

    pub(crate) fn liked_videos_display_total(&self) -> usize {
        let remote_loaded = self.library.liked_videos.items.len();
        let remote_total = self
            .library
            .liked_videos
            .total_results
            .map(|t| (t as usize).max(remote_loaded))
            .unwrap_or(remote_loaded);
        if self.library.liked_videos.rx.is_some() {
            remote_total.max(self.library.liked_videos.merged_videos.len())
        } else {
            remote_total.saturating_add(self.library.liked_videos.extra_local_count)
        }
    }

    pub(crate) fn playlists_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        if let Some(idx) = self.library.selected_collection_idx {
            if let Some(col) = self.library.data.collections.get(idx).cloned() {
                let videos: Vec<Video> = col.videos.iter().map(|r| r.video.clone()).collect();
                let is_short_flags: Vec<bool> = videos
                    .iter()
                    .map(|v| {
                        self.metadata_for(&v.url)
                            .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                            || video_is_short(v)
                    })
                    .collect();
                let count = videos.len();
                let count_fmt = format_grouped_count(count, pt);
                let owner = self
                    .auth
                    .my_channel_title
                    .clone()
                    .unwrap_or_else(|| if pt { "Privada" } else { "Private" }.to_owned());
                let subtitle = if pt {
                    format!(
                        "{owner} · {count_fmt} {} · Playlist local",
                        if count == 1 { "vídeo" } else { "vídeos" }
                    )
                } else {
                    format!(
                        "{owner} · {count_fmt} {} · Local playlist",
                        if count == 1 { "video" } else { "videos" }
                    )
                };
                let (chosen, remove_url, _, _) = self.youtube_playlist_detail_page_ui(
                    ui,
                    ctx,
                    &videos,
                    &is_short_flags,
                    PlaylistDetailSpec {
                        title: &col.name,
                        subtitle: &subtitle,
                        empty_message: if pt {
                            "Nenhum vídeo nesta playlist ainda."
                        } else {
                            "No videos in this playlist yet."
                        },
                        scroll_id: "playlist_detail_local_scroll",
                        can_back_to_playlists: true,
                        remove_label: Some(if pt {
                            "Remover da playlist"
                        } else {
                            "Remove from playlist"
                        }),
                        loading: false,
                        can_refresh: false,
                        error: None,
                    },
                    palette,
                );
                if let Some(url) = remove_url
                    && self.library.data.remove_from_collection(idx, &url)
                {
                    self.save_library();
                }
                return chosen;
            }
            self.library.selected_collection_idx = None;
        }

        if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
            let videos = std::mem::take(&mut self.feed.videos);
            let is_short_flags: Vec<bool> = videos
                .iter()
                .map(|v| {
                    self.video_metadata
                        .get(&v.url)
                        .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                        || v.url.contains("/shorts/")
                })
                .collect();
            let loaded_count = videos.len();
            let total_count = self
                .feed
                .home
                .playlist_total_results
                .map(|t| (t as usize).max(loaded_count))
                .unwrap_or(loaded_count);
            let total_fmt = format_grouped_count(total_count, pt);
            let loaded_fmt = format_grouped_count(loaded_count, pt);
            let fallback_title = if pt {
                "Playlist do YouTube"
            } else {
                "YouTube Playlist"
            };
            let custom_title = self.library.active_remote_playlist_title.clone();
            let title = custom_title.as_deref().unwrap_or(fallback_title);
            let loading = self.feed.home.rx.is_some();
            let subtitle = if loading {
                if total_count > 0 {
                    if pt {
                        format!(
                            "{total_fmt} vídeos (carregando {loaded_fmt}/{total_fmt}...) · Conta Google"
                        )
                    } else {
                        format!(
                            "{total_fmt} videos (loading {loaded_fmt}/{total_fmt}...) · Google Account"
                        )
                    }
                } else if pt {
                    "Carregando vídeos da playlist...".to_owned()
                } else {
                    "Loading playlist videos...".to_owned()
                }
            } else if pt {
                format!(
                    "{total_fmt} {} · Conta Google",
                    if total_count == 1 {
                        "vídeo"
                    } else {
                        "vídeos"
                    }
                )
            } else {
                format!(
                    "{total_fmt} {} · Google Account",
                    if total_count == 1 { "video" } else { "videos" }
                )
            };
            let (chosen, _, _, _) = self.youtube_playlist_detail_page_ui(
                ui,
                ctx,
                &videos,
                &is_short_flags,
                PlaylistDetailSpec {
                    title,
                    subtitle: &subtitle,
                    empty_message: if pt {
                        "Nenhum vídeo encontrado nesta playlist."
                    } else {
                        "No videos found in this playlist."
                    },
                    scroll_id: "playlist_detail_remote_scroll",
                    can_back_to_playlists: true,
                    remove_label: None,
                    loading,
                    can_refresh: false,
                    error: None,
                },
                palette,
            );
            self.feed.videos = videos;
            return chosen;
        }

        self.ensure_liked_videos_merged_cache();
        let liked_display_total = self.liked_videos_display_total();
        let liked_cover_thumb = self
            .library
            .liked_videos
            .merged_videos
            .first()
            .map(video_thumb_url)
            .unwrap_or_default();

        let dark_theme = self.settings.theme.value == Theme::Dark;
        let watch_later_videos: Vec<Video> = self
            .library
            .data
            .watch_later
            .iter()
            .map(|r| r.video.clone())
            .collect();
        let collections: Vec<(usize, String, Vec<Video>)> = self
            .library
            .data
            .collections
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    i,
                    c.name.clone(),
                    c.videos.iter().map(|r| r.video.clone()).collect(),
                )
            })
            .collect();
        let remote_playlists = self.auth.remote_playlists.clone();

        let mut open_section: Option<BrowseSection> = None;
        let mut open_local_idx: Option<usize> = None;
        let mut open_remote: Option<(String, String)> = None;

        egui::ScrollArea::vertical()
            .id_salt("browse_playlists_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Playlists")
                            .size(28.0)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let create_clicked = styled_pill_button(
                            ui,
                            if pt { "Criar" } else { "Create" },
                            false,
                            &palette,
                        )
                        .clicked();
                        let name_resp = ui.add_sized(
                            [210.0, 30.0],
                            egui::TextEdit::singleline(&mut self.library.collection_name)
                                .hint_text(if pt {
                                    "Nova playlist..."
                                } else {
                                    "New playlist..."
                                }),
                        );
                        if create_clicked
                            || (name_resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            let candidate = std::mem::take(&mut self.library.collection_name);
                            if !candidate.trim().is_empty() {
                                match self.library.data.create_collection(&candidate) {
                                    Ok(_) => {
                                        self.save_library();
                                    }
                                    Err(err) => {
                                        self.library.notice = Some(err.to_owned());
                                    }
                                }
                            }
                        }
                    });
                });
                ui.add_space(10.0);

                let chips = if pt {
                    [(0_u8, "Tudo"), (1, "Criadas por você"), (2, "Salvas")]
                } else {
                    [(0_u8, "All"), (1, "Owned"), (2, "Saved")]
                };
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    for (chip, lbl) in chips {
                        if styled_pill_button(
                            ui,
                            lbl,
                            self.library.playlist_chip_filter == chip,
                            &palette,
                        )
                        .clicked()
                        {
                            self.library.playlist_chip_filter = chip;
                        }
                    }
                });
                ui.add_space(16.0);

                struct PlaylistCardEntry {
                    title: String,
                    meta: String,
                    badge: String,
                    thumb_url: String,
                    action_kind: u8,
                    action_idx: usize,
                    action_str: String,
                }

                let filter = self.library.playlist_chip_filter;
                let mut cards: Vec<PlaylistCardEntry> = Vec::new();
                if filter == 0 || filter == 2 {
                    cards.push(PlaylistCardEntry {
                        title: if pt {
                            "Assistir mais tarde"
                        } else {
                            "Watch later"
                        }
                        .into(),
                        meta: if pt {
                            "Privada · Playlist"
                        } else {
                            "Private · Playlist"
                        }
                        .into(),
                        badge: format!(
                            "{} {}",
                            watch_later_videos.len(),
                            if pt { "vídeos" } else { "videos" }
                        ),
                        thumb_url: watch_later_videos
                            .first()
                            .map(video_thumb_url)
                            .unwrap_or_default(),
                        action_kind: 0,
                        action_idx: 0,
                        action_str: String::new(),
                    });
                    cards.push(PlaylistCardEntry {
                        title: if pt {
                            "Vídeos com \"Gostei\""
                        } else {
                            "Liked videos"
                        }
                        .into(),
                        meta: if pt {
                            "Privada · Playlist"
                        } else {
                            "Private · Playlist"
                        }
                        .into(),
                        badge: format!(
                            "{} {}",
                            format_grouped_count(liked_display_total, pt),
                            if pt {
                                if liked_display_total == 1 {
                                    "vídeo"
                                } else {
                                    "vídeos"
                                }
                            } else if liked_display_total == 1 {
                                "video"
                            } else {
                                "videos"
                            }
                        ),
                        thumb_url: liked_cover_thumb,
                        action_kind: 1,
                        action_idx: 0,
                        action_str: String::new(),
                    });
                }
                if filter == 0 || filter == 1 {
                    for (idx, name, vids) in collections {
                        cards.push(PlaylistCardEntry {
                            title: name,
                            meta: if pt {
                                "Local · Ver playlist completa"
                            } else {
                                "Local · View full playlist"
                            }
                            .into(),
                            badge: format!(
                                "{} {}",
                                vids.len(),
                                if pt { "vídeos" } else { "videos" }
                            ),
                            thumb_url: vids.first().map(video_thumb_url).unwrap_or_default(),
                            action_kind: 2,
                            action_idx: idx,
                            action_str: String::new(),
                        });
                    }
                    for (pl_id, pl_title) in remote_playlists {
                        cards.push(PlaylistCardEntry {
                            title: pl_title,
                            meta: if pt {
                                "YouTube · Ver playlist completa"
                            } else {
                                "YouTube · View full playlist"
                            }
                            .into(),
                            badge: "Playlist".into(),
                            thumb_url: String::new(),
                            action_kind: 3,
                            action_idx: 0,
                            action_str: pl_id,
                        });
                    }
                }

                let cols = layout::compute_home_grid_columns(ui.available_width()).clamp(1, 5);
                let spacing = 16.0;
                let card_w = ((ui.available_width() - spacing * ((cols.saturating_sub(1)) as f32))
                    / (cols as f32))
                    .max(190.0);
                let thumb_h = (card_w * 9.0 / 16.0).round();

                self.thumbnails.begin_pass();
                for row in cards.chunks(cols) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = spacing;
                        for card in row {
                            let resp = ui.vertical(|ui| {
                                ui.set_width(card_w);
                                ui.set_max_width(card_w);
                                ui.add_space(7.0);
                                let thumb_resp = if !card.thumb_url.trim().is_empty() {
                                    self.thumbnails.show_rounded(
                                        ui,
                                        ctx,
                                        &card.thumb_url,
                                        egui::vec2(card_w, thumb_h),
                                        10.0,
                                    )
                                } else {
                                    let (rect, r) = ui.allocate_exact_size(
                                        egui::vec2(card_w, thumb_h),
                                        egui::Sense::click(),
                                    );
                                    ui.painter().rect_filled(rect, 10.0, palette.card_alt_bg);
                                    r
                                };
                                paint_mix_top_tabs(ui, thumb_resp.rect, dark_theme);
                                paint_playlist_badge_overlay(ui, thumb_resp.rect, &card.badge);
                                ui.add_space(8.0);
                                let title_resp = ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(&card.title)
                                                .size(14.5)
                                                .strong()
                                                .color(palette.channel_fg),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                ui.label(
                                    RichText::new(&card.meta)
                                        .size(12.5)
                                        .color(palette.neutral_status_fg),
                                );
                                (thumb_resp, title_resp)
                            });
                            let (thumb_resp, title_resp) = resp.inner;
                            if thumb_resp.clicked() || title_resp.clicked() {
                                match card.action_kind {
                                    0 => open_section = Some(BrowseSection::WatchLater),
                                    1 => open_section = Some(BrowseSection::LikedVideos),
                                    2 => open_local_idx = Some(card.action_idx),
                                    _ => {
                                        open_remote =
                                            Some((card.action_str.clone(), card.title.clone()))
                                    }
                                }
                            }
                        }
                    });
                    ui.add_space(16.0);
                }
                self.thumbnails.end_pass(ctx);
            });

        if let Some(sec) = open_section {
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = sec;
            if sec == BrowseSection::LikedVideos && self.auth.authenticated {
                self.start_liked_videos_fetch(false, ctx);
            }
        } else if let Some(idx) = open_local_idx {
            self.library.detail_chip_filter = 0;
            self.library.selected_collection_idx = Some(idx);
        } else if let Some((pl_id, pl_title)) = open_remote {
            self.library.detail_chip_filter = 0;
            self.library.active_remote_playlist_title = Some(pl_title);
            self.start_browser_feed(format!("fasttube-google-playlist:{pl_id}"), false, ctx);
        }
        None
    }

    pub(crate) fn watch_later_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        let videos: Vec<Video> = self
            .library
            .data
            .watch_later
            .iter()
            .map(|r| r.video.clone())
            .collect();
        let is_short_flags: Vec<bool> = videos
            .iter()
            .map(|v| {
                self.metadata_for(&v.url)
                    .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                    || video_is_short(v)
            })
            .collect();
        let count = videos.len();
        let count_fmt = format_grouped_count(count, pt);
        let owner = self
            .auth
            .my_channel_title
            .clone()
            .unwrap_or_else(|| if pt { "Você" } else { "You" }.to_owned());
        let subtitle = if pt {
            format!(
                "{owner} · {count_fmt} {} · Privada",
                if count == 1 { "vídeo" } else { "vídeos" }
            )
        } else {
            format!(
                "{owner} · {count_fmt} {} · Private",
                if count == 1 { "video" } else { "videos" }
            )
        };
        let (chosen, remove_url, _, _) = self.youtube_playlist_detail_page_ui(
            ui,
            ctx,
            &videos,
            &is_short_flags,
            PlaylistDetailSpec {
                title: if pt {
                    "Assistir mais tarde"
                } else {
                    "Watch later"
                },
                subtitle: &subtitle,
                empty_message: if pt {
                    "Nenhum vídeo em Assistir mais tarde. Salve vídeos pelo botão '+' nos cards."
                } else {
                    "No videos in Watch later yet. Save videos using the menu on any video."
                },
                scroll_id: "watch_later_detail_scroll",
                can_back_to_playlists: false,
                remove_label: Some(if pt {
                    "Remover de Assistir mais tarde"
                } else {
                    "Remove from Watch later"
                }),
                loading: false,
                can_refresh: false,
                error: None,
            },
            palette,
        );
        if let Some(url) = remove_url
            && self.library.data.remove_from_watch_later(&url)
        {
            self.save_library();
        }
        chosen
    }

    pub(crate) fn liked_videos_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let pt = self.ui_pt();
        self.ensure_liked_videos_merged_cache();
        let display_total = self.liked_videos_display_total();
        let remote_loaded = self.library.liked_videos.items.len();
        let loading = self.library.liked_videos.rx.is_some();
        let error = self.library.liked_videos.error.clone();
        let can_refresh = self.auth.authenticated;
        let videos = std::mem::take(&mut self.library.liked_videos.merged_videos);
        let is_short_flags = std::mem::take(&mut self.library.liked_videos.merged_is_short);

        let owner = self
            .auth
            .my_channel_title
            .clone()
            .unwrap_or_else(|| if pt { "Você" } else { "You" }.to_owned());
        let total_fmt = format_grouped_count(display_total, pt);
        let loaded_fmt = format_grouped_count(remote_loaded, pt);
        let subtitle = if loading {
            if display_total > 0 {
                if pt {
                    format!(
                        "{owner} · {total_fmt} vídeos (carregando {loaded_fmt}/{total_fmt}...) · Privada"
                    )
                } else {
                    format!(
                        "{owner} · {total_fmt} videos (loading {loaded_fmt}/{total_fmt}...) · Private"
                    )
                }
            } else if pt {
                format!("{owner} · Carregando vídeos com \"Gostei\"... · Privada")
            } else {
                format!("{owner} · Loading liked videos... · Private")
            }
        } else if pt {
            format!(
                "{owner} · {total_fmt} {} · Privada",
                if display_total == 1 {
                    "vídeo"
                } else {
                    "vídeos"
                }
            )
        } else {
            format!(
                "{owner} · {total_fmt} {} · Private",
                if display_total == 1 {
                    "video"
                } else {
                    "videos"
                }
            )
        };
        let (chosen, remove_url, refresh_clicked, cancel_clicked) = self
            .youtube_playlist_detail_page_ui(
                ui,
                ctx,
                &videos,
                &is_short_flags,
                PlaylistDetailSpec {
                    title: if pt {
                        "Vídeos com \"Gostei\""
                    } else {
                        "Liked videos"
                    },
                    subtitle: &subtitle,
                    empty_message: if pt {
                        "Nenhum vídeo com \"Gostei\" ainda. Curta vídeos ou entre com sua conta."
                    } else {
                        "No liked videos yet. Favorite videos or sign in with your account."
                    },
                    scroll_id: "liked_videos_detail_scroll",
                    can_back_to_playlists: false,
                    remove_label: Some(if pt {
                        "Remover dos vídeos com \"Gostei\""
                    } else {
                        "Remove from Liked videos"
                    }),
                    loading,
                    can_refresh,
                    error: error.as_deref(),
                },
                palette,
            );
        self.library.liked_videos.merged_videos = videos;
        self.library.liked_videos.merged_is_short = is_short_flags;

        if cancel_clicked {
            self.cancel_liked_videos_fetch();
        } else if refresh_clicked {
            self.start_liked_videos_fetch(true, ctx);
        }
        if let Some(url) = remove_url
            && self.library.data.remove_from_favorites(&url)
        {
            self.save_library();
        }
        chosen
    }

    fn youtube_playlist_detail_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        videos: &[Video],
        is_short_flags: &[bool],
        spec: PlaylistDetailSpec<'_>,
        palette: ThemePalette,
    ) -> (Option<Video>, Option<String>, bool, bool) {
        let avail_w = ui.available_width().max(320.0);
        let side_by_side = avail_w >= 860.0;
        let filtered_indices: Vec<usize> = (0..videos.len())
            .filter(|&idx| {
                let is_short = is_short_flags.get(idx).copied().unwrap_or(false);
                match self.library.detail_chip_filter {
                    1 => !is_short,
                    2 => is_short,
                    _ => true,
                }
            })
            .collect();
        let cover_url = videos.first().map(video_thumb_url).unwrap_or_default();

        self.thumbnails.begin_pass();
        let (banner_act, list_act) = if side_by_side {
            let left_w = 348.0;
            let right_w = (avail_w - left_w - 24.0).max(320.0);
            let col_h = ui.available_height().max(480.0);
            let mut b_act = PlaylistBannerAction::default();
            let mut l_act = PlaylistListAction::default();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(left_w);
                        b_act = self.playlist_detail_banner_ui(
                            ui,
                            ctx,
                            filtered_indices.is_empty(),
                            &cover_url,
                            &spec,
                            &palette,
                        );
                    },
                );
                ui.add_space(24.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(right_w);
                        l_act = self.playlist_detail_list_ui(
                            ui,
                            ctx,
                            videos,
                            &filtered_indices,
                            &spec,
                            &palette,
                        );
                    },
                );
            });
            (b_act, l_act)
        } else {
            let b_act = self.playlist_detail_banner_ui(
                ui,
                ctx,
                filtered_indices.is_empty(),
                &cover_url,
                &spec,
                &palette,
            );
            ui.add_space(16.0);
            let l_act =
                self.playlist_detail_list_ui(ui, ctx, videos, &filtered_indices, &spec, &palette);
            (b_act, l_act)
        };
        self.thumbnails.end_pass(ctx);

        let mut chosen = list_act.chosen;
        if banner_act.go_back {
            self.library.selected_collection_idx = None;
            self.library.active_remote_playlist_title = None;
            if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
                self.feed.home.title = None;
                self.feed.home.playlist_total_results = None;
            }
        }
        if banner_act.play_all && !filtered_indices.is_empty() {
            let filtered_videos: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            chosen = self.play_all_videos(&filtered_videos);
        } else if banner_act.shuffle && !filtered_indices.is_empty() {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as usize)
                .unwrap_or(1);
            let mut rotated: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            let k = seed % rotated.len();
            rotated.rotate_left(k);
            chosen = self.play_all_videos(&rotated);
        } else if banner_act.enqueue_all && !filtered_indices.is_empty() {
            let filtered_videos: Vec<Video> = filtered_indices
                .iter()
                .map(|&idx| videos[idx].clone())
                .collect();
            let _ = self.enqueue_all_videos(&filtered_videos);
        }
        if let Some(video) = list_act.enqueue_target {
            let _ = self.enqueue_video(video);
        }
        if let Some(video) = list_act.download_target {
            self.start_download_video(video, downloads::DownloadFormat::VideoMp4Best, ctx);
        }
        if let Some(video) = list_act.share_target {
            self.open_share_modal(&video);
        }
        if let Some(target) = list_act.open_channel_target {
            self.open_channel(target, ctx);
        }

        (
            chosen,
            list_act.remove_url,
            banner_act.refresh,
            banner_act.cancel_load,
        )
    }

    fn playlist_detail_banner_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        filtered_empty: bool,
        cover_url: &str,
        spec: &PlaylistDetailSpec<'_>,
        palette: &ThemePalette,
    ) -> PlaylistBannerAction {
        let pt = self.ui_pt();
        let mut act = PlaylistBannerAction::default();
        if spec.can_back_to_playlists
            && styled_pill_button(ui, "‹  Playlists", false, palette).clicked()
        {
            act.go_back = true;
        }
        if spec.can_back_to_playlists {
            ui.add_space(10.0);
        }
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(16.0)
            .inner_margin(egui::Margin::same(22))
            .show(ui, |ui| {
                let banner_w = ui.available_width().max(220.0);
                let cover_h = (banner_w * 9.0 / 16.0).round();
                if !cover_url.trim().is_empty() {
                    let _ = self.thumbnails.show_rounded(
                        ui,
                        ctx,
                        cover_url,
                        egui::vec2(banner_w, cover_h),
                        12.0,
                    );
                } else {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(banner_w, cover_h), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 12.0, palette.card_bg);
                }
                ui.add_space(16.0);
                ui.label(
                    RichText::new(spec.title)
                        .size(24.0)
                        .strong()
                        .color(palette.channel_fg),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(spec.subtitle)
                        .size(13.0)
                        .color(palette.neutral_status_fg),
                );
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    let play_lbl = if pt {
                        "▶  Reproduzir tudo"
                    } else {
                        "▶  Play all"
                    };
                    let play_btn = egui::Button::new(
                        RichText::new(play_lbl)
                            .size(13.5)
                            .strong()
                            .color(palette.active_button_fg),
                    )
                    .fill(palette.active_button_bg)
                    .corner_radius(egui::CornerRadius::same(18))
                    .min_size(egui::vec2(0.0, 34.0));
                    if ui
                        .add_enabled(!filtered_empty, play_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        act.play_all = true;
                    }
                    if styled_pill_button(
                        ui,
                        if pt {
                            "🔀 Aleatório"
                        } else {
                            "🔀 Shuffle"
                        },
                        false,
                        palette,
                    )
                    .clicked()
                        && !filtered_empty
                    {
                        act.shuffle = true;
                    }
                    if styled_pill_button(ui, if pt { "+ Fila" } else { "+ Queue" }, false, palette)
                        .clicked()
                        && !filtered_empty
                    {
                        act.enqueue_all = true;
                    }
                    if spec.loading {
                        if styled_pill_button(
                            ui,
                            if pt { "✕ Cancelar" } else { "✕ Cancel" },
                            false,
                            palette,
                        )
                        .clicked()
                        {
                            act.cancel_load = true;
                        }
                    } else if spec.can_refresh
                        && styled_pill_button(
                            ui,
                            if pt { "↻ Atualizar" } else { "↻ Refresh" },
                            false,
                            palette,
                        )
                        .clicked()
                    {
                        act.refresh = true;
                    }
                });
                if let Some(err) = spec.error {
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(palette.error_bg)
                        .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.label(RichText::new(err).size(12.0).color(palette.error_banner_fg));
                        });
                }
            });
        act
    }

    fn playlist_item_menu_ui(
        &mut self,
        ui: &mut egui::Ui,
        video: &Video,
        menu_id: egui::Id,
        remove_label: Option<&str>,
        palette: &ThemePalette,
        act: &mut PlaylistListAction,
    ) {
        let pt = self.ui_pt();
        let dots = paint_more_dots_button(ui, palette);
        if dots.clicked() {
            egui::Popup::toggle_id(ui.ctx(), menu_id);
        }
        egui::Popup::from_toggle_button_response(&dots)
            .id(menu_id)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                ui.set_min_width(210.0);
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
                    act.enqueue_target = Some(video.clone());
                }
                if search_menu_item_ui(
                    ui,
                    SearchMenuIcon::Share,
                    if pt { "Compartilhar" } else { "Share" },
                    palette,
                )
                .clicked()
                {
                    act.share_target = Some(video.clone());
                }
                if self.channel_target_for_video(video).is_some()
                    && search_menu_item_ui(
                        ui,
                        SearchMenuIcon::Queue,
                        if pt { "Abrir canal" } else { "Open channel" },
                        palette,
                    )
                    .clicked()
                {
                    act.open_channel_target = self.channel_target_for_video(video);
                }
                if search_menu_item_ui(
                    ui,
                    SearchMenuIcon::Download,
                    if pt { "Baixar" } else { "Download" },
                    palette,
                )
                .clicked()
                {
                    act.download_target = Some(video.clone());
                }
                if let Some(rm_lbl) = remove_label {
                    ui.separator();
                    if search_menu_item_ui(ui, SearchMenuIcon::Trash, rm_lbl, palette).clicked() {
                        act.remove_url = Some(video.url.clone());
                    }
                }
            });
    }

    fn playlist_detail_shorts_grid_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        videos: &[Video],
        filtered_indices: &[usize],
        spec: &PlaylistDetailSpec<'_>,
        palette: &ThemePalette,
    ) -> PlaylistListAction {
        let pt = self.ui_pt();
        let mut act = PlaylistListAction::default();
        let avail_w = ui.available_width().max(260.0);
        let gap = 12.0;
        let cols = ((avail_w + gap) / 215.0).floor().clamp(2.0, 5.0) as usize;
        let card_w = ((avail_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(120.0);
        let thumb_h = (card_w * 16.0 / 9.0).round();
        let footer_h = 74.0;
        let row_h = thumb_h + footer_h;
        let total_rows = filtered_indices.len().div_ceil(cols);

        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 14.0;
            egui::ScrollArea::vertical()
                .id_salt((spec.scroll_id, "shorts_grid"))
                .auto_shrink([false, false])
                .show_rows(ui, row_h, total_rows, |ui, row_range| {
                    let row_w = ui.available_width().max(card_w);
                    for row_idx in row_range {
                        let start = row_idx * cols;
                        let end = (start + cols).min(filtered_indices.len());
                        ui.allocate_ui_with_layout(
                            egui::vec2(row_w, row_h),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_height(row_h);
                                ui.set_max_height(row_h);
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap;
                                    for &vid_idx in &filtered_indices[start..end] {
                                        let video = &videos[vid_idx];
                                        let thumb_url = video_thumb_url(video);
                                        let views = self
                                            .metadata_for(&video.url)
                                            .and_then(|m| m.view_count);
                                        ui.push_id(("pl_short_card", spec.scroll_id, vid_idx), |ui| {
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(card_w, row_h),
                                                egui::Layout::top_down(egui::Align::Min),
                                                |ui| {
                                                    ui.set_width(card_w);
                                                    ui.set_max_width(card_w);
                                                    let thumb_resp = if !thumb_url.trim().is_empty() {
                                                        self.thumbnails.show_cover_rounded(
                                                            ui,
                                                            ctx,
                                                            &thumb_url,
                                                            egui::vec2(card_w, thumb_h),
                                                            12.0,
                                                        )
                                                    } else {
                                                        let (r, resp) = ui.allocate_exact_size(
                                                            egui::vec2(card_w, thumb_h),
                                                            egui::Sense::click(),
                                                        );
                                                        ui.painter().rect_filled(
                                                            r,
                                                            12.0,
                                                            palette.card_alt_bg,
                                                        );
                                                        resp
                                                    };
                                                    let hover_t = ctx.animate_bool_with_time(
                                                        ui.make_persistent_id((
                                                            "pl_short_hover",
                                                            spec.scroll_id,
                                                            vid_idx,
                                                        )),
                                                        thumb_resp.hovered(),
                                                        0.14,
                                                    );
                                                    paint_card_hover_highlight(
                                                        ui,
                                                        thumb_resp.rect,
                                                        12.0,
                                                        hover_t,
                                                        palette,
                                                    );
                                                    if thumb_resp.clicked() {
                                                        act.chosen = Some(video.clone());
                                                    }
                                                    ui.add_space(8.0);
                                                    ui.horizontal_top(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 4.0;
                                                        let title_w = (card_w - 28.0).max(56.0);
                                                        ui.vertical(|ui| {
                                                            ui.set_width(title_w);
                                                            ui.set_max_width(title_w);
                                                            let short_title =
                                                                truncate_text_chars(&video.title, 48);
                                                            let title_resp = ui
                                                                .add(
                                                                    egui::Button::new(
                                                                        RichText::new(short_title)
                                                                            .size(14.0)
                                                                            .strong()
                                                                            .color(palette.channel_fg),
                                                                    )
                                                                    .frame(false)
                                                                    .wrap(),
                                                                )
                                                                .on_hover_cursor(
                                                                    egui::CursorIcon::PointingHand,
                                                                )
                                                                .on_hover_text(&video.title);
                                                            if title_resp.clicked() {
                                                                act.chosen = Some(video.clone());
                                                            }
                                                        });
                                                        let menu_id = ui.make_persistent_id((
                                                            "pl_short_menu",
                                                            spec.scroll_id,
                                                            vid_idx,
                                                        ));
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::TOP,
                                                            ),
                                                            |ui| {
                                                                self.playlist_item_menu_ui(
                                                                    ui,
                                                                    video,
                                                                    menu_id,
                                                                    spec.remove_label,
                                                                    palette,
                                                                    &mut act,
                                                                );
                                                            },
                                                        );
                                                    });
                                                    if let Some(count) = views {
                                                        ui.label(
                                                            RichText::new(
                                                                format_playlist_short_views(
                                                                    count, pt,
                                                                ),
                                                            )
                                                            .size(12.5)
                                                            .color(palette.neutral_status_fg),
                                                        );
                                                    } else if !video.channel.trim().is_empty() {
                                                        ui.label(
                                                            RichText::new(&video.channel)
                                                                .size(12.5)
                                                                .color(palette.neutral_status_fg),
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                    }
                                });
                            },
                        );
                    }
                });
        });
        act
    }

    fn playlist_detail_list_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        videos: &[Video],
        filtered_indices: &[usize],
        spec: &PlaylistDetailSpec<'_>,
        palette: &ThemePalette,
    ) -> PlaylistListAction {
        let pt = self.ui_pt();
        let mut act = PlaylistListAction::default();
        let chips = if pt {
            [(0_u8, "Tudo"), (1, "Vídeos"), (2, "Shorts")]
        } else {
            [(0_u8, "All"), (1, "Videos"), (2, "Shorts")]
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (chip, lbl) in chips {
                if styled_pill_button(ui, lbl, self.library.detail_chip_filter == chip, palette)
                    .clicked()
                {
                    self.library.detail_chip_filter = chip;
                }
            }
        });
        ui.add_space(12.0);
        if filtered_indices.is_empty() {
            if spec.loading {
                let loading_msg = if self.library.detail_chip_filter == 2 {
                    if pt {
                        "Carregando Shorts..."
                    } else {
                        "Loading Shorts..."
                    }
                } else if pt {
                    "Carregando vídeos..."
                } else {
                    "Loading videos..."
                };
                empty_state_card_ui(ui, spec.title, loading_msg, palette);
            } else {
                empty_state_card_ui(ui, spec.title, spec.empty_message, palette);
            }
            return act;
        }
        if self.library.detail_chip_filter == 2 {
            return self.playlist_detail_shorts_grid_ui(
                ui,
                ctx,
                videos,
                filtered_indices,
                spec,
                palette,
            );
        }

        let row_h = 94.0;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            egui::ScrollArea::vertical()
                .id_salt(spec.scroll_id)
                .auto_shrink([false, false])
                .show_rows(ui, row_h, filtered_indices.len(), |ui, row_range| {
                    let row_w = ui.available_width();
                    for pos in row_range {
                        let vid_idx = filtered_indices[pos];
                        let video = &videos[vid_idx];
                        let thumb_url = video_thumb_url(video);
                        ui.push_id(("pl_row", spec.scroll_id, vid_idx), |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(row_w, row_h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_min_height(row_h);
                                    ui.set_max_height(row_h);
                                    let row_resp = ui.horizontal_top(|ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.add_sized(
                                            [38.0, 90.0],
                                            egui::Label::new(
                                                RichText::new(format!("{}", pos + 1))
                                                    .size(13.0)
                                                    .color(palette.neutral_status_fg),
                                            ),
                                        );
                                        let thumb_size = egui::vec2(160.0, 90.0);
                                        let thumb_resp = if !thumb_url.trim().is_empty() {
                                            self.thumbnails
                                                .show_rounded(ui, ctx, &thumb_url, thumb_size, 8.0)
                                        } else {
                                            let (r, resp) = ui.allocate_exact_size(
                                                thumb_size,
                                                egui::Sense::click(),
                                            );
                                            ui.painter().rect_filled(r, 8.0, palette.card_alt_bg);
                                            resp
                                        };
                                        paint_thumbnail_overlays(
                                            ui,
                                            thumb_resp.rect,
                                            &video.duration,
                                            None,
                                            false,
                                        );
                                        ui.add_space(12.0);
                                        ui.vertical(|ui| {
                                            ui.set_max_width(
                                                (ui.available_width() - 40.0).max(120.0),
                                            );
                                            let title_resp = ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(&video.title)
                                                            .size(14.5)
                                                            .strong()
                                                            .color(palette.channel_fg),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            ui.add_space(3.0);
                                            let meta = if video.duration.trim().is_empty() {
                                                video.channel.clone()
                                            } else {
                                                format!("{} · {}", video.channel, video.duration)
                                            };
                                            ui.label(
                                                RichText::new(meta)
                                                    .size(12.5)
                                                    .color(palette.neutral_status_fg),
                                            );
                                            if thumb_resp.clicked() || title_resp.clicked() {
                                                act.chosen = Some(video.clone());
                                            }
                                        });
                                        let menu_id = ui.make_persistent_id((
                                            "pl_row_menu",
                                            spec.scroll_id,
                                            vid_idx,
                                        ));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::TOP),
                                            |ui| {
                                                self.playlist_item_menu_ui(
                                                    ui,
                                                    video,
                                                    menu_id,
                                                    spec.remove_label,
                                                    palette,
                                                    &mut act,
                                                );
                                            },
                                        );
                                    });
                                    paint_card_hover_highlight(
                                        ui,
                                        row_resp.response.rect.expand2(egui::vec2(6.0, 4.0)),
                                        10.0,
                                        ctx.animate_bool_with_time(
                                            ui.make_persistent_id((
                                                "pl_row_hover",
                                                spec.scroll_id,
                                                vid_idx,
                                            )),
                                            row_resp.response.hovered(),
                                            0.12,
                                        ),
                                        palette,
                                    );
                                },
                            );
                        });
                    }
                });
        });
        act
    }

    pub(crate) fn insights_section_ui(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        self.ensure_feed_cache();
        let palette = self.palette();
        let pt = self.ui_pt();
        let summary = insights::compute_local_insights(
            &self.history.data,
            &self.library.data,
            &self.feed.cache.profile,
            &self.followed_channels.data,
        );

        ui.heading(if pt {
            "Insights locais de uso"
        } else {
            "Local usage insights"
        });
        ui.label(
            RichText::new(if pt {
                "Estatísticas calculadas 100% localmente a partir do seu histórico, biblioteca e canais seguidos."
            } else {
                "Statistics computed 100% locally from your watch history, library, and followed channels."
            })
            .color(palette.neutral_status_fg),
        );
        ui.add_space(12.0);

        let watch_human = insights::format_watch_duration_human(summary.total_watch_ms, pt);
        let stat_cards: [(&str, String); 6] = [
            (
                if pt { "Tempo assistido" } else { "Watch time" },
                watch_human,
            ),
            (
                if pt {
                    "Vídeos no histórico"
                } else {
                    "Videos in history"
                },
                summary.total_videos.to_string(),
            ),
            (
                if pt { "Concluídos" } else { "Completed" },
                format!(
                    "{} ({} {})",
                    summary.completed_videos,
                    summary.in_progress_videos,
                    if pt { "em andamento" } else { "in progress" }
                ),
            ),
            (
                if pt {
                    "Canais seguidos locais"
                } else {
                    "Local followed channels"
                },
                summary.followed_channels_count.to_string(),
            ),
            (
                if pt { "Biblioteca" } else { "Library" },
                format!(
                    "{} fav · {} {} · {} {}",
                    summary.favorites_count,
                    summary.watch_later_count,
                    if pt { "depois" } else { "later" },
                    summary.collections_count,
                    if pt { "coleções" } else { "collections" }
                ),
            ),
            (
                if pt { "Marcadores" } else { "Bookmarks" },
                summary.bookmarks_count.to_string(),
            ),
        ];

        egui::ScrollArea::vertical()
            .id_salt("browse_insights_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail = ui.available_width().max(240.0);
                let cols: usize = if avail >= 720.0 {
                    3
                } else if avail >= 440.0 {
                    2
                } else {
                    1
                };
                let card_w =
                    ((avail - 12.0 * (cols.saturating_sub(1) as f32)) / cols as f32).max(160.0);
                for row in stat_cards.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for (title, value) in row {
                            egui::Frame::new()
                                .fill(palette.card_bg)
                                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(10)
                                .inner_margin(egui::Margin::symmetric(14, 12))
                                .show(ui, |ui| {
                                    ui.set_min_width(card_w - 28.0);
                                    ui.set_max_width(card_w - 28.0);
                                    ui.label(
                                        RichText::new(*title)
                                            .small()
                                            .color(palette.neutral_status_fg),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(value)
                                            .size(18.0)
                                            .strong()
                                            .color(palette.channel_fg),
                                    );
                                });
                        }
                    });
                    ui.add_space(10.0);
                }

                ui.add_space(8.0);
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            RichText::new(if pt {
                                "Top canais mais assistidos"
                            } else {
                                "Top watched channels"
                            })
                            .size(15.0)
                            .strong()
                            .color(palette.channel_fg),
                        );
                        ui.add_space(8.0);
                        if summary.top_channels.is_empty() {
                            ui.label(
                                RichText::new(if pt {
                                    "Assista a alguns vídeos para ver o ranking dos seus canais."
                                } else {
                                    "Watch a few videos to see your top channels ranking."
                                })
                                .small()
                                .color(palette.neutral_status_fg),
                            );
                        } else {
                            let max_ms = summary
                                .top_channels
                                .iter()
                                .map(|c| c.watch_ms)
                                .max()
                                .unwrap_or(1)
                                .max(1);
                            for stat in &summary.top_channels {
                                let frac = (stat.watch_ms as f32 / max_ms as f32).clamp(0.05, 1.0);
                                let dur = insights::format_watch_duration_human(stat.watch_ms, pt);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&stat.channel)
                                            .strong()
                                            .color(palette.channel_fg),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{dur} · {} {}",
                                                    stat.videos,
                                                    if pt { "vídeos" } else { "videos" }
                                                ))
                                                .small()
                                                .color(palette.neutral_status_fg),
                                            );
                                        },
                                    );
                                });
                                ui.add(
                                    egui::ProgressBar::new(frac)
                                        .desired_width(ui.available_width())
                                        .desired_height(6.0),
                                );
                                ui.add_space(6.0);
                            }
                        }
                    });

                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            RichText::new(if pt {
                                "Tópicos com maior afinidade local"
                            } else {
                                "Top local interest topics"
                            })
                            .size(15.0)
                            .strong()
                            .color(palette.channel_fg),
                        );
                        ui.add_space(8.0);
                        if summary.top_topics.is_empty() {
                            ui.label(
                                RichText::new(if pt {
                                    "Nenhum tópico aprendido no perfil local ainda."
                                } else {
                                    "No learned topics in local profile yet."
                                })
                                .small()
                                .color(palette.neutral_status_fg),
                            );
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                for (topic, score) in &summary.top_topics {
                                    egui::Frame::new()
                                        .fill(palette.card_alt_bg)
                                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                        .corner_radius(14)
                                        .inner_margin(egui::Margin::symmetric(10, 4))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(format!("{topic} ({score:.1})"))
                                                    .small()
                                                    .color(palette.channel_fg),
                                            );
                                        });
                                }
                            });
                        }
                    });
            });
    }
}

struct PlaylistDetailSpec<'a> {
    title: &'a str,
    subtitle: &'a str,
    empty_message: &'a str,
    scroll_id: &'a str,
    can_back_to_playlists: bool,
    remove_label: Option<&'a str>,
    loading: bool,
    can_refresh: bool,
    error: Option<&'a str>,
}

#[derive(Default)]
struct PlaylistBannerAction {
    go_back: bool,
    play_all: bool,
    shuffle: bool,
    enqueue_all: bool,
    refresh: bool,
    cancel_load: bool,
}

#[derive(Default)]
struct PlaylistListAction {
    chosen: Option<Video>,
    enqueue_target: Option<Video>,
    download_target: Option<Video>,
    share_target: Option<Video>,
    open_channel_target: Option<ChannelTarget>,
    remove_url: Option<String>,
}

fn format_grouped_count(n: usize, pt: bool) -> String {
    let sep = if pt { '.' } else { ',' };
    let digits = n.to_string();
    let len = digits.len();
    if len <= 3 {
        return digits;
    }
    let mut out = String::with_capacity(len + (len - 1) / 3);
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

fn format_playlist_short_views(views: u64, pt: bool) -> String {
    if pt {
        if views >= 1_000_000_000 {
            let billions = views as f64 / 1_000_000_000.0;
            if billions >= 10.0 || (views % 1_000_000_000) < 100_000_000 {
                format!("{} bi de visualizações", views / 1_000_000_000)
            } else {
                format!(
                    "{} bi de visualizações",
                    format!("{billions:.1}").replace('.', ",")
                )
            }
        } else if views >= 1_000_000 {
            let millions = views as f64 / 1_000_000.0;
            if millions >= 10.0 || (views % 1_000_000) < 100_000 {
                format!("{} mi de visualizações", views / 1_000_000)
            } else {
                format!(
                    "{} mi de visualizações",
                    format!("{millions:.1}").replace('.', ",")
                )
            }
        } else if views >= 1_000 {
            format!("{} mil visualizações", views / 1_000)
        } else if views == 1 {
            "1 visualização".to_owned()
        } else {
            format!("{views} visualizações")
        }
    } else if views >= 1_000_000_000 {
        let billions = views as f64 / 1_000_000_000.0;
        if billions >= 10.0 || (views % 1_000_000_000) < 100_000_000 {
            format!("{}B views", views / 1_000_000_000)
        } else {
            format!("{billions:.1}B views")
        }
    } else if views >= 1_000_000 {
        let millions = views as f64 / 1_000_000.0;
        if millions >= 10.0 || (views % 1_000_000) < 100_000 {
            format!("{}M views", views / 1_000_000)
        } else {
            format!("{millions:.1}M views")
        }
    } else if views >= 1_000 {
        format!("{}K views", views / 1_000)
    } else if views == 1 {
        "1 view".to_owned()
    } else {
        format!("{views} views")
    }
}

fn video_is_short(video: &Video) -> bool {
    if video.url.contains("/shorts/") {
        return true;
    }
    let parts: Vec<&str> = video.duration.trim().split(':').collect();
    match parts.as_slice() {
        [m, s] => {
            if let (Ok(mins), Ok(secs)) = (m.parse::<u64>(), s.parse::<u64>()) {
                mins == 0 || (mins == 1 && secs <= 2)
            } else {
                false
            }
        }
        _ => false,
    }
}

fn video_thumb_url(video: &Video) -> String {
    if !video.thumbnail.trim().is_empty() {
        video.thumbnail.clone()
    } else if let Some(vid) = history::youtube_video_id(&video.url) {
        format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg")
    } else {
        String::new()
    }
}

fn download_format_is_audio(fmt: downloads::DownloadFormat) -> bool {
    matches!(
        fmt,
        downloads::DownloadFormat::AudioM4a
            | downloads::DownloadFormat::AudioOpus
            | downloads::DownloadFormat::AudioMp3
    )
}

fn download_item_thumb_url(item: &downloads::DownloadItem) -> std::borrow::Cow<'_, str> {
    if !item.video.thumbnail.trim().is_empty() {
        std::borrow::Cow::Borrowed(item.video.thumbnail.as_str())
    } else if let Some(vid) = history::youtube_video_id(&item.video.url) {
        std::borrow::Cow::Owned(format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg"))
    } else {
        std::borrow::Cow::Borrowed("")
    }
}

pub(crate) fn parse_direct_youtube_input(raw: &str) -> Option<Video> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let video_id = if history::is_valid_video_id(trimmed) {
        Some(trimmed.to_owned())
    } else {
        history::youtube_video_id(trimmed)
    }?;
    Some(Video {
        title: format!("YouTube ({video_id})"),
        channel: "YouTube".to_owned(),
        url: history::youtube_watch_url(&video_id)?,
        thumbnail: format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"),
        duration: String::new(),
    })
}
