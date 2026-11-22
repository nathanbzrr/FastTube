use super::*;
use crate::browse_ui::compute_variable_visible_range;
use eframe::egui::RichText;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LiveChatFilterCacheKey {
    pub(crate) session_id: u64,
    pub(crate) message_count: usize,
    pub(crate) seen_ids_len: usize,
    pub(crate) first_msg_id: String,
    pub(crate) last_msg_id: String,
    pub(crate) filter_query: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CachedLiveChatFilter {
    pub(crate) key: LiveChatFilterCacheKey,
    pub(crate) filtered_indices: Vec<usize>,
}

pub(crate) fn resolve_live_chat_filter_cache(
    existing: Option<&Arc<CachedLiveChatFilter>>,
    key: LiveChatFilterCacheKey,
    messages: &VecDeque<live_chat::LiveChatMessage>,
) -> Arc<CachedLiveChatFilter> {
    if let Some(cached) = existing
        && cached.key == key
    {
        return Arc::clone(cached);
    }
    let trimmed = key.filter_query.trim();
    let filtered_indices = if trimmed.is_empty() {
        (0..messages.len()).collect()
    } else {
        messages
            .iter()
            .enumerate()
            .filter_map(|(idx, msg)| live_chat::message_matches_filter(msg, trimmed).then_some(idx))
            .collect()
    };
    Arc::new(CachedLiveChatFilter {
        key,
        filtered_indices,
    })
}

#[derive(Clone, Default)]
struct LiveChatHeightCache {
    width_key: i32,
    heights: HashMap<String, f32>,
}

pub(crate) fn estimate_live_chat_message_height(
    msg: &live_chat::LiveChatMessage,
    list_width: f32,
) -> f32 {
    let text_w = (list_width - 42.0).max(90.0);
    let chars_per_line = (text_w / 6.8).floor().max(12.0);
    let lines = ((msg.text.chars().count() as f32) / chars_per_line)
        .ceil()
        .max(1.0);
    match &msg.kind {
        live_chat::LiveChatMessageKind::Engagement => 18.0 + lines * 16.0,
        live_chat::LiveChatMessageKind::Paid { .. }
        | live_chat::LiveChatMessageKind::Membership { .. } => {
            let body_h = if msg.text.is_empty() {
                0.0
            } else {
                4.0 + lines * 17.0
            };
            48.0 + body_h
        }
        live_chat::LiveChatMessageKind::Text => (20.0 + lines * 17.0).max(32.0),
    }
}

pub(crate) fn live_chat_visible_range(
    item_heights: &[f32],
    spacing: f32,
    list_top: f32,
    clip_top: f32,
    clip_bottom: f32,
    overscan: usize,
) -> (Range<usize>, f32, f32) {
    compute_variable_visible_range(
        item_heights,
        spacing,
        list_top,
        clip_top,
        clip_bottom,
        overscan,
    )
}

/// Filters live chat replay message indices whose `video_offset_ms` is at or before `playback_pos_ms`.
#[allow(dead_code)]
pub(crate) fn filter_live_chat_replay_indices(
    offsets_ms: &[Option<u64>],
    playback_pos_ms: u64,
) -> Vec<usize> {
    offsets_ms
        .iter()
        .enumerate()
        .filter_map(|(idx, offset)| offset.is_none_or(|ms| ms <= playback_pos_ms).then_some(idx))
        .collect()
}

impl FastTube {
    pub(crate) fn argb_u32_to_color32(argb: u32) -> Color32 {
        let r = ((argb >> 16) & 0xff) as u8;
        let g = ((argb >> 8) & 0xff) as u8;
        let b = (argb & 0xff) as u8;
        Color32::from_rgb(r, g, b)
    }

    pub(crate) fn render_live_chat_runs_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        fallback_text: &str,
        runs: &[live_chat::ChatTextRun],
        text_color: Color32,
        font_size: f32,
    ) {
        let has_media_run = runs.iter().any(|r| r.emoji_url.is_some() || r.is_sticker);
        if runs.is_empty() || !has_media_run {
            let text = if !fallback_text.is_empty() {
                fallback_text
            } else {
                ""
            };
            if !text.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(text).size(font_size).color(text_color)).wrap(),
                );
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for run in runs {
                if let Some(url) = run.emoji_url.as_deref().filter(|u| !u.trim().is_empty()) {
                    let dim = if run.is_sticker { 48.0 } else { 18.0 };
                    if let Some((tex_id, _)) = self.thumbnails.resolve_storyboard_texture(ctx, url)
                    {
                        let img = egui::Image::from_texture((tex_id, egui::vec2(dim, dim)))
                            .corner_radius(if run.is_sticker { 6.0 } else { 2.0 });
                        let resp = ui.add(img);
                        if !run.text.trim().is_empty() {
                            resp.on_hover_text(&run.text);
                        }
                    } else if !run.text.is_empty() {
                        ui.label(RichText::new(&run.text).size(font_size).color(text_color));
                    }
                } else if !run.text.is_empty() {
                    ui.label(RichText::new(&run.text).size(font_size).color(text_color));
                }
            }
        });
    }

    pub(crate) fn render_live_chat_pinned_banner_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pinned: &live_chat::LiveChatMessage,
        palette: &ThemePalette,
        pt: bool,
    ) {
        egui::Frame::new()
            .fill(palette.info_box_bg)
            .stroke(egui::Stroke::new(
                1.0,
                palette.info_accent_fg.gamma_multiply(0.45),
            ))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    self.paint_avatar_or_initials(
                        ui,
                        ctx,
                        pinned.author_avatar_url.as_deref(),
                        &pinned.author_name,
                        22.0,
                        false,
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(if pt {
                                    "\u{1f4cc} Fixada"
                                } else {
                                    "\u{1f4cc} Pinned"
                                })
                                .size(10.5)
                                .strong()
                                .color(palette.info_accent_fg),
                            );
                            ui.label(
                                RichText::new(&pinned.author_name)
                                    .size(11.5)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                        });
                        self.render_live_chat_runs_ui(
                            ui,
                            ctx,
                            &pinned.text,
                            &pinned.runs,
                            palette.channel_fg,
                            12.0,
                        );
                    });
                });
            });
    }

    pub(crate) fn render_live_chat_super_chat_ticker_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        items: &[live_chat::SuperChatTickerItem],
    ) {
        if items.is_empty() {
            return;
        }
        egui::ScrollArea::horizontal()
            .id_salt("watch_live_chat_superchat_ticker")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for item in items.iter().rev().take(12) {
                        let pill_color = if item.color_rgb != 0 {
                            Self::argb_u32_to_color32(item.color_rgb)
                        } else {
                            Color32::from_rgb(21, 101, 192)
                        };
                        egui::Frame::new()
                            .fill(pill_color)
                            .corner_radius(14)
                            .inner_margin(egui::Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    self.paint_avatar_or_initials(
                                        ui,
                                        ctx,
                                        item.avatar_url.as_deref(),
                                        &item.author,
                                        18.0,
                                        false,
                                    );
                                    ui.label(
                                        RichText::new(&item.amount_text)
                                            .size(11.5)
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                });
                            })
                            .response
                            .on_hover_text(format!(
                                "{} \u{00b7} {}",
                                item.author, item.amount_text
                            ));
                    }
                });
            });
    }

    pub(crate) fn render_live_chat_slow_mode_pill_ui(
        ui: &mut egui::Ui,
        slow_mode_secs: u32,
        palette: &ThemePalette,
        pt: bool,
    ) {
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(if pt {
                        format!("\u{1f422} Modo lento: {slow_mode_secs}s")
                    } else {
                        format!("\u{1f422} Slow mode: {slow_mode_secs}s")
                    })
                    .size(11.0)
                    .color(palette.neutral_status_fg),
                );
            });
    }

    pub(crate) fn watch_live_chat_panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        width: f32,
        max_height: f32,
    ) -> bool {
        let pt = self.ui_pt();
        let is_selected_live = self
            .selected
            .as_ref()
            .is_some_and(|video| self.is_video_known_live(video));
        let has_chat = self.watch.live.has_chat();
        let watch_loading = self.watch.live.watch_info_loading;
        if !has_chat && !watch_loading && !is_selected_live {
            return false;
        }
        if !has_chat && !watch_loading && self.watch.live.watch_info_error.is_none() {
            return false;
        }

        let palette = self.palette();
        let panel_width = width.min(ui.available_width()).max(220.0);

        if watch_loading && !has_chat {
            let dark_theme = self.is_dark_theme();
            egui::Frame::new()
                .fill(palette.card_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    let inner_w = (panel_width - 24.0).max(180.0);
                    ui.set_width(inner_w);
                    skeleton::live_chat_skeleton(ui, inner_w, dark_theme);
                });
            return true;
        }

        if !has_chat {
            if let Some(err) = self.watch.live.watch_info_error.as_deref() {
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width((panel_width - 24.0).max(180.0));
                        ui.label(
                            RichText::new(if pt {
                                "Chat ao vivo indispon\u{00ed}vel no momento."
                            } else {
                                "Live chat unavailable right now."
                            })
                            .size(12.0)
                            .color(palette.neutral_status_fg),
                        )
                        .on_hover_text(err);
                    });
                return true;
            }
            return false;
        }

        let is_replay_chat = !is_selected_live
            || self
                .watch
                .live
                .messages
                .iter()
                .any(|m| m.video_offset_ms.is_some());

        if !self.watch.live.chat_visible {
            let show_label = if is_replay_chat {
                if pt {
                    "\u{1f4ac} Mostrar replay do chat"
                } else {
                    "\u{1f4ac} Show chat replay"
                }
            } else if pt {
                "\u{1f4ac} Mostrar chat ao vivo"
            } else {
                "\u{1f4ac} Show live chat"
            };
            let show_btn = egui::Button::new(
                RichText::new(show_label)
                    .size(13.0)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(18);
            if ui
                .add_sized([panel_width, 36.0], show_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.watch.live.chat_visible = true;
                self.watch.live.chat_user_toggled = true;
                self.start_live_chat_stream_if_needed(ctx);
            }
            return true;
        }

        let mut switch_mode_idx: Option<usize> = None;
        let mut hide_chat = false;
        let mut send_draft = false;
        let playback_pos_ms = self.current_player_live_pos_ms();

        egui::Frame::new()
            .fill(palette.card_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width((panel_width - 24.0).max(180.0));
                ui.horizontal(|ui| {
                    if self.watch.live.chat_modes.is_empty() {
                        ui.label(
                            RichText::new(if is_replay_chat {
                                if pt { "Replay do chat" } else { "Chat replay" }
                            } else if pt {
                                "Chat ao vivo"
                            } else {
                                "Live chat"
                            })
                            .size(14.0)
                            .strong()
                            .color(palette.channel_fg),
                        );
                    } else {
                        let current_idx = self
                            .watch
                            .live
                            .chat_modes
                            .iter()
                            .position(|mode| mode.selected)
                            .unwrap_or(0);
                        let mut selected_idx = current_idx;
                        let current_label = self.watch.live.chat_modes[current_idx].label.clone();
                        egui::ComboBox::from_id_salt("watch_live_chat_mode_combo")
                            .selected_text(RichText::new(current_label).size(13.5).strong())
                            .show_ui(ui, |ui| {
                                for (idx, mode) in self.watch.live.chat_modes.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut selected_idx,
                                        idx,
                                        mode.label.as_str(),
                                    );
                                }
                            });
                        if selected_idx != current_idx {
                            switch_mode_idx = Some(selected_idx);
                        }
                    }

                    if is_replay_chat {
                        egui::Frame::new()
                            .fill(palette.info_box_bg)
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                let badge_text = if let Some(pos_ms) = playback_pos_ms {
                                    format!(
                                        "\u{23ee} {}",
                                        overlay::format_time(pos_ms as f64 / 1000.0)
                                    )
                                } else {
                                    "\u{23ee} Replay".to_owned()
                                };
                                ui.label(
                                    RichText::new(badge_text)
                                        .size(10.5)
                                        .strong()
                                        .color(palette.info_accent_fg),
                                );
                            });
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if styled_compact_button(
                            ui,
                            if pt { "Ocultar chat" } else { "Hide chat" },
                            false,
                            &palette,
                        )
                        .clicked()
                        {
                            hide_chat = true;
                        }
                    });
                });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let clear_w = if self.watch.live.filter_query.is_empty() {
                        0.0
                    } else {
                        24.0
                    };
                    let filter_w = ((panel_width - 28.0) - clear_w).max(100.0);
                    ui.add_sized(
                        [filter_w, 26.0],
                        egui::TextEdit::singleline(&mut self.watch.live.filter_query)
                            .id_salt("watch_live_chat_filter_input")
                            .hint_text(if pt {
                                "Filtrar por usu\u{00e1}rio ou palavra..."
                            } else {
                                "Filter by user or keyword..."
                            }),
                    );
                    if !self.watch.live.filter_query.is_empty()
                        && ui
                            .add_sized(
                                [20.0, 26.0],
                                egui::Button::new(RichText::new("\u{00d7}").size(14.0))
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(if pt {
                                "Limpar filtro do chat"
                            } else {
                                "Clear chat filter"
                            })
                            .clicked()
                    {
                        self.watch.live.filter_query.clear();
                    }
                });

                let pinned_msg = self.watch.live.pinned_message.clone();
                if let Some(ref pinned) = pinned_msg {
                    ui.add_space(6.0);
                    self.render_live_chat_pinned_banner_ui(ui, ctx, pinned, &palette, pt);
                }

                let ticker_items: Vec<live_chat::SuperChatTickerItem> =
                    if !self.watch.live.ticker_items.is_empty() {
                        self.watch.live.ticker_items.clone()
                    } else {
                        self.watch
                            .live
                            .messages
                            .iter()
                            .filter_map(|m| match &m.kind {
                                live_chat::LiveChatMessageKind::Paid {
                                    amount, header_bg, ..
                                } => Some(live_chat::SuperChatTickerItem {
                                    id: m.id.clone(),
                                    author: m.author_name.clone(),
                                    amount_text: amount.clone(),
                                    avatar_url: m.author_avatar_url.clone(),
                                    color_rgb: *header_bg,
                                    duration_secs: 60,
                                }),
                                _ => None,
                            })
                            .collect()
                    };
                if !ticker_items.is_empty() {
                    ui.add_space(6.0);
                    self.render_live_chat_super_chat_ticker_ui(ui, ctx, &ticker_items);
                }

                ui.add_space(6.0);
                Self::paint_hairline_divider(ui, &palette);
                ui.add_space(6.0);

                let list_height = (max_height - 190.0).clamp(150.0, 420.0);
                let stick_bottom = self.watch.live.auto_scroll;
                let has_filter = !self.watch.live.filter_query.trim().is_empty();
                let chat_session_id = self.watch.live.session_id;
                let seen_ids_len = self.watch.live.seen_ids.len();
                let messages = std::mem::take(&mut self.watch.live.messages);
                let chat_loading = self.watch.live.chat_loading;
                let chat_error = self.watch.live.chat_error.clone();

                let filter_cache_id = egui::Id::new("watch_live_chat_filter_cache");
                let filter_key = LiveChatFilterCacheKey {
                    session_id: chat_session_id,
                    message_count: messages.len(),
                    seen_ids_len,
                    first_msg_id: messages.front().map(|m| m.id.clone()).unwrap_or_default(),
                    last_msg_id: messages.back().map(|m| m.id.clone()).unwrap_or_default(),
                    filter_query: self.watch.live.filter_query.trim().to_owned(),
                };
                let existing_filter =
                    ctx.data(|d| d.get_temp::<Arc<CachedLiveChatFilter>>(filter_cache_id));
                let cached_filter =
                    resolve_live_chat_filter_cache(existing_filter.as_ref(), filter_key, &messages);
                if existing_filter
                    .as_ref()
                    .is_none_or(|prev| !Arc::ptr_eq(prev, &cached_filter))
                {
                    ctx.data_mut(|d| d.insert_temp(filter_cache_id, Arc::clone(&cached_filter)));
                }

                let synced_replay_indices: Option<Vec<usize>> = if is_replay_chat
                    && let Some(pos_ms) = playback_pos_ms
                    && messages.iter().any(|m| m.video_offset_ms.is_some())
                {
                    Some(
                        cached_filter
                            .filtered_indices
                            .iter()
                            .copied()
                            .filter(|&idx| {
                                messages[idx]
                                    .video_offset_ms
                                    .is_none_or(|offset| offset <= pos_ms)
                            })
                            .collect(),
                    )
                } else {
                    None
                };
                let visible_msg_indices: &[usize] = synced_replay_indices
                    .as_deref()
                    .unwrap_or(&cached_filter.filtered_indices);

                let scroll_output = egui::ScrollArea::vertical()
                    .id_salt("watch_live_chat_scroll")
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .stick_to_bottom(stick_bottom)
                    .show(ui, |ui| {
                        let list_w = (panel_width - 34.0).max(160.0);
                        ui.set_width(list_w);
                        if messages.is_empty() {
                            if chat_loading {
                                ui.add_space(6.0);
                                let dark_theme = self.is_dark_theme();
                                skeleton::live_chat_skeleton(ui, list_w, dark_theme);
                            } else {
                                ui.add_space(12.0);
                                egui::Frame::new()
                                    .fill(palette.card_alt_bg)
                                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                    .corner_radius(12)
                                    .inner_margin(egui::Margin::symmetric(14, 12))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.vertical_centered(|ui| {
                                            if let Some(err) = chat_error.as_deref() {
                                                ui.label(
                                                    RichText::new(err)
                                                        .size(12.0)
                                                        .color(Color32::from_rgb(235, 110, 110)),
                                                );
                                            } else {
                                                ui.label(
                                                    RichText::new(if is_replay_chat {
                                                        if pt {
                                                            "Aguardando mensagens do replay do chat..."
                                                        } else {
                                                            "Waiting for chat replay messages..."
                                                        }
                                                    } else if pt {
                                                        "Aguardando mensagens do chat ao vivo..."
                                                    } else {
                                                        "Waiting for live chat messages..."
                                                    })
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                                );
                                            }
                                        });
                                    });
                            }
                            return;
                        }

                        if visible_msg_indices.is_empty() {
                            if has_filter || is_replay_chat {
                                ui.add_space(12.0);
                                egui::Frame::new()
                                    .fill(palette.card_alt_bg)
                                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                    .corner_radius(12)
                                    .inner_margin(egui::Margin::symmetric(14, 12))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.vertical_centered(|ui| {
                                            ui.label(
                                                RichText::new(if has_filter {
                                                    if pt {
                                                        "Nenhuma mensagem corresponde ao filtro."
                                                    } else {
                                                        "No messages match your filter."
                                                    }
                                                } else if pt {
                                                    "Nenhuma mensagem at\u{00e9} este momento do v\u{00ed}deo."
                                                } else {
                                                    "No chat messages up to this point in the video."
                                                })
                                                .size(12.0)
                                                .color(palette.neutral_status_fg),
                                            );
                                        });
                                    });
                            }
                            return;
                        }

                        let width_key = list_w.round() as i32;
                        let height_cache_id = egui::Id::new("watch_live_chat_height_cache");
                        let mut height_cache = ctx
                            .data(|d| d.get_temp::<LiveChatHeightCache>(height_cache_id))
                            .unwrap_or_default();
                        let mut heights_dirty = false;
                        if height_cache.width_key != width_key {
                            height_cache.width_key = width_key;
                            height_cache.heights.clear();
                            heights_dirty = true;
                        }
                        if height_cache.heights.len() > 400 {
                            height_cache.heights.clear();
                            heights_dirty = true;
                        }

                        let item_heights: Vec<f32> = visible_msg_indices
                            .iter()
                            .map(|&idx| {
                                let msg = &messages[idx];
                                height_cache
                                    .heights
                                    .get(&msg.id)
                                    .copied()
                                    .unwrap_or_else(|| {
                                        estimate_live_chat_message_height(msg, list_w)
                                    })
                            })
                            .collect();

                        let row_gap = 5.0;
                        let inner_spacing_y = ui.spacing().item_spacing.y;
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let list_top = ui.cursor().top();
                        let clip = ui.clip_rect();
                        let (visible_range, top_spacer_h, bottom_spacer_h) =
                            live_chat_visible_range(
                                &item_heights,
                                row_gap,
                                list_top,
                                clip.top(),
                                clip.bottom(),
                                3,
                            );

                        if top_spacer_h > 0.0 {
                            ui.allocate_exact_size(
                                egui::vec2(list_w, top_spacer_h),
                                egui::Sense::hover(),
                            );
                        }

                        let rendered_end = visible_range.end;
                        for pos in visible_range {
                            let msg_idx = visible_msg_indices[pos];
                            let msg = &messages[msg_idx];
                            let row_start_y = ui.cursor().top();
                            ui.scope(|ui| {
                                ui.spacing_mut().item_spacing.y = inner_spacing_y;
                                match &msg.kind {
                                    live_chat::LiveChatMessageKind::Engagement => {
                                        egui::Frame::new()
                                            .fill(palette.card_alt_bg)
                                            .corner_radius(8)
                                            .inner_margin(egui::Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                self.render_live_chat_runs_ui(
                                                    ui,
                                                    ctx,
                                                    &msg.text,
                                                    &msg.runs,
                                                    palette.neutral_status_fg,
                                                    11.5,
                                                );
                                            });
                                    }
                                    live_chat::LiveChatMessageKind::Paid {
                                        amount,
                                        header_bg,
                                        body_bg,
                                    } => {
                                        let hdr_color = Self::argb_u32_to_color32(*header_bg);
                                        let body_color = Self::argb_u32_to_color32(*body_bg);
                                        egui::Frame::new()
                                            .fill(body_color)
                                            .corner_radius(8)
                                            .inner_margin(egui::Margin::symmetric(8, 6))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    self.paint_avatar_or_initials(
                                                        ui,
                                                        ctx,
                                                        msg.author_avatar_url.as_deref(),
                                                        &msg.author_name,
                                                        24.0,
                                                        false,
                                                    );
                                                    ui.vertical(|ui| {
                                                        ui.label(
                                                            RichText::new(&msg.author_name)
                                                                .size(12.0)
                                                                .strong()
                                                                .color(Color32::WHITE),
                                                        );
                                                        egui::Frame::new()
                                                            .fill(hdr_color)
                                                            .corner_radius(4)
                                                            .inner_margin(egui::Margin::symmetric(
                                                                5, 1,
                                                            ))
                                                            .show(ui, |ui| {
                                                                ui.label(
                                                                    RichText::new(amount)
                                                                        .size(11.5)
                                                                        .strong()
                                                                        .color(Color32::WHITE),
                                                                );
                                                            });
                                                    });
                                                });
                                                if !msg.text.is_empty() || !msg.runs.is_empty() {
                                                    ui.add_space(3.0);
                                                    self.render_live_chat_runs_ui(
                                                        ui,
                                                        ctx,
                                                        &msg.text,
                                                        &msg.runs,
                                                        Color32::WHITE,
                                                        12.5,
                                                    );
                                                }
                                            });
                                    }
                                    live_chat::LiveChatMessageKind::Membership {
                                        header_subtext,
                                    } => {
                                        egui::Frame::new()
                                            .fill(Color32::from_rgb(15, 110, 62))
                                            .corner_radius(8)
                                            .inner_margin(egui::Margin::symmetric(8, 6))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    self.paint_avatar_or_initials(
                                                        ui,
                                                        ctx,
                                                        msg.author_avatar_url.as_deref(),
                                                        &msg.author_name,
                                                        24.0,
                                                        false,
                                                    );
                                                    ui.vertical(|ui| {
                                                        ui.label(
                                                            RichText::new(&msg.author_name)
                                                                .size(12.0)
                                                                .strong()
                                                                .color(Color32::WHITE),
                                                        );
                                                        if !header_subtext.is_empty() {
                                                            ui.label(
                                                                RichText::new(header_subtext)
                                                                    .size(11.5)
                                                                    .color(Color32::from_rgb(
                                                                        220, 248, 232,
                                                                    )),
                                                            );
                                                        }
                                                    });
                                                });
                                                if !msg.text.is_empty() || !msg.runs.is_empty() {
                                                    ui.add_space(3.0);
                                                    self.render_live_chat_runs_ui(
                                                        ui,
                                                        ctx,
                                                        &msg.text,
                                                        &msg.runs,
                                                        Color32::WHITE,
                                                        12.5,
                                                    );
                                                }
                                            });
                                    }
                                    live_chat::LiveChatMessageKind::Text => {
                                        ui.horizontal_top(|ui| {
                                            self.paint_avatar_or_initials(
                                                ui,
                                                ctx,
                                                msg.author_avatar_url.as_deref(),
                                                &msg.author_name,
                                                24.0,
                                                false,
                                            );
                                            ui.vertical(|ui| {
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    if let Some(ts) = msg
                                                        .timestamp_text
                                                        .as_deref()
                                                        .filter(|s| !s.trim().is_empty())
                                                        && is_replay_chat
                                                    {
                                                        ui.label(
                                                            RichText::new(ts)
                                                                .size(10.5)
                                                                .color(palette.neutral_status_fg),
                                                        );
                                                    }
                                                    let author_color = if msg.badges.contains(
                                                        &live_chat::LiveChatBadgeKind::Owner,
                                                    ) {
                                                        Color32::from_rgb(255, 214, 0)
                                                    } else if msg.badges.contains(
                                                        &live_chat::LiveChatBadgeKind::Moderator,
                                                    ) {
                                                        Color32::from_rgb(94, 158, 255)
                                                    } else if msg.badges.contains(
                                                        &live_chat::LiveChatBadgeKind::Member,
                                                    ) {
                                                        Color32::from_rgb(43, 166, 64)
                                                    } else {
                                                        palette.neutral_status_fg
                                                    };
                                                    ui.label(
                                                        RichText::new(&msg.author_name)
                                                            .size(12.0)
                                                            .strong()
                                                            .color(author_color),
                                                    );
                                                    for badge in &msg.badges {
                                                        let (badge_text, badge_bg, badge_fg) =
                                                            match badge {
                                                                live_chat::LiveChatBadgeKind::Owner => (
                                                                    if pt {
                                                                        "Propriet\u{00e1}rio"
                                                                    } else {
                                                                        "Owner"
                                                                    },
                                                                    Color32::from_rgb(255, 214, 0),
                                                                    Color32::BLACK,
                                                                ),
                                                                live_chat::LiveChatBadgeKind::Moderator => (
                                                                    "Mod",
                                                                    Color32::from_rgb(47, 111, 216),
                                                                    Color32::WHITE,
                                                                ),
                                                                live_chat::LiveChatBadgeKind::Member => (
                                                                    if pt { "Membro" } else { "Member" },
                                                                    Color32::from_rgb(43, 166, 64),
                                                                    Color32::WHITE,
                                                                ),
                                                                live_chat::LiveChatBadgeKind::Verified => (
                                                                    "\u{2713}",
                                                                    palette.header_button_bg,
                                                                    palette.channel_fg,
                                                                ),
                                                            };
                                                        egui::Frame::new()
                                                            .fill(badge_bg)
                                                            .corner_radius(4)
                                                            .inner_margin(egui::Margin::symmetric(
                                                                4, 1,
                                                            ))
                                                            .show(ui, |ui| {
                                                                ui.label(
                                                                    RichText::new(badge_text)
                                                                        .size(10.0)
                                                                        .strong()
                                                                        .color(badge_fg),
                                                                );
                                                            });
                                                    }
                                                });
                                                if !msg.text.is_empty() || !msg.runs.is_empty() {
                                                    self.render_live_chat_runs_ui(
                                                        ui,
                                                        ctx,
                                                        &msg.text,
                                                        &msg.runs,
                                                        palette.channel_fg,
                                                        12.5,
                                                    );
                                                }
                                            });
                                        });
                                    }
                                }
                            });
                            let measured_h = (ui.cursor().top() - row_start_y).max(20.0);
                            let prev_h = height_cache.heights.get(&msg.id).copied();
                            if prev_h.is_none_or(|h| (h - measured_h).abs() > 1.0) {
                                height_cache.heights.insert(msg.id.clone(), measured_h);
                                heights_dirty = true;
                            }
                            if pos + 1 < rendered_end {
                                ui.add_space(row_gap);
                            }
                        }

                        if bottom_spacer_h > 0.0 {
                            ui.allocate_exact_size(
                                egui::vec2(list_w, bottom_spacer_h),
                                egui::Sense::hover(),
                            );
                        }

                        if heights_dirty {
                            ctx.data_mut(|d| d.insert_temp(height_cache_id, height_cache));
                        }
                    });

                let newly_arrived = std::mem::replace(&mut self.watch.live.messages, messages);
                for msg in newly_arrived {
                    self.watch.live.messages.push_back(msg);
                }

                let max_offset =
                    (scroll_output.content_size.y - scroll_output.inner_rect.height()).max(0.0);
                let (hover_pos, wheel_up, pointer_down) = ui.input(|i| {
                    (
                        i.pointer.hover_pos(),
                        i.smooth_scroll_delta.y > 1.0,
                        i.pointer.any_down(),
                    )
                });
                let over_chat =
                    hover_pos.is_some_and(|pos| scroll_output.inner_rect.contains(pos));
                if max_offset > 4.0
                    && scroll_output.state.offset.y + 12.0 < max_offset
                    && ((over_chat && wheel_up) || pointer_down)
                {
                    self.watch.live.auto_scroll = false;
                } else if max_offset > 4.0 && scroll_output.state.offset.y + 4.0 >= max_offset {
                    self.watch.live.auto_scroll = true;
                }

                if !self.watch.live.auto_scroll {
                    let pill_size = egui::vec2(228.0, 28.0);
                    let pill_rect = egui::Rect::from_center_size(
                        egui::pos2(
                            scroll_output.inner_rect.center().x,
                            (scroll_output.inner_rect.bottom() - 18.0)
                                .max(scroll_output.inner_rect.top() + 14.0),
                        ),
                        pill_size,
                    );
                    let mut pill_clicked = false;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(pill_rect), |ui| {
                        ui.centered_and_justified(|ui| {
                            let jump_btn = egui::Button::new(
                                RichText::new(if pt {
                                    "Rolagem pausada \u{2014} \u{2193} Ir para o fim"
                                } else {
                                    "Scroll paused \u{2014} \u{2193} Jump to bottom"
                                })
                                .size(11.5)
                                .strong()
                                .color(palette.active_button_fg),
                            )
                            .fill(palette.active_button_bg)
                            .corner_radius(14);
                            if ui
                                .add(jump_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                pill_clicked = true;
                            }
                        });
                    });
                    if pill_clicked {
                        self.watch.live.auto_scroll = true;
                        let mut state = scroll_output.state;
                        state.offset.y = max_offset;
                        state.store(ctx, scroll_output.id);
                    }
                }

                if let Some(err) = self
                    .watch
                    .live
                    .send_error
                    .as_deref()
                    .or_else(|| {
                        (!self.watch.live.messages.is_empty())
                            .then_some(self.watch.live.chat_error.as_deref())
                            .flatten()
                    })
                {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(err)
                            .size(11.5)
                            .color(Color32::from_rgb(235, 110, 110)),
                    );
                }

                ui.add_space(6.0);
                Self::paint_hairline_divider(ui, &palette);
                ui.add_space(6.0);

                if let Some(slow_secs) = self.watch.live.slow_mode_secs.filter(|&s| s > 0) {
                    Self::render_live_chat_slow_mode_pill_ui(ui, slow_secs, &palette, pt);
                    ui.add_space(5.0);
                }

                if self.watch.live.send_params.is_some() && !is_replay_chat {
                    let sending = self.watch.live.sending;
                    ui.horizontal(|ui| {
                        let input_w = (ui.available_width() - 78.0).max(100.0);
                        let input_resp = ui.add_enabled(
                            !sending,
                            egui::TextEdit::singleline(&mut self.watch.live.composer_draft)
                                .desired_width(input_w)
                                .hint_text(if pt { "Conversar..." } else { "Chat..." }),
                        );
                        if input_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !self.watch.live.composer_draft.trim().is_empty()
                        {
                            send_draft = true;
                        }
                        let send_enabled =
                            !sending && !self.watch.live.composer_draft.trim().is_empty();
                        let send_btn = egui::Button::new(
                            RichText::new(if sending {
                                "..."
                            } else if pt {
                                "Enviar"
                            } else {
                                "Send"
                            })
                            .size(12.5)
                            .strong(),
                        )
                        .corner_radius(16);
                        if ui
                            .add_enabled(send_enabled, send_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            send_draft = true;
                        }
                    });
                } else if is_replay_chat {
                    ui.label(
                        RichText::new(if pt {
                            "Replay do chat sincronizado com o v\u{00ed}deo."
                        } else {
                            "Chat replay synced with video playback."
                        })
                        .size(11.5)
                        .color(palette.neutral_status_fg),
                    );
                } else {
                    ui.label(
                        RichText::new(if pt {
                            "Entre no YouTube para enviar mensagens no chat."
                        } else {
                            "Sign in to YouTube to send chat messages."
                        })
                        .size(11.5)
                        .color(palette.neutral_status_fg),
                    );
                }
            });

        if hide_chat {
            self.watch.live.chat_visible = false;
            self.watch.live.chat_user_toggled = true;
            self.watch.live.stop_chat_stream();
        } else if let Some(idx) = switch_mode_idx {
            self.switch_live_chat_mode(idx, ctx);
        } else if send_draft {
            self.send_live_chat_draft(true, ctx);
        }
        true
    }
}
