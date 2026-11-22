use super::{empty_state_card_ui, paint_library_card_hover_glow, paint_section_hairline};
use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};

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
        let mut move_bottom_action: Option<usize> = None;
        let mut reorder_action: Option<(usize, usize)> = None;
        let mut remove_action: Option<usize> = None;
        let drag_state_id = egui::Id::new(("fasttube_queue_drag_from", scroll_id));
        let mut active_drag_from = ui.ctx().data(|d| d.get_temp::<usize>(drag_state_id));
        let mut hovered_drop_target: Option<(usize, bool)> = None;
        let pointer_pos = ui
            .ctx()
            .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
        let pointer_released = ui.ctx().input(|i| i.pointer.any_released());
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
                                            let handle_resp = ui
                                                .add(
                                                    egui::Label::new(
                                                        RichText::new("⋮⋮")
                                                            .size(13.0)
                                                            .color(palette.neutral_status_fg),
                                                    )
                                                    .sense(egui::Sense::drag())
                                                    .selectable(false),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::Grab)
                                                .on_hover_text(self.pick(
                                                    "Arraste para reordenar na fila",
                                                    "Drag to reorder in queue",
                                                ));
                                            if handle_resp.drag_started() || handle_resp.dragged() {
                                                active_drag_from = Some(index);
                                                ctx.data_mut(|d| {
                                                    d.insert_temp(drag_state_id, index);
                                                });
                                            }
                                            ui.add_space(4.0);
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
                                                    let meta_w = if queued.len() > 1 {
                                                        (ui.available_width() - 168.0).max(60.0)
                                                    } else {
                                                        ui.available_width().max(60.0)
                                                    };
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(meta_w, 26.0),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            let rel_date = self
                                                                .metadata_for(&item.url)
                                                                .and_then(|meta| {
                                                                    meta.upload_date
                                                                        .as_deref()
                                                                        .and_then(|d| {
                                                                            recommendations::format_youtube_relative_time(d, None)
                                                                        })
                                                                        .or_else(|| {
                                                                            meta.published_time_text
                                                                                .as_deref()
                                                                                .map(str::trim)
                                                                                .filter(|s| !s.is_empty())
                                                                                .map(str::to_owned)
                                                                        })
                                                                });
                                                            let mut parts = Vec::new();
                                                            if !item.channel.is_empty() {
                                                                parts.push(item.channel.as_str());
                                                            }
                                                            if !item.duration.is_empty() {
                                                                parts.push(item.duration.as_str());
                                                            }
                                                            if let Some(date) = rel_date.as_deref() {
                                                                parts.push(date);
                                                            }
                                                            if !parts.is_empty() {
                                                                let sub = parts.join(" • ");
                                                                ui.add(
                                                                    egui::Label::new(
                                                                        RichText::new(sub)
                                                                            .small()
                                                                            .weak(),
                                                                    )
                                                                    .truncate(),
                                                                );
                                                            }
                                                        },
                                                    );
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if index + 1 < queued.len()
                                                                && styled_compact_button(
                                                                    ui,
                                                                    self.pick("Fim", "Bottom"),
                                                                    false,
                                                                    &palette,
                                                                )
                                                                .on_hover_text(self.pick(
                                                                    "Mover para o fim da fila",
                                                                    "Move to the bottom of the queue",
                                                                ))
                                                                .clicked()
                                                            {
                                                                move_bottom_action = Some(index);
                                                            }
                                                            if index > 0
                                                                && styled_compact_button(
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
                                                });
                                            });
                                        });
                                    });
                                let card_rect = frame_resp.response.rect;
                                if let (Some(drag_from), Some(pos)) = (active_drag_from, pointer_pos)
                                    && card_rect.contains(pos)
                                    && drag_from != index
                                {
                                    let insert_after = pos.y > card_rect.center().y;
                                    hovered_drop_target = Some((index, insert_after));
                                    let line_y = if insert_after {
                                        card_rect.bottom() + 2.0
                                    } else {
                                        card_rect.top() - 2.0
                                    };
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(card_rect.left() + 6.0, line_y),
                                            egui::pos2(card_rect.right() - 6.0, line_y),
                                        ],
                                        egui::Stroke::new(2.5, palette.info_accent_fg),
                                    );
                                }
                                paint_library_card_hover_glow(
                                    ui,
                                    card_rect,
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
        if pointer_released && active_drag_from.is_some() {
            if let (Some(from_idx), Some((target_idx, _))) = (active_drag_from, hovered_drop_target)
                && from_idx != target_idx
            {
                reorder_action = Some((from_idx, target_idx));
            }
            ctx.data_mut(|d| d.remove::<usize>(drag_state_id));
        }
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
        if let Some(index) = move_bottom_action
            && self.queue.data.move_to_bottom(index)
        {
            self.persist_queue();
        }
        if let Some((from, to)) = reorder_action
            && self.queue.data.move_item(from, to)
        {
            self.persist_queue();
        }
        if let Some(index) = remove_action {
            self.remove_from_queue(index);
        }
        if actions.cycle_repeat {
            self.cycle_repeat_mode();
            self.queue.data.set_repeat_mode(self.queue.repeat_mode);
            self.persist_queue();
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
}
