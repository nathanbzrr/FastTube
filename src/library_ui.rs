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
        );
        if let Some(error) = &self.queue.persistence_error {
            ui.colored_label(Color32::RED, error);
        }
        if let Some(error) = &self.queue.error {
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
        if let Some(notice) = &self.queue.notice {
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
                RichText::new(self.pick(
                    "Fila vazia. Adicione resultados com '+ Fila' e avance manualmente com '⏭ Próximo'.",
                    "Queue is empty. Add videos with '+ Queue' and advance manually with '⏭ Next'.",
                ))
                .small()
                .weak(),
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
            if ui
                .button(
                    RichText::new(
                        self.pick("Salvar fila como coleção", "Save queue as collection"),
                    )
                    .small(),
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
        ui.add_space(4.0);

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
        scroll.show(ui, |ui| {
            let target_width = max_card_width
                .map(|limit_w| ui.available_width().min(limit_w))
                .unwrap_or_else(|| ui.available_width());
            for (index, (entry_id, item)) in queued.iter().enumerate() {
                ui.push_id((entry_id, index), |ui| {
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
                                ui.horizontal_top(|ui| {
                                    let badge = if is_next {
                                        format!("{} #{}", self.pick("Próximo", "Next"), index + 1)
                                    } else {
                                        format!("#{}", index + 1)
                                    };
                                    ui.label(RichText::new(badge).small().strong());
                                    ui.vertical(|ui| {
                                        let text_w = (ui.available_width() - 96.0).max(60.0);
                                        ui.set_max_width(text_w);
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(&item.title).small().strong(),
                                                )
                                                .frame(false)
                                                .wrap(),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(self.pick(
                                                "Reproduzir este item agora",
                                                "Play this item now",
                                            ))
                                            .clicked()
                                        {
                                            play_now_action = Some(*entry_id);
                                        }
                                        if !item.channel.is_empty() || !item.duration.is_empty() {
                                            let sub: std::borrow::Cow<'_, str> = if item
                                                .channel
                                                .is_empty()
                                            {
                                                std::borrow::Cow::Borrowed(item.duration.as_str())
                                            } else if item.duration.is_empty() {
                                                std::borrow::Cow::Borrowed(item.channel.as_str())
                                            } else {
                                                std::borrow::Cow::Owned(format!(
                                                    "{} • {}",
                                                    item.channel, item.duration
                                                ))
                                            };
                                            ui.label(RichText::new(sub.as_ref()).small().weak());
                                        }
                                        if index > 0
                                            && ui
                                                .small_button(
                                                    self.pick("Tocar em seguida", "Play next"),
                                                )
                                                .on_hover_text(self.pick(
                                                    "Mover para o topo da fila",
                                                    "Move to the top of the queue",
                                                ))
                                                .clicked()
                                        {
                                            move_top_action = Some(index);
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
                                                .on_hover_text(
                                                    self.pick(
                                                        "Remover da fila",
                                                        "Remove from queue",
                                                    ),
                                                )
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
                                                .on_hover_text(self.pick(
                                                    "Mover para baixo na fila",
                                                    "Move down in queue",
                                                ))
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
                                                .on_hover_text(self.pick(
                                                    "Mover para cima na fila",
                                                    "Move up in queue",
                                                ))
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
        compact: bool,
        len: usize,
        limit: usize,
        duration_summary: &str,
        has_queued: bool,
        repeat_mode: queue::RepeatMode,
        pt: bool,
    ) -> QueueHeadingActions {
        let mut actions = QueueHeadingActions::default();
        ui.horizontal_wrapped(|ui| {
            let heading_size = if compact { 15.0 } else { 17.0 };
            ui.label(
                RichText::new(if pt {
                    "Fila de reprodução"
                } else {
                    "Playback queue"
                })
                .size(heading_size)
                .strong(),
            );
            if has_queued {
                ui.label(
                    RichText::new(format!("{duration_summary} ({len}/{limit})"))
                        .small()
                        .weak(),
                );
            } else {
                ui.label(RichText::new(format!("({len}/{limit})")).small().weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        has_queued,
                        egui::Button::new(
                            RichText::new(if pt { "⏭ Próximo" } else { "⏭ Next" }).small(),
                        ),
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
                if ui
                    .button(RichText::new(repeat_label).small())
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
                    if ui
                        .button(
                            RichText::new(if pt { "Limpar fila" } else { "Clear queue" }).small(),
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
                    if ui
                        .add_enabled(
                            len > 1,
                            egui::Button::new(
                                RichText::new(if pt {
                                    "Remover duplicados"
                                } else {
                                    "Deduplicate"
                                })
                                .small(),
                            ),
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
                    if ui
                        .add_enabled(
                            len > 1,
                            egui::Button::new(
                                RichText::new(if pt { "Embaralhar" } else { "Shuffle" }).small(),
                            ),
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
        actions
    }

    pub(crate) fn history_entry_card_ui(
        &self,
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

    pub(crate) fn history_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
        max_height: Option<f32>,
    ) -> Option<Video> {
        self.ensure_history_synchronized();
        let entry_count = self.history.data.entries().len();
        if entry_count == 0 {
            return None;
        }
        let current_video_id = self
            .selected
            .as_ref()
            .and_then(|video| history::youtube_video_id(&video.url));
        let palette = self.palette();

        let mut clear_clicked = false;
        ui.horizontal(|ui| {
            let heading_size = if compact { 15.0 } else { 17.0 };
            ui.label(RichText::new("Histórico local").size(heading_size).strong());
            ui.label(
                RichText::new(format!("({entry_count} vídeos)"))
                    .small()
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new("Limpar histórico").small())
                    .on_hover_text("Apagar todos os itens do histórico local")
                    .clicked()
                {
                    clear_clicked = true;
                }
            });
        });
        if clear_clicked {
            self.clear_history();
            return None;
        }
        ui.add_space(4.0);

        let entries: Vec<HistoryEntry> = self.history.data.entries().to_vec();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            let search_w = if compact { 150.0 } else { 220.0 };
            ui.add_sized(
                [search_w, 24.0],
                egui::TextEdit::singleline(&mut self.history.search_query)
                    .hint_text("Filtrar por título ou canal"),
            );
            if !self.history.search_query.is_empty() && ui.small_button("✕").clicked() {
                self.history.search_query.clear();
            }
            for (filter, label) in [
                (history::HistoryFilter::All, "Todos"),
                (history::HistoryFilter::InProgress, "Em andamento"),
                (history::HistoryFilter::Completed, "Concluídos"),
            ] {
                let selected = self.history.filter == filter;
                if ui
                    .selectable_label(selected, RichText::new(label).small())
                    .clicked()
                {
                    self.history.filter = filter;
                }
            }
        });
        ui.add_space(4.0);

        let stats = history::compute_watch_stats(&entries);
        let mut stats_line = format!(
            "Total: {} assistido(s) · Concluídos: {}",
            stats.total_videos, stats.completed_count
        );
        if !stats.top_channels.is_empty() {
            let top: Vec<String> = stats
                .top_channels
                .iter()
                .map(|(ch, count)| format!("{ch} ({count})"))
                .collect();
            stats_line.push_str(&format!(" · Top canais: {}", top.join(", ")));
        }
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new(stats_line)
                        .small()
                        .color(palette.neutral_status_fg),
                );
            });
        ui.add_space(4.0);

        let filtered =
            history::filter_entries(&entries, &self.history.search_query, self.history.filter);
        if filtered.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Nenhum vídeo corresponde ao filtro.")
                        .small()
                        .weak(),
                );
                if ui.small_button("Limpar filtro").clicked() {
                    self.history.search_query.clear();
                    self.history.filter = history::HistoryFilter::All;
                }
            });
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
            for (slot_idx, bucket) in buckets.iter().enumerate() {
                if bucket.is_empty() {
                    continue;
                }
                ui.label(
                    RichText::new(groups[slot_idx].label_pt())
                        .small()
                        .strong()
                        .color(palette.neutral_status_fg),
                );
                ui.add_space(2.0);
                for entry in bucket {
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

    pub(crate) fn library_video_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        title: &str,
        description: &str,
        videos: &[Video],
        scroll_id: &str,
    ) -> (Option<Video>, Option<String>) {
        let palette = self.palette();
        let mut chosen = None;
        let mut remove_url = None;
        let mut play_all_clicked = false;
        let mut enqueue_all_clicked = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(title).size(20.0).strong());
            if !videos.is_empty() {
                ui.label(
                    RichText::new(format!("({} itens)", videos.len()))
                        .size(12.5)
                        .color(palette.neutral_status_fg),
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("▶ Reproduzir tudo")
                                .size(12.5)
                                .strong()
                                .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .corner_radius(16),
                    )
                    .on_hover_text("Reproduzir o primeiro vídeo e enfileirar os restantes")
                    .clicked()
                {
                    play_all_clicked = true;
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("+ Fila (tudo)")
                                .size(12.5)
                                .color(palette.header_button_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(16),
                    )
                    .on_hover_text("Adicionar todos os vídeos desta lista à fila")
                    .clicked()
                {
                    enqueue_all_clicked = true;
                }
            }
        });
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
            return (chosen, None);
        }

        self.thumbnails.begin_pass();
        egui::ScrollArea::horizontal()
            .id_salt(scroll_id)
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    for video in videos.iter().take(24) {
                        ui.push_id((scroll_id, &video.url), |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(350.0);
                                ui.set_max_width(350.0);
                                if self.watch_recommendation_card_ui(ui, ctx, video, true) {
                                    chosen = Some(video.clone());
                                }
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button("✕ Remover")
                                        .on_hover_text("Remover este vídeo da lista")
                                        .clicked()
                                    {
                                        remove_url = Some(video.url.clone());
                                    }
                                });
                            });
                        });
                    }
                });
            });
        self.thumbnails.end_pass(ctx);
        (chosen, remove_url)
    }

    pub(crate) fn library_header_ui(
        &mut self,
        ui: &mut egui::Ui,
        watch_later_count: usize,
        favorites_count: usize,
        collections_count: usize,
        bookmarks_count: usize,
        palette: ThemePalette,
    ) {
        ui.label(RichText::new("Você").size(28.0).strong());
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{} salvos · {} favoritos · {} coleções · {} marcadores",
                watch_later_count, favorites_count, collections_count, bookmarks_count
            ))
            .size(13.0)
            .color(palette.neutral_status_fg),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [300.0, 32.0],
                egui::TextEdit::singleline(&mut self.library.search_query)
                    .hint_text("Filtrar biblioteca por título, canal, coleção ou marcador..."),
            );
            if !self.library.search_query.is_empty() && ui.small_button("✕ Limpar").clicked() {
                self.library.search_query.clear();
            }
        });
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
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(notice).small().color(palette.notice_fg));
                        if ui.small_button("✕").clicked() {
                            dismiss = true;
                        }
                    });
                });
            if dismiss {
                self.library.notice = None;
            }
        }
        ui.add_space(16.0);
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
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Coleções").size(20.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{total_collections}"))
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
                        egui::TextEdit::singleline(&mut self.library.collection_name)
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
                        let name_input = self.library.collection_name.clone();
                        match self.library.data.create_collection(&name_input) {
                            Ok(col_idx) => {
                                let created = self.library.data.collections[col_idx].name.clone();
                                self.library.collection_name.clear();
                                self.library.error = None;
                                self.library.notice =
                                    Some(format!("Coleção \"{created}\" criada."));
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
            egui::Frame::new()
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(16, 16))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let msg = if has_filter {
                        "Nenhuma coleção corresponde ao filtro."
                    } else {
                        "Crie coleções para organizar vídeos por assunto ou projeto."
                    };
                    ui.label(
                        RichText::new(msg)
                            .size(13.0)
                            .color(palette.neutral_status_fg),
                    );
                });
        } else {
            for (col_idx, name, videos, total_in_col) in collections {
                ui.push_id(("library_col_controls", *col_idx), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if self.library.renaming_collection == Some(*col_idx) {
                            ui.add_sized(
                                [200.0, 28.0],
                                egui::TextEdit::singleline(&mut self.library.rename_draft)
                                    .hint_text("Novo nome da coleção"),
                            );
                            if ui.small_button("Salvar").clicked() {
                                let draft = self.library.rename_draft.clone();
                                match self.library.data.rename_collection(*col_idx, &draft) {
                                    Ok(()) => {
                                        self.library.renaming_collection = None;
                                        self.library.rename_draft.clear();
                                        self.library.error = None;
                                        self.library.notice = Some("Coleção renomeada.".to_owned());
                                        self.save_library();
                                    }
                                    Err(err) => {
                                        self.library.error = Some(err.to_owned());
                                    }
                                }
                            }
                            if ui.small_button("Cancelar").clicked() {
                                self.library.renaming_collection = None;
                                self.library.rename_draft.clear();
                            }
                        } else if self.library.confirm_delete_collection == Some(*col_idx) {
                            ui.label(
                                RichText::new(format!(
                                    "Excluir coleção \"{name}\" ({total_in_col} itens)?"
                                ))
                                .size(12.5)
                                .color(palette.warning_fg),
                            );
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Confirmar exclusão")
                                            .size(12.0)
                                            .strong()
                                            .color(palette.error_banner_fg),
                                    )
                                    .fill(palette.error_bg),
                                )
                                .clicked()
                                && self.library.data.delete_collection(*col_idx).is_some()
                            {
                                self.library.confirm_delete_collection = None;
                                self.library.renaming_collection = None;
                                self.library.error = None;
                                self.library.notice = Some(format!("Coleção \"{name}\" excluída."));
                                self.save_library();
                            }
                            if ui.small_button("Cancelar").clicked() {
                                self.library.confirm_delete_collection = None;
                            }
                        } else {
                            if ui
                                .small_button("Renomear")
                                .on_hover_text("Renomear esta coleção")
                                .clicked()
                            {
                                self.library.renaming_collection = Some(*col_idx);
                                self.library.rename_draft = name.clone();
                                self.library.confirm_delete_collection = None;
                            }
                            if ui
                                .small_button("Excluir coleção")
                                .on_hover_text("Excluir esta coleção da biblioteca")
                                .clicked()
                            {
                                self.library.confirm_delete_collection = Some(*col_idx);
                                self.library.renaming_collection = None;
                            }
                        }
                    });
                });
                let empty_msg = if has_filter {
                    "Nenhum vídeo desta coleção corresponde ao filtro."
                } else {
                    "Esta coleção ainda não tem vídeos."
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
                    self.library.notice = Some(format!("Vídeo removido de \"{name}\"."));
                    self.save_library();
                }
                ui.add_space(18.0);
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
        let mut bookmark_to_open = None;
        let visible_marks: Vec<library::Bookmark> =
            bookmarks.iter().map(|(_, mark)| mark.clone()).collect();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Marcadores").size(20.0).strong());
            ui.label(
                RichText::new(format!("({}/{})", visible_marks.len(), total_bookmarks))
                    .size(12.5)
                    .color(palette.neutral_status_fg),
            );
            if ui
                .add_enabled(
                    !visible_marks.is_empty(),
                    egui::Button::new("Copiar marcadores (Markdown)"),
                )
                .on_hover_text("Copiar marcadores filtrados no formato Markdown")
                .clicked()
            {
                let md = library::format_bookmarks_markdown(&visible_marks);
                let _ = copy_text_to_clipboard(ctx, &md);
                self.library.notice = Some(format!(
                    "{} marcador(es) copiado(s) em Markdown.",
                    visible_marks.len()
                ));
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [260.0, 28.0],
                egui::TextEdit::singleline(&mut self.library.bookmark_filter)
                    .hint_text("Filtrar marcadores por nota ou vídeo..."),
            );
            if !self.library.bookmark_filter.is_empty() && ui.small_button("✕ Limpar").clicked() {
                self.library.bookmark_filter.clear();
            }
        });
        ui.add_space(6.0);
        if bookmarks.is_empty() {
            let empty_msg = if total_bookmarks > 0 {
                "Nenhum marcador corresponde ao filtro."
            } else {
                "Marcadores criados durante a reprodução aparecerão aqui."
            };
            ui.label(
                RichText::new(empty_msg)
                    .size(13.0)
                    .color(palette.neutral_status_fg),
            );
        } else {
            for (orig_idx, mark) in bookmarks {
                let is_editing = self.library.editing_bookmark_idx == Some(*orig_idx);
                ui.push_id(("library_bookmark_row", *orig_idx), |ui| {
                    egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .corner_radius(10)
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
                                        .hint_text("Nota do marcador"),
                                    );
                                    if ui.small_button("Salvar").clicked() {
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
                                                self.library.notice =
                                                    Some("Nota do marcador atualizada.".to_owned());
                                                self.save_library();
                                            }
                                            Err(err) => {
                                                self.library.error = Some(err.to_owned());
                                            }
                                        }
                                    }
                                    if ui.small_button("Cancelar").clicked() {
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
                                            if ui
                                                .small_button("✕")
                                                .on_hover_text("Remover marcador")
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
                                            if ui
                                                .small_button("Editar")
                                                .on_hover_text("Editar nota do marcador")
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
                    watch_later_total,
                    favorites_total,
                    collections_total,
                    bookmarks_total,
                    palette,
                );

                let wl_empty = if has_lib_filter && watch_later_total > 0 {
                    "Nenhum item em Assistir depois corresponde ao filtro."
                } else {
                    "Os vídeos salvos para assistir depois aparecerão aqui."
                };
                let (wl_chosen, wl_remove) = self.library_video_section_ui(
                    ui,
                    ctx,
                    "Assistir depois",
                    wl_empty,
                    &watch_later,
                    "library_watch_later",
                );
                if let Some(video) = wl_chosen {
                    chosen = Some(video);
                }
                if let Some(url) = wl_remove
                    && self.library.data.remove_from_watch_later(&url)
                {
                    self.library.notice = Some("Vídeo removido de Assistir depois.".to_owned());
                    self.save_library();
                }

                ui.add_space(20.0);
                let fav_empty = if has_lib_filter && favorites_total > 0 {
                    "Nenhum item em Favoritos corresponde ao filtro."
                } else {
                    "Favorite vídeos nos cartões para encontrá-los aqui."
                };
                let (fav_chosen, fav_remove) = self.library_video_section_ui(
                    ui,
                    ctx,
                    "Favoritos",
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
                    self.library.notice = Some("Vídeo removido dos Favoritos.".to_owned());
                    self.save_library();
                }

                ui.add_space(22.0);
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

                ui.add_space(4.0);
                bookmark_to_open = self.library_bookmarks_section_ui(
                    ui,
                    ctx,
                    &bookmarks,
                    bookmarks_total,
                    palette,
                );
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

        ui.horizontal_wrapped(|ui| {
            ui.heading(if pt {
                "Downloads offline"
            } else {
                "Offline downloads"
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(if pt { "Abrir pasta" } else { "Open folder" })
                    .on_hover_text(effective_dir.to_string_lossy().as_ref())
                    .clicked()
                {
                    let _ = std::fs::create_dir_all(&effective_dir);
                    if let Err(err) = settings::open_in_explorer(&effective_dir) {
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
                    && ui
                        .button(if pt {
                            "Limpar concluídos"
                        } else {
                            "Clear finished"
                        })
                        .clicked()
                {
                    self.downloads.manager.clear_finished();
                }
                if self.downloads.manager.has_active()
                    && ui
                        .button(if pt { "Cancelar todos" } else { "Cancel all" })
                        .clicked()
                {
                    self.downloads.manager.cancel_all();
                }
            });
        });
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
            .small()
            .color(palette.neutral_status_fg),
        );
        ui.add_space(10.0);

        if self.downloads.manager.items.is_empty() {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(16, 18))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(if pt {
                            "Nenhum download na fila ainda. Use o menu '...' de qualquer vídeo ou o painel do player para baixar MP4 (1080p/720p/480p) ou áudio (M4A/Opus/MP3)."
                        } else {
                            "No downloads yet. Use the '...' menu on any video card or the watch controls to download MP4 (1080p/720p/480p) or audio (M4A/Opus/MP3)."
                        })
                        .color(palette.neutral_status_fg),
                    );
                });
            return;
        }

        let items = self.downloads.manager.items.clone();
        let mut cancel_id: Option<u64> = None;
        let mut open_path: Option<std::path::PathBuf> = None;

        egui::ScrollArea::vertical()
            .id_salt("browse_downloads_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for item in items.iter().rev() {
                    let fmt_label = if pt {
                        item.format.label_pt()
                    } else {
                        item.format.label_en()
                    };
                    egui::Frame::new()
                        .fill(palette.card_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(10)
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_max_width((ui.available_width() - 150.0).max(120.0));
                                    ui.label(
                                        RichText::new(&item.video.title)
                                            .size(14.0)
                                            .strong()
                                            .color(palette.channel_fg),
                                    );
                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {fmt_label}",
                                            item.video.channel
                                        ))
                                        .small()
                                        .color(palette.neutral_status_fg),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| match &item.status {
                                        downloads::DownloadStatus::Queued
                                        | downloads::DownloadStatus::Running { .. } => {
                                            if ui
                                                .button(if pt { "Cancelar" } else { "Cancel" })
                                                .clicked()
                                            {
                                                cancel_id = Some(item.id);
                                            }
                                        }
                                        downloads::DownloadStatus::Completed { output_path } => {
                                            let folder_target = output_path
                                                .as_deref()
                                                .and_then(|p| p.parent())
                                                .map(|p| p.to_path_buf())
                                                .unwrap_or_else(|| item.output_dir.clone());
                                            if ui
                                                .button(if pt {
                                                    "Mostrar na pasta"
                                                } else {
                                                    "Show in folder"
                                                })
                                                .clicked()
                                            {
                                                open_path = Some(folder_target);
                                            }
                                        }
                                        downloads::DownloadStatus::Failed(_)
                                        | downloads::DownloadStatus::Cancelled => {}
                                    },
                                );
                            });

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
                                            .desired_width(ui.available_width())
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
                                    } else {
                                        if pt { "Concluído" } else { "Completed" }.to_owned()
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
                    ui.add_space(8.0);
                }
            });

        if let Some(id) = cancel_id {
            self.downloads.manager.cancel(id, ctx);
        }
        if let Some(path) = open_path
            && let Err(err) = settings::open_in_explorer(&path)
        {
            self.message = err.clone();
            self.toasts.error(err);
        }
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
