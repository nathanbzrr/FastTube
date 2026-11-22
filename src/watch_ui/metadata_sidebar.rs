use super::*;
use eframe::egui::RichText;
use std::collections::HashMap;

impl FastTube {
    pub(crate) fn looks_like_youtube_channel_id(raw: &str) -> bool {
        let trimmed = raw.trim();
        trimmed.starts_with("UC") && trimmed.len() >= 20 && !trimmed.contains(' ')
    }

    pub(crate) fn format_watch_metadata_stats(
        _video: &Video,
        metadata: Option<&VideoMetadata>,
        live_info: Option<&live_chat::WatchLiveInfo>,
        is_live: bool,
    ) -> Vec<String> {
        let mut stats = Vec::new();
        if is_live {
            if let Some(view_text) = live_info
                .and_then(|info| info.view_count_text.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                stats.push(view_text.to_owned());
            } else if let Some(concurrent) = live_info
                .and_then(|info| info.concurrent_view_count)
                .or_else(|| metadata.and_then(|meta| meta.view_count))
            {
                stats.push(format!("{} assistindo agora", format_thousands(concurrent)));
            }
            if let Some(started) = live_info
                .and_then(|info| info.started_text.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                stats.push(started.to_owned());
            } else if let Some(relative_date) = metadata
                .and_then(|meta| meta.upload_date.as_deref())
                .and_then(|date| recommendations::format_youtube_relative_time(date, None))
                .or_else(|| {
                    metadata
                        .and_then(|meta| meta.published_time_text.as_deref())
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned)
                })
                .or_else(|| {
                    live_info
                        .and_then(|info| info.relative_date_text.as_deref())
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned)
                })
            {
                stats.push(relative_date);
            }
            return stats;
        }
        if let Some(views) = metadata.and_then(|meta| meta.view_count) {
            stats.push(format_view_count(views));
        } else if let Some(view_text) = metadata
            .and_then(|meta| meta.view_count_text.as_deref())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .or_else(|| {
                live_info
                    .and_then(|info| info.view_count_text.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            })
        {
            stats.push(view_text.to_owned());
        }
        if let Some(relative_date) = metadata
            .and_then(|meta| meta.upload_date.as_deref())
            .and_then(|date| recommendations::format_youtube_relative_time(date, None))
            .or_else(|| {
                metadata
                    .and_then(|meta| meta.published_time_text.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                live_info
                    .and_then(|info| info.relative_date_text.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                live_info
                    .and_then(|info| info.started_text.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
            })
        {
            stats.push(relative_date);
        }
        stats
    }

    pub(crate) fn watch_metadata_channel_row_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        display: &WatchChannelDisplay<'_>,
    ) -> (Option<ChannelTarget>, bool) {
        let palette = display.palette;
        let live_channel = self
            .watch
            .live
            .info
            .as_ref()
            .filter(|_| self.watch.live.video_url.as_deref() == Some(video.url.as_str()))
            .and_then(|info| info.channel.as_deref())
            .map(str::trim)
            .filter(|label| !label.is_empty());
        let handle_label = display.channel_target.and_then(|target| match target {
            ChannelTarget::Handle(handle) => {
                let trimmed = handle.trim();
                (!trimmed.is_empty()).then_some(trimmed)
            }
            ChannelTarget::ChannelId(_) => None,
        });
        let channel_label = (!video.channel.trim().is_empty()
            && !Self::looks_like_youtube_channel_id(&video.channel))
        .then_some(video.channel.trim())
        .or_else(|| live_channel.filter(|label| !Self::looks_like_youtube_channel_id(label)))
        .or(handle_label)
        .or_else(|| (!video.channel.trim().is_empty()).then_some(video.channel.trim()))
        .or(live_channel)
        .or_else(|| {
            display
                .channel_target
                .map(channel_target_display_label)
                .filter(|label| !label.trim().is_empty())
        })
        .unwrap_or("Canal")
        .to_owned();
        let avatar = self.paint_avatar_or_initials(
            ui,
            ctx,
            display.channel_avatar_url,
            &channel_label,
            40.0,
            display.channel_target.is_some(),
        );
        let mut open_channel_target = None;
        let mut toggle_subscribe = false;
        if avatar.clicked() {
            open_channel_target = display.channel_target.cloned();
        }

        let live_channel_verified = self
            .watch
            .live
            .info
            .as_ref()
            .filter(|_| self.watch.live.video_url.as_deref() == Some(video.url.as_str()))
            .is_some_and(|i| i.channel_verified);
        let manifest_channel_verified = self
            .playback
            .current_manifest
            .as_ref()
            .filter(|m| {
                history::youtube_video_id(&video.url).as_deref() == Some(m.video_id.as_str())
            })
            .is_some_and(|m| m.channel_verified);
        let channel_verified = is_watch_channel_verified(
            self.metadata_for(&video.url),
            self.channels.page.as_ref(),
            display.channel_target,
        ) || live_channel_verified
            || manifest_channel_verified;
        let channel_hover_text = if let Some(target) = display.channel_target {
            format!(
                "{} ({})",
                self.pick("Abrir canal", "Open channel"),
                target.canonical_url()
            )
        } else {
            self.pick("Abrir detalhes do canal", "Open channel details")
                .to_owned()
        };
        let verified_hover_text = self.pick("Canal verificado", "Verified channel");

        let channel_width = (display.content_width - 110.0).max(120.0);
        ui.vertical(|ui| {
            ui.set_max_width(channel_width);
            let clean_channel = truncate_text_chars(&channel_label, 48);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
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
                    .on_hover_text(channel_hover_text)
                    .clicked()
                {
                    open_channel_target = display.channel_target.cloned();
                }
                if channel_verified {
                    ui.horizontal(|ui| {
                        paint_verified_badge_sized(ui, palette, 14.0);
                    })
                    .response
                    .on_hover_text(verified_hover_text);
                }
            });
            let _ = display.stats;
            if let Some(subs) = display
                .subscriber_count_text
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                let response = ui.label(
                    RichText::new(subs)
                        .size(12.0)
                        .color(palette.neutral_status_fg),
                );
                if let Some(technical) = display.technical_resolution {
                    response.on_hover_text(technical);
                }
            }
        });
        if display.channel_target.is_some() {
            let is_sub = display.subscribed == Some(true);
            let sub_label = if self.auth.authenticated {
                if is_sub {
                    self.pick("✓ Inscrito", "✓ Subscribed")
                } else {
                    self.pick("Inscrever-se", "Subscribe")
                }
            } else if is_sub {
                self.pick("✓ Seguindo", "✓ Following")
            } else {
                self.pick("Seguir", "Follow")
            };
            let sub_btn = egui::Button::new(RichText::new(sub_label).size(13.0).strong().color(
                if is_sub {
                    palette.header_button_fg
                } else {
                    palette.active_button_fg
                },
            ))
            .fill(if is_sub {
                palette.header_button_bg
            } else {
                palette.active_button_bg
            })
            .corner_radius(18);
            let hover_text = if self.auth.authenticated {
                if is_sub {
                    self.pick(
                        "Cancelar inscrição neste canal",
                        "Unsubscribe from this channel",
                    )
                } else {
                    self.pick("Inscrever-se neste canal", "Subscribe to this channel")
                }
            } else if is_sub {
                self.pick(
                    "Deixar de seguir canal localmente",
                    "Unfollow channel locally",
                )
            } else {
                self.pick(
                    "Seguir canal localmente (sem login)",
                    "Follow channel locally (no sign-in required)",
                )
            };
            if ui
                .add_enabled(!display.sub_pending, sub_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(hover_text)
                .clicked()
            {
                toggle_subscribe = true;
            }
        }
        (open_channel_target, toggle_subscribe)
    }

    pub(crate) fn watch_metadata_actions_ui(
        &self,
        ui: &mut egui::Ui,
        state: &WatchMetadataActionState<'_>,
        flags: &mut WatchMetadataActionFlags,
    ) {
        let palette = state.palette;
        ui.horizontal_wrapped(|ui| {
            let like_active = state.current_rating == Some(oauth::VideoRating::Like);
            let like_text = if let Some(ryd) = state.ryd_stats {
                format!("👍 {}", format_thousands(ryd.likes))
            } else if state.pt {
                "👍 Gostei".to_owned()
            } else {
                "👍 Like".to_owned()
            };
            let dislike_active = state.current_rating == Some(oauth::VideoRating::Dislike);
            let dislike_text = if let Some(ryd) = state.ryd_stats {
                format!("👎 {}", format_thousands(ryd.dislikes))
            } else {
                "👎".to_owned()
            };
            let dislike_hover = if let Some(ryd) = state.ryd_stats {
                if state.pt {
                    format!(
                        "Não gostei (Return YouTube Dislike: {} likes / {} dislikes, nota {:.1}/5)",
                        format_thousands(ryd.likes),
                        format_thousands(ryd.dislikes),
                        ryd.rating
                    )
                } else {
                    format!(
                        "Dislike (Return YouTube Dislike: {} likes / {} dislikes, rating {:.1}/5)",
                        format_thousands(ryd.likes),
                        format_thousands(ryd.dislikes),
                        ryd.rating
                    )
                }
            } else if state.pt {
                "Marcar Não gostei no YouTube".to_owned()
            } else {
                "Dislike on YouTube".to_owned()
            };

            let ryd_ratio = state
                .ryd_stats
                .and_then(|ryd| watch_extras::ryd_like_ratio(ryd.likes, ryd.dislikes));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                let pill_resp = egui::Frame::new()
                    .fill(palette.header_button_bg)
                    .corner_radius(18)
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let like_button = egui::Button::new(
                                RichText::new(like_text).size(12.5).strong().color(
                                    if like_active {
                                        palette.active_button_fg
                                    } else {
                                        palette.header_button_fg
                                    },
                                ),
                            )
                            .fill(if like_active {
                                palette.active_button_bg
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .min_size(egui::vec2(78.0, 36.0))
                            .corner_radius(egui::CornerRadius {
                                nw: 18,
                                sw: 18,
                                ne: 0,
                                se: 0,
                            });
                            let like_clicked = ui
                                .add_enabled_ui(
                                    !state.rating_pending && state.video_id.is_some(),
                                    |ui| ui.add(like_button),
                                )
                                .inner
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(if state.pt {
                                    "Marcar Gostei no YouTube"
                                } else {
                                    "Like on YouTube"
                                })
                                .clicked();
                            if like_clicked {
                                flags.rating_to_apply = Some(if like_active {
                                    None
                                } else {
                                    Some(oauth::VideoRating::Like)
                                });
                            }

                            let (sep_rect, _) =
                                ui.allocate_exact_size(egui::vec2(1.0, 22.0), egui::Sense::hover());
                            ui.painter().line_segment(
                                [
                                    egui::pos2(sep_rect.center().x, sep_rect.top()),
                                    egui::pos2(sep_rect.center().x, sep_rect.bottom()),
                                ],
                                egui::Stroke::new(1.0, palette.card_alt_stroke),
                            );

                            let dislike_button = egui::Button::new(
                                RichText::new(dislike_text)
                                    .size(13.5)
                                    .color(if dislike_active {
                                        palette.active_button_fg
                                    } else {
                                        palette.header_button_fg
                                    }),
                            )
                            .fill(if dislike_active {
                                palette.active_button_bg
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .min_size(egui::vec2(46.0, 36.0))
                            .corner_radius(egui::CornerRadius {
                                nw: 0,
                                sw: 0,
                                ne: 18,
                                se: 18,
                            });
                            let dislike_clicked = ui
                                .add_enabled_ui(
                                    !state.rating_pending && state.video_id.is_some(),
                                    |ui| ui.add(dislike_button),
                                )
                                .inner
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(dislike_hover)
                                .clicked();
                            if dislike_clicked {
                                flags.rating_to_apply = Some(if dislike_active {
                                    None
                                } else {
                                    Some(oauth::VideoRating::Dislike)
                                });
                            }
                        });
                    })
                    .response;

                if let (Some(ratio), Some(ryd)) = (ryd_ratio, state.ryd_stats) {
                    let bar_w = pill_resp.rect.width().max(64.0);
                    let (bar_rect, bar_resp) =
                        ui.allocate_exact_size(egui::vec2(bar_w, 3.0), egui::Sense::hover());
                    let track_color = Color32::from_rgba_unmultiplied(220, 72, 72, 145);
                    ui.painter().rect_filled(bar_rect, 1.5, track_color);
                    let fill_w = (bar_rect.width() * ratio).clamp(0.0, bar_rect.width());
                    if fill_w > 0.5 {
                        let fill_rect = egui::Rect::from_min_size(
                            bar_rect.min,
                            egui::vec2(fill_w, bar_rect.height()),
                        );
                        ui.painter()
                            .rect_filled(fill_rect, 1.5, palette.header_button_fg);
                    }
                    let pct = ratio * 100.0;
                    bar_resp.on_hover_text(if state.pt {
                        format!(
                            "{pct:.1}% gostei ({} / {})",
                            format_thousands(ryd.likes),
                            format_thousands(ryd.dislikes)
                        )
                    } else {
                        format!(
                            "{pct:.1}% likes ({} / {})",
                            format_thousands(ryd.likes),
                            format_thousands(ryd.dislikes)
                        )
                    });
                }
            });

            let share_label = if state.share_recently_copied {
                if state.pt {
                    "✓ Copiado"
                } else {
                    "✓ Copied"
                }
            } else if state.pt {
                "Compartilhar"
            } else {
                "Share"
            };
            let share_button = egui::Button::new(RichText::new(share_label).size(13.0).color(
                if state.share_recently_copied {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                },
            ))
            .fill(if state.share_recently_copied {
                palette.active_button_bg
            } else {
                palette.header_button_bg
            })
            .min_size(egui::vec2(96.0, 36.0))
            .corner_radius(18);
            if ui
                .add(share_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if state.pt {
                    "Copiar o endereço deste vídeo"
                } else {
                    "Copy video link"
                })
                .clicked()
            {
                flags.share_video = true;
            }

            let download_btn = egui::Button::new(
                RichText::new(if state.pt {
                    "⬇ Baixar"
                } else {
                    "⬇ Download"
                })
                .size(13.0)
                .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .min_size(egui::vec2(88.0, 36.0))
            .corner_radius(18);
            let dl_resp = ui
                .add(download_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if state.pt {
                    "Baixar vídeo ou áudio offline com yt-dlp"
                } else {
                    "Download video or audio offline with yt-dlp"
                });
            egui::Popup::menu(&dl_resp).show(|ui| {
                ui.set_min_width(190.0);
                for format in downloads::DownloadFormat::ALL {
                    let label = if state.pt {
                        format.label_pt()
                    } else {
                        format.label_en()
                    };
                    if ui.button(label).clicked() {
                        flags.download_format = Some(format);
                        ui.close();
                    }
                }
            });

            let save_label = if state.saved_in_library {
                if state.pt { "✓ Salvo" } else { "✓ Saved" }
            } else if state.pt {
                "Salvar"
            } else {
                "Save"
            };
            let save_button = egui::Button::new(RichText::new(save_label).size(13.0).color(
                if state.save_menu_open {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                },
            ))
            .fill(if state.save_menu_open {
                palette.active_button_bg
            } else {
                palette.header_button_bg
            })
            .corner_radius(18);
            if ui
                .add_sized([80.0, 36.0], save_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if state.pt {
                    "Salvar em Assistir mais tarde ou playlists"
                } else {
                    "Save to Watch later or playlists"
                })
                .clicked()
            {
                flags.toggle_save_menu = true;
            }

            let more_button = egui::Button::new(
                RichText::new("\u{22ef}")
                    .size(16.0)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            let more_resp = ui
                .add_sized([36.0, 36.0], more_button)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if state.pt {
                    "Mais a\u{00e7}\u{00f5}es"
                } else {
                    "More actions"
                });
            egui::Popup::menu(&more_resp).show(|ui| {
                ui.set_min_width(210.0);
                if styled_menu_item_button(
                    ui,
                    if state.pt {
                        "Adicionar \u{00e0} fila"
                    } else {
                        "Add to queue"
                    },
                    false,
                    palette,
                )
                .clicked()
                {
                    flags.enqueue_video = true;
                    ui.close();
                }
                if styled_menu_item_button(
                    ui,
                    if state.pt {
                        "Mostrar transcri\u{00e7}\u{00e3}o"
                    } else {
                        "Show transcript"
                    },
                    false,
                    palette,
                )
                .clicked()
                {
                    flags.open_transcript = true;
                    ui.close();
                }
                ui.separator();
                if styled_menu_item_button(
                    ui,
                    if state.pt { "Denunciar" } else { "Report" },
                    false,
                    palette,
                )
                .clicked()
                {
                    flags.open_report_modal = true;
                    ui.close();
                }
            });
        });
    }

    #[allow(dead_code)]
    pub(crate) fn watch_description_chapters_ui(
        &mut self,
        ui: &mut egui::Ui,
        id_salt: (&str, &str),
        chapters: &[watch_extras::DescriptionChapter],
        palette: &ThemePalette,
    ) {
        if chapters.is_empty() {
            return;
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new(
            RichText::new(format!("Capítulos da descrição ({})", chapters.len()))
                .size(14.0)
                .strong(),
        )
        .id_salt(id_salt)
        .default_open(true)
        .show(ui, |ui| {
            let mut seek_to: Option<f64> = None;
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    for ch in chapters {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{}  {}", ch.label, ch.title))
                                        .size(13.0)
                                        .color(palette.header_button_fg),
                                )
                                .frame(false)
                                .wrap(),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(format!("Ir para {}", ch.label))
                            .clicked()
                        {
                            seek_to = Some(ch.seconds as f64);
                        }
                    }
                });
            if let Some(secs) = seek_to {
                self.seek_to_seconds(secs);
            }
        });
    }

    #[cfg(test)]
    pub(crate) fn watch_metadata_description_ui(
        &mut self,
        ui: &mut egui::Ui,
        video_url: &str,
        content_width: f32,
        palette: &ThemePalette,
    ) {
        let stats = self.watch.metadata_cache.stats.clone();
        self.watch_metadata_description_with_stats_ui(
            ui,
            video_url,
            content_width,
            &stats,
            None,
            palette,
        );
    }

    pub(crate) fn render_rich_description_lines_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        lines: &[watch_extras::CachedTimestampLine],
        color: Color32,
        font_size: f32,
    ) {
        let pt = self.ui_pt();
        let link_color = if self.is_dark_theme() {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let mut seek_secs: Option<f64> = None;
        let mut open_url: Option<String> = None;
        let mut open_mention: Option<String> = None;
        let mut search_hashtag: Option<String> = None;

        for line in lines {
            let tokens = watch_extras::tokenize_description_line(&line.text);
            if tokens.is_empty() {
                ui.add_space(font_size * 0.5);
                continue;
            }
            if tokens.len() == 1
                && let watch_extras::DescriptionToken::Text(plain) = tokens[0]
            {
                ui.add(egui::Label::new(RichText::new(plain).color(color).size(font_size)).wrap());
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for token in tokens {
                    match token {
                        watch_extras::DescriptionToken::Text(text) => {
                            ui.label(RichText::new(text).color(color).size(font_size));
                        }
                        watch_extras::DescriptionToken::Timestamp { raw, secs } => {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(raw)
                                            .color(link_color)
                                            .size(font_size)
                                            .strong(),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .corner_radius(egui::CornerRadius::same(4)),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(if pt {
                                    format!("Ir para {raw}")
                                } else {
                                    format!("Jump to {raw}")
                                })
                                .clicked()
                            {
                                seek_secs = Some(secs);
                            }
                        }
                        watch_extras::DescriptionToken::Url(url) => {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(url).color(link_color).size(font_size),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(if pt {
                                    format!("Abrir link externo: {url}")
                                } else {
                                    format!("Open external link: {url}")
                                })
                                .clicked()
                            {
                                open_url = Some(url.to_owned());
                            }
                        }
                        watch_extras::DescriptionToken::Mention(handle) => {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(handle)
                                            .color(link_color)
                                            .size(font_size)
                                            .strong(),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(if pt {
                                    format!("Abrir canal {handle}")
                                } else {
                                    format!("Open channel {handle}")
                                })
                                .clicked()
                            {
                                open_mention = Some(handle.to_owned());
                            }
                        }
                        watch_extras::DescriptionToken::Hashtag(tag) => {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(tag).color(link_color).size(font_size),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(if pt {
                                    format!("Pesquisar {tag}")
                                } else {
                                    format!("Search {tag}")
                                })
                                .clicked()
                            {
                                search_hashtag = Some(tag.to_owned());
                            }
                        }
                    }
                }
            });
        }

        if let Some(secs) = seek_secs {
            self.seek_to_seconds(secs);
        } else if let Some(url) = open_url {
            open_external_url(&url);
        } else if let Some(handle) = open_mention {
            self.open_channel(ChannelTarget::Handle(handle), ctx);
        } else if let Some(tag) = search_hashtag {
            self.feed.query = tag;
            self.ui.search_results_mode = true;
            self.channels.view_open = false;
            self.start_search(ctx);
        }
    }

    pub(crate) fn render_description_chapter_carousel_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video_url: &str,
        chapters: &[watch_extras::DescriptionChapter],
        palette: &ThemePalette,
    ) {
        if chapters.is_empty() {
            return;
        }
        let pt = self.ui_pt();
        let active_pos_secs = self
            .current_player_live_pos_ms()
            .map(|ms| ms as f64 / 1000.0);
        let active_chapter_idx = active_pos_secs.and_then(|pos| {
            chapters
                .iter()
                .rposition(|ch| (ch.seconds as f64) <= pos + 0.5)
        });
        let mut seek_to: Option<f64> = None;

        ui.label(
            RichText::new(if pt {
                format!("Cap\u{00ed}tulos ({})", chapters.len())
            } else {
                format!("Chapters ({})", chapters.len())
            })
            .size(14.0)
            .strong()
            .color(palette.channel_fg),
        );
        ui.add_space(6.0);

        egui::ScrollArea::horizontal()
            .id_salt(("watch_desc_chapter_carousel", video_url))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    for (idx, ch) in chapters.iter().enumerate() {
                        let is_active = active_chapter_idx == Some(idx);
                        let card_w = 164.0_f32;
                        let thumb_h = 92.0_f32;
                        let card_h = thumb_h + 62.0;
                        let (card_rect, card_resp) = ui
                            .allocate_exact_size(egui::vec2(card_w, card_h), egui::Sense::click());
                        let card_resp = card_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(format!("{} \u{00b7} {}", ch.label, ch.title));

                        let border_color = if is_active {
                            palette.info_accent_fg
                        } else if card_resp.hovered() {
                            palette.header_button_fg.gamma_multiply(0.5)
                        } else {
                            palette.card_alt_stroke
                        };
                        ui.painter().rect_filled(card_rect, 10.0, palette.card_bg);
                        ui.painter().rect_stroke(
                            card_rect,
                            10.0,
                            egui::Stroke::new(if is_active { 1.6 } else { 1.0 }, border_color),
                            egui::StrokeKind::Inside,
                        );

                        let thumb_rect =
                            egui::Rect::from_min_size(card_rect.min, egui::vec2(card_w, thumb_h));
                        let storyboard_slice = if self.playback.storyboard_video_url.as_deref()
                            == Some(video_url)
                            && let Some((sheet_url, uv)) =
                                self.playback.storyboard.as_ref().and_then(|spec| {
                                    spec.lookup(ch.seconds as f64)
                                        .map(|(url, uv)| (url.to_owned(), uv))
                                })
                            && let Some((texture_id, _)) =
                                self.thumbnails.resolve_storyboard_texture(ctx, &sheet_url)
                        {
                            Some((texture_id, uv))
                        } else {
                            None
                        };

                        if let Some((texture_id, uv)) = storyboard_slice {
                            egui::Image::from_texture((texture_id, thumb_rect.size()))
                                .uv(uv)
                                .corner_radius(egui::CornerRadius {
                                    nw: 10,
                                    ne: 10,
                                    sw: 0,
                                    se: 0,
                                })
                                .paint_at(ui, thumb_rect);
                        } else {
                            ui.painter().rect_filled(
                                thumb_rect,
                                egui::CornerRadius {
                                    nw: 10,
                                    ne: 10,
                                    sw: 0,
                                    se: 0,
                                },
                                Color32::from_rgb(24, 27, 36),
                            );
                            ui.painter().text(
                                thumb_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                format!("#{}", idx + 1),
                                egui::FontId::proportional(18.0),
                                Color32::from_rgba_unmultiplied(230, 235, 245, 130),
                            );
                        }

                        // Start timestamp badge inside bottom-right of thumbnail.
                        let ts_galley = ui.painter().layout_no_wrap(
                            ch.label.clone(),
                            egui::FontId::proportional(10.5),
                            Color32::WHITE,
                        );
                        let ts_size =
                            egui::vec2(ts_galley.size().x + 8.0, ts_galley.size().y + 4.0);
                        let ts_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                thumb_rect.right() - ts_size.x - 6.0,
                                thumb_rect.bottom() - ts_size.y - 6.0,
                            ),
                            ts_size,
                        );
                        ui.painter().rect_filled(
                            ts_rect,
                            4.0,
                            if is_active {
                                Color32::from_rgb(255, 0, 51)
                            } else {
                                Color32::from_black_alpha(205)
                            },
                        );
                        ui.painter().galley(
                            egui::pos2(
                                ts_rect.center().x - ts_galley.size().x * 0.5,
                                ts_rect.center().y - ts_galley.size().y * 0.5,
                            ),
                            ts_galley,
                            Color32::WHITE,
                        );

                        // 2-line clamped chapter title + timestamp line below thumbnail.
                        let text_x = card_rect.left() + 8.0;
                        let text_w = (card_w - 16.0).max(40.0);
                        let title_galley = layout_two_line_clamped_galley(
                            ui,
                            &ch.title,
                            text_w,
                            egui::FontId::proportional(12.0),
                            palette.channel_fg,
                        );
                        ui.painter().galley(
                            egui::pos2(text_x, thumb_rect.bottom() + 6.0),
                            title_galley,
                            palette.channel_fg,
                        );
                        let sub_label = if is_active {
                            if pt {
                                format!("{} \u{00b7} Reproduzindo", ch.label)
                            } else {
                                format!("{} \u{00b7} Playing", ch.label)
                            }
                        } else {
                            ch.label.clone()
                        };
                        ui.painter().text(
                            egui::pos2(text_x, card_rect.bottom() - 12.0),
                            egui::Align2::LEFT_CENTER,
                            sub_label,
                            egui::FontId::proportional(11.0),
                            if is_active {
                                palette.info_accent_fg
                            } else {
                                palette.neutral_status_fg
                            },
                        );

                        if card_resp.clicked() {
                            seek_to = Some(ch.seconds as f64);
                        }
                    }
                });
            });

        if let Some(secs) = seek_to {
            self.seek_to_seconds(secs);
        }
    }

    pub(crate) fn watch_metadata_description_with_stats_ui(
        &mut self,
        ui: &mut egui::Ui,
        video_url: &str,
        content_width: f32,
        stats: &[String],
        technical_resolution: Option<&str>,
        palette: &ThemePalette,
    ) {
        let mut chapters = self.watch.metadata_cache.description_chapters.clone();
        if chapters.is_empty()
            && self.watch.overlay_cache.chapters_video_url.as_deref() == Some(video_url)
        {
            chapters = self
                .watch
                .overlay_cache
                .chapters
                .iter()
                .filter(|ch| ch.time_secs.is_finite() && ch.time_secs >= 0.0)
                .map(|ch| {
                    let secs = ch.time_secs.floor() as u64;
                    watch_extras::DescriptionChapter {
                        title: ch.title.clone(),
                        label: overlay::format_time(ch.time_secs),
                        seconds: secs,
                    }
                })
                .collect();
        }
        let has_desc = self.watch.metadata_cache.cleaned_description.is_some();
        if !stats.is_empty() || has_desc || !chapters.is_empty() {
            ui.add_space(10.0);
            if has_desc && self.watch.metadata_cache.description_lines.is_none() {
                let parsed = self
                    .watch
                    .metadata_cache
                    .cleaned_description
                    .as_deref()
                    .map(watch_extras::parse_timestamp_lines)
                    .unwrap_or_default();
                self.watch.metadata_cache.description_lines = Some(parsed);
            }
            let lines = std::mem::take(&mut self.watch.metadata_cache.description_lines);
            let card_inner_w = (content_width - 24.0).max(1.0);
            let pt = self.ui_pt();
            let ctx = ui.ctx().clone();
            let mut next_expanded: Option<bool> = None;
            let mut open_transcript = false;
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(card_inner_w);
                    ui.set_max_width(card_inner_w);
                    if !stats.is_empty() {
                        let stats_line = stats.join("  \u{00b7}  ");
                        let response = ui.label(
                            RichText::new(stats_line)
                                .size(13.0)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if let Some(technical) = technical_resolution {
                            response.on_hover_text(technical);
                        }
                    }
                    let line_count = lines
                        .as_deref()
                        .map_or(0, <[watch_extras::CachedTimestampLine]>::len);
                    const COLLAPSED_LINES: usize = 3;
                    let needs_collapse = line_count > COLLAPSED_LINES || !chapters.is_empty();
                    let expanded =
                        self.watch.metadata_cache.description_expanded || !needs_collapse;
                    if let Some(lines) = lines.as_deref()
                        && !lines.is_empty()
                    {
                        if !stats.is_empty() {
                            ui.add_space(6.0);
                        }
                        let visible_lines = if expanded {
                            lines
                        } else {
                            &lines[..COLLAPSED_LINES.min(lines.len())]
                        };
                        self.render_rich_description_lines_ui(
                            ui,
                            &ctx,
                            visible_lines,
                            palette.description_fg,
                            13.0,
                        );
                    }
                    if expanded {
                        if !chapters.is_empty() {
                            ui.add_space(12.0);
                            Self::paint_hairline_divider(ui, palette);
                            ui.add_space(10.0);
                            self.render_description_chapter_carousel_ui(
                                ui, &ctx, video_url, &chapters, palette,
                            );
                        }
                        ui.add_space(12.0);
                        Self::paint_hairline_divider(ui, palette);
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(if pt {
                                "Transcri\u{00e7}\u{00e3}o"
                            } else {
                                "Transcript"
                            })
                            .size(14.0)
                            .strong()
                            .color(palette.channel_fg),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(if pt {
                                "Acompanhe usando a transcri\u{00e7}\u{00e3}o."
                            } else {
                                "Follow along using the transcript."
                            })
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                        );
                        ui.add_space(6.0);
                        let transcript_active = self.watch.extras.is_transcript_open();
                        if styled_pill_button(
                            ui,
                            if pt {
                                "Mostrar transcri\u{00e7}\u{00e3}o"
                            } else {
                                "Show transcript"
                            },
                            transcript_active,
                            palette,
                        )
                        .clicked()
                        {
                            open_transcript = true;
                        }
                    }
                    if needs_collapse {
                        ui.add_space(8.0);
                        let toggle_label = if expanded {
                            if pt { "Mostrar menos" } else { "Show less" }
                        } else if pt {
                            "...mais"
                        } else {
                            "...more"
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(toggle_label)
                                        .size(13.0)
                                        .strong()
                                        .color(palette.channel_fg),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            next_expanded = Some(!expanded);
                        }
                    }
                });
            if let Some(expanded) = next_expanded {
                self.watch.metadata_cache.description_expanded = expanded;
            }
            if open_transcript {
                self.open_transcript_panel(&ctx);
            }
            if self.watch.metadata_cache.video_url.as_deref() == Some(video_url) {
                self.watch.metadata_cache.description_lines = lines;
            }
        }
    }

    pub(crate) fn watch_video_bookmarks_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video_id: &str,
        palette: &ThemePalette,
    ) {
        let matching: Vec<(usize, library::Bookmark)> = self
            .library
            .data
            .bookmarks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.video_id == video_id)
            .map(|(idx, b)| (idx, b.clone()))
            .collect();
        if matching.is_empty() {
            return;
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new(
            RichText::new(format!("Marcadores deste vídeo ({})", matching.len()))
                .size(14.0)
                .strong(),
        )
        .id_salt(("watch_video_bookmarks", video_id))
        .default_open(true)
        .show(ui, |ui| {
            let mut seek_ms: Option<u64> = None;
            let mut remove_idx: Option<usize> = None;
            let mut save_edit: Option<(usize, String)> = None;
            ui.horizontal(|ui| {
                if styled_compact_button(ui, "Copiar marcadores (Markdown)", false, palette)
                    .on_hover_text("Copiar os marcadores deste vídeo em formato Markdown")
                    .clicked()
                {
                    let only_this: Vec<library::Bookmark> =
                        matching.iter().map(|(_, b)| b.clone()).collect();
                    let md = library::format_bookmarks_markdown(&only_this);
                    let _ = copy_text_to_clipboard(ctx, &md);
                    self.library.notice = Some("Marcadores copiados em formato Markdown.".into());
                }
            });
            ui.add_space(4.0);
            for (idx, mark) in &matching {
                let timestamp = format_time(mark.time_ms as f64 / 1000.0);
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        if self.library.editing_bookmark_idx == Some(*idx) {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&timestamp).small().strong());
                                ui.add_sized(
                                    [(ui.available_width() - 140.0).max(80.0), 26.0],
                                    egui::TextEdit::singleline(
                                        &mut self.library.editing_bookmark_note,
                                    )
                                    .hint_text("Nota do marcador"),
                                );
                                if styled_compact_button(ui, "Salvar", true, palette).clicked() {
                                    save_edit =
                                        Some((*idx, self.library.editing_bookmark_note.clone()));
                                }
                                if styled_ghost_button(ui, "Cancelar", palette).clicked() {
                                    self.library.editing_bookmark_idx = None;
                                    self.library.editing_bookmark_note.clear();
                                }
                            });
                        } else {
                            ui.horizontal(|ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("{timestamp}  {}", mark.note))
                                                .size(13.0)
                                                .strong()
                                                .color(palette.header_button_fg),
                                        )
                                        .frame(false)
                                        .wrap(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text("Ir para este marcador")
                                    .clicked()
                                {
                                    seek_ms = Some(mark.time_ms);
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if styled_icon_chip_button(ui, true, "✕", palette)
                                            .on_hover_text("Excluir marcador")
                                            .clicked()
                                        {
                                            remove_idx = Some(*idx);
                                        }
                                        if styled_compact_button(ui, "Editar", false, palette)
                                            .on_hover_text("Editar nota do marcador")
                                            .clicked()
                                        {
                                            self.library.editing_bookmark_idx = Some(*idx);
                                            self.library.editing_bookmark_note = mark.note.clone();
                                        }
                                    },
                                );
                            });
                        }
                    });
                ui.add_space(4.0);
            }
            if let Some(ms) = seek_ms {
                self.seek_to_seconds(ms as f64 / 1000.0);
            }
            if let Some((idx, note)) = save_edit {
                match self.library.data.update_bookmark_note(idx, &note) {
                    Ok(()) => {
                        self.library.editing_bookmark_idx = None;
                        self.library.editing_bookmark_note.clear();
                        self.save_library();
                    }
                    Err(err) => {
                        self.library.error = Some(err.to_owned());
                    }
                }
            }
            if let Some(idx) = remove_idx
                && self.library.data.remove_bookmark(idx).is_some()
            {
                if self.library.editing_bookmark_idx == Some(idx) {
                    self.library.editing_bookmark_idx = None;
                    self.library.editing_bookmark_note.clear();
                }
                self.save_library();
            }
        });
    }

    pub(crate) fn ensure_watch_metadata_cache(&mut self, video: &Video) {
        let is_live = self.is_video_known_live(video);
        let up_to_date = self.watch.metadata_cache.valid
            && self.watch.metadata_cache.video_url.as_deref() == Some(video.url.as_str())
            && self.watch.metadata_cache.metadata_revision == self.feed.metadata_revision
            && self.watch.metadata_cache.live_info_revision == self.watch.live.info_revision
            && self.watch.metadata_cache.community_revision == self.watch.community_revision
            && self.watch.metadata_cache.library_revision == self.library.revision
            && self.watch.metadata_cache.is_live == is_live;
        if up_to_date {
            return;
        }
        let saved_in_library = self.video_saved_in_library(video);
        let metadata = self.metadata_for(&video.url).cloned();
        let live_info = self
            .watch
            .live
            .info
            .as_ref()
            .filter(|_| self.watch.live.video_url.as_deref() == Some(video.url.as_str()))
            .cloned();
        let channel_target = metadata
            .as_ref()
            .and_then(search::channel_target_for_metadata)
            .or_else(|| {
                live_info.as_ref().and_then(|info| {
                    info.channel_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .map(|id| ChannelTarget::ChannelId(id.to_owned()))
                })
            });
        let channel_id_hint = metadata
            .as_ref()
            .and_then(|meta| meta.channel_id.clone())
            .or_else(|| live_info.as_ref().and_then(|info| info.channel_id.clone()));
        let channel_avatar_url = live_info
            .as_ref()
            .and_then(|info| info.channel_avatar_url.clone())
            .or_else(|| {
                metadata
                    .as_ref()
                    .and_then(|meta| meta.channel_avatar_url.clone())
            })
            .or_else(|| {
                let candidate_name = live_info
                    .as_ref()
                    .and_then(|info| info.channel.as_deref())
                    .or_else(|| {
                        (!video.channel.trim().is_empty()).then_some(video.channel.as_str())
                    })?;
                let key = recommendations::normalize_channel_key(candidate_name);
                self.feed.home.channel_avatars.get(&key).cloned()
            });
        let subscriber_count_text = live_info
            .as_ref()
            .and_then(|info| info.subscriber_count_text.clone())
            .or_else(|| {
                metadata
                    .as_ref()
                    .and_then(|meta| meta.subscriber_count_text.clone())
            });
        let stats = Self::format_watch_metadata_stats(
            video,
            metadata.as_ref(),
            live_info.as_ref(),
            is_live,
        );
        let cleaned_description = clean_description(
            metadata
                .as_ref()
                .and_then(|meta| meta.description.as_deref())
                .or_else(|| {
                    live_info
                        .as_ref()
                        .and_then(|info| info.description.as_deref())
                }),
        );
        let description_chapters = cleaned_description
            .as_deref()
            .map(watch_extras::extract_description_chapters)
            .unwrap_or_default();
        let preserve_expanded = self.watch.metadata_cache.video_url.as_deref()
            == Some(video.url.as_str())
            && self.watch.metadata_cache.description_expanded;
        self.watch.metadata_cache = app_state::WatchMetadataUiCache {
            valid: true,
            video_url: Some(video.url.clone()),
            metadata_revision: self.feed.metadata_revision,
            live_info_revision: self.watch.live.info_revision,
            community_revision: self.watch.community_revision,
            library_revision: self.library.revision,
            is_live,
            channel_target,
            channel_id_hint,
            channel_avatar_url,
            subscriber_count_text,
            saved_in_library,
            stats,
            cleaned_description,
            description_lines: None,
            description_chapters,
            description_expanded: preserve_expanded,
        };
    }

    pub(crate) fn watch_metadata_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        video: &Video,
        width: f32,
        resolution_badge: Option<&str>,
    ) {
        let ctx = ui.ctx().clone();
        self.ensure_watch_metadata_cache(video);
        let channel_target = self.watch.metadata_cache.channel_target.clone();
        let channel_id_hint = self.watch.metadata_cache.channel_id_hint.clone();
        let channel_avatar_url = self.watch.metadata_cache.channel_avatar_url.clone();
        let subscriber_count_text = self.watch.metadata_cache.subscriber_count_text.clone();
        let saved_in_library = self.watch.metadata_cache.saved_in_library;
        let stats = std::mem::take(&mut self.watch.metadata_cache.stats);
        let subscribed = if self.auth.authenticated {
            self.subscription_status_for(channel_target.as_ref(), channel_id_hint.as_deref())
                .map(|s| s.subscription_id.is_some())
        } else {
            Some(
                self.followed_channels
                    .data
                    .is_following_target(channel_target.as_ref(), channel_id_hint.as_deref()),
            )
        };
        let sub_pending = self.auth.authenticated && self.auth.sub_toggle_task.is_running();
        let palette = self.palette();
        let content_width = width.min(ui.available_width()).max(1.0);
        let save_menu_open = self.playback.save_menu_open;
        let video_id = history::youtube_video_id(&video.url);
        let current_rating = video_id
            .as_ref()
            .and_then(|id| self.auth.ratings.get(id).copied());
        let rating_pending = self.auth.rating_task.is_running();
        let ryd_stats = if self.settings.prefs.value.ryd_enabled {
            video_id
                .as_deref()
                .and_then(|id| self.watch.community_cache.get(id))
                .and_then(|c| c.ryd.clone())
        } else {
            None
        };
        let dearrow_title = if self.settings.prefs.value.dearrow_enabled {
            video_id
                .as_deref()
                .and_then(|id| self.watch.community_cache.get(id))
                .and_then(|c| c.dearrow.as_ref())
                .and_then(|d| d.clean_title.clone())
                .filter(|clean| !clean.trim().is_empty() && clean != &video.title)
        } else {
            None
        };
        let pt = self.ui_pt();
        let mut action_flags = WatchMetadataActionFlags::default();
        let mut open_channel_target = None;
        let mut sub_clicked = false;
        let technical_resolution = resolution_badge.map(str::to_owned);
        let channel_display = WatchChannelDisplay {
            channel_target: channel_target.as_ref(),
            channel_avatar_url: channel_avatar_url.as_deref(),
            subscriber_count_text: subscriber_count_text.as_deref(),
            content_width,
            stats: &stats,
            technical_resolution: technical_resolution.as_deref(),
            palette: &palette,
            subscribed,
            sub_pending,
        };
        let action_state = WatchMetadataActionState {
            video_id: video_id.as_deref(),
            current_rating,
            rating_pending,
            saved_in_library,
            save_menu_open,
            share_recently_copied: self
                .watch
                .share_copied_at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(2)),
            ryd_stats: ryd_stats.as_ref(),
            pt,
            palette: &palette,
        };

        ui.add_space(12.0);
        ui.scope(|ui| {
            ui.set_min_width(content_width);
            ui.set_max_width(content_width);
            let display_title = match dearrow_title.as_deref() {
                Some(clean) if !self.watch.show_original_title => clean,
                _ => video.title.as_str(),
            };
            ui.label(
                RichText::new(display_title)
                    .size(if content_width < 620.0 { 18.0 } else { 20.0 })
                    .strong()
                    .color(palette.channel_fg),
            );
            if dearrow_title.is_some() {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let toggle_label = if self.watch.show_original_title {
                        if pt {
                            "DeArrow ativo · Mostrar título limpo"
                        } else {
                            "DeArrow · Show clean title"
                        }
                    } else if pt {
                        "Título DeArrow · Ver título original"
                    } else {
                        "DeArrow title · Show original title"
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(toggle_label)
                                    .size(11.5)
                                    .color(palette.neutral_status_fg),
                            )
                            .frame(false),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.watch.show_original_title = !self.watch.show_original_title;
                    }
                });
            }

            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);
                let (target, sub) =
                    self.watch_metadata_channel_row_ui(ui, &ctx, video, &channel_display);
                if let Some(target) = target {
                    open_channel_target = Some(target);
                }
                if sub {
                    sub_clicked = true;
                }

                self.watch_metadata_actions_ui(ui, &action_state, &mut action_flags);
            });

            if save_menu_open {
                ui.add_space(8.0);
                self.save_menu_ui(ui, video, &ctx);
            }

            self.watch_metadata_description_with_stats_ui(
                ui,
                &video.url,
                content_width,
                &stats,
                technical_resolution.as_deref(),
                &palette,
            );
            if let Some(id) = video_id.as_deref() {
                self.watch_video_bookmarks_ui(ui, &ctx, id, &palette);
            }

            if !self.watch.extras.comments_loaded
                && self.watch.extras.comments_loading.is_none()
                && !self.watch.extras.innertube_comments_loading
                && self.watch.extras.comments_error.is_none()
            {
                if let Some(continuation) =
                    self.watch.extras.innertube_comments_continuation.clone()
                {
                    self.start_load_innertube_comments(continuation, true, &ctx);
                } else if self
                    .auth
                    .session
                    .as_ref()
                    .is_some_and(oauth::TokenSession::has_comment_scope)
                {
                    self.watch
                        .extras
                        .start_comments(self.auth.session.as_ref(), false, &ctx);
                }
            }

            ui.add_space(14.0);
            Self::paint_hairline_divider(ui, &palette);
            ui.add_space(12.0);
            self.comments_ui(ui, &ctx, "watch_primary_comments");
            ui.add_space(12.0);
        });
        if self.watch.metadata_cache.video_url.as_deref() == Some(video.url.as_str()) {
            self.watch.metadata_cache.stats = stats;
        }

        if let (Some(video_id), Some(rating)) = (video_id, action_flags.rating_to_apply) {
            self.start_video_rating(video_id, rating, &ctx);
        }
        if sub_clicked {
            if self.auth.authenticated {
                if let Some(target) = channel_target.clone() {
                    self.start_toggle_subscription(target, channel_id_hint.clone(), &ctx);
                }
            } else {
                let cid = channel_id_hint.clone().or_else(|| {
                    channel_target.as_ref().map(|t| match t {
                        ChannelTarget::ChannelId(id) | ChannelTarget::Handle(id) => id.clone(),
                    })
                });
                if let Some(cid) = cid {
                    let handle = channel_target.as_ref().and_then(|t| match t {
                        ChannelTarget::Handle(h) => Some(h.clone()),
                        ChannelTarget::ChannelId(_) => None,
                    });
                    let url = channel_target
                        .as_ref()
                        .map(ChannelTarget::canonical_url)
                        .unwrap_or_else(|| format!("https://www.youtube.com/channel/{cid}"));
                    let title = if video.channel.trim().is_empty() {
                        cid.clone()
                    } else {
                        video.channel.clone()
                    };
                    self.toggle_local_follow_channel(cid, title, handle, url, channel_avatar_url);
                }
            }
        }
        if action_flags.share_video {
            self.open_share_modal(video);
        }
        if action_flags.toggle_save_menu {
            self.playback.save_menu_open = !self.playback.save_menu_open;
            if self.playback.save_menu_open
                && self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
            {
                self.start_fetch_remote_playlists(&ctx);
            }
        }
        if action_flags.enqueue_video {
            let _ = self.enqueue_video(video.clone());
        }
        if action_flags.open_transcript {
            self.open_transcript_panel(&ctx);
        }
        if action_flags.open_report_modal {
            self.watch.extras.report_reason.clear();
            self.watch.extras.report_details.clear();
            self.watch.extras.report_modal_open = true;
        }
        if let Some(format) = action_flags.download_format {
            self.start_download_video(video.clone(), format, &ctx);
        }
        if let Some(target) = open_channel_target {
            self.open_channel(target, &ctx);
        }
    }

    pub(crate) fn watch_report_modal_ui(&mut self, ctx: &egui::Context) {
        if !self.watch.extras.report_modal_open {
            return;
        }
        let pass_id = egui::Id::new("watch_report_modal_rendered_pass");
        let cur_pass = ctx.cumulative_pass_nr();
        if ctx.data(|d| d.get_temp::<u64>(pass_id)) == Some(cur_pass) {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(pass_id, cur_pass));

        let pt = self.ui_pt();
        let palette = self.palette();
        let reasons: [(&str, &str); 7] = [
            ("Conte\u{00fa}do sexual", "Sexual content"),
            (
                "Conte\u{00fa}do violento ou repulsivo",
                "Violent or repulsive content",
            ),
            (
                "Conte\u{00fa}do de incita\u{00e7}\u{00e3}o ao \u{00f3}dio ou abusivo",
                "Hateful or abusive content",
            ),
            ("Ass\u{00e9}dio ou bullying", "Harassment or bullying"),
            ("Atos perigosos ou nocivos", "Harmful or dangerous acts"),
            ("Desinforma\u{00e7}\u{00e3}o", "Misinformation"),
            ("Spam ou enganoso", "Spam or misleading"),
        ];

        let mut submit_clicked = false;
        let mut cancel_clicked = false;
        let title = if pt {
            "Denunciar conte\u{00fa}do"
        } else {
            "Report content"
        };

        let mut modal_open = self.watch.extras.report_modal_open;
        app_modal_window(
            ctx,
            egui::Id::new("watch_report_modal"),
            title,
            &mut modal_open,
            460.0,
            |ui| {
                ui.label(
                    RichText::new(if pt {
                        "Selecione o motivo da den\u{00fa}ncia:"
                    } else {
                        "Select a reason for reporting:"
                    })
                    .size(13.0)
                    .color(palette.neutral_status_fg),
                );
                ui.add_space(8.0);
                for (label_pt, label_en) in reasons {
                    let label = if pt { label_pt } else { label_en };
                    let selected = self.watch.extras.report_reason == label;
                    if ui
                        .radio(
                            selected,
                            RichText::new(label).size(13.5).color(palette.channel_fg),
                        )
                        .clicked()
                    {
                        self.watch.extras.report_reason = label.to_owned();
                    }
                }
                ui.add_space(10.0);
                ui.label(
                    RichText::new(if pt {
                        "Detalhes adicionais (opcional)"
                    } else {
                        "Additional details (optional)"
                    })
                    .size(12.5)
                    .color(palette.neutral_status_fg),
                );
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::multiline(&mut self.watch.extras.report_details)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(if pt {
                            "Descreva o problema ou inclua carimbos de tempo..."
                        } else {
                            "Describe the issue or include timestamps..."
                        }),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_submit = !self.watch.extras.report_reason.trim().is_empty();
                        if ui
                            .add_enabled_ui(can_submit, |ui| {
                                styled_pill_button(
                                    ui,
                                    if pt { "Denunciar" } else { "Report" },
                                    can_submit,
                                    &palette,
                                )
                            })
                            .inner
                            .clicked()
                        {
                            submit_clicked = true;
                        }
                        if styled_ghost_button(ui, if pt { "Cancelar" } else { "Cancel" }, &palette)
                            .clicked()
                        {
                            cancel_clicked = true;
                        }
                    });
                });
            },
        );

        if submit_clicked {
            let reason = std::mem::take(&mut self.watch.extras.report_reason);
            let details = std::mem::take(&mut self.watch.extras.report_details);
            self.watch.extras.report_modal_open = false;
            self.submit_video_report(reason, details, ctx);
        } else if cancel_clicked || !modal_open {
            self.watch.extras.report_modal_open = false;
        }
    }

    pub(crate) fn watch_sidebar_tabs_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &ThemePalette,
    ) {
        let pt = self.ui_pt();
        let current_channel = self
            .selected
            .as_ref()
            .map(|v| v.channel.trim())
            .filter(|ch| !ch.is_empty())
            .map(|ch| {
                format!(
                    "{} {}",
                    if pt { "De" } else { "From" },
                    truncate_text_chars(ch, 20)
                )
            })
            .unwrap_or_else(|| {
                if pt {
                    "Deste canal"
                } else {
                    "From this channel"
                }
                .to_owned()
            });

        let has_innertube_chips = !self.watch.extras.related_chips.is_empty();
        let mut clicked_tab: Option<watch_extras::SidebarTab> = None;
        let mut clicked_related_chip: Option<(Option<usize>, Option<String>)> = None;

        egui::ScrollArea::horizontal()
            .id_salt("watch_sidebar_filter_chips")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                    let render_pill = |ui: &mut egui::Ui, label: &str, is_selected: bool| -> bool {
                        let (bg, fg, stroke_col) = if is_selected {
                            (
                                palette.active_button_bg,
                                palette.active_button_fg,
                                palette.active_button_bg,
                            )
                        } else {
                            (
                                palette.header_button_bg,
                                palette.header_button_fg,
                                palette.card_alt_stroke,
                            )
                        };
                        let text = if is_selected {
                            RichText::new(label).size(13.0).strong().color(fg)
                        } else {
                            RichText::new(label).size(13.0).color(fg)
                        };
                        let btn = egui::Button::new(text)
                            .fill(bg)
                            .stroke(egui::Stroke::new(1.0, stroke_col))
                            .corner_radius(8);
                        ui.add_sized([0.0, 32.0], btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                    };

                    if has_innertube_chips {
                        let all_label = if pt { "Todos" } else { "All" };
                        let first_is_all =
                            self.watch.extras.related_chips.first().is_some_and(|c| {
                                let lower = c.title.trim().to_lowercase();
                                matches!(lower.as_str(), "all" | "todos" | "tudo")
                            });
                        if !first_is_all {
                            let all_selected = !self.channels.view_open
                                && self.watch.extras.sidebar_tab == watch_extras::SidebarTab::All
                                && self.watch.extras.selected_related_chip.is_none();
                            if render_pill(ui, all_label, all_selected) {
                                clicked_tab = Some(watch_extras::SidebarTab::All);
                                clicked_related_chip = Some((None, None));
                            }
                        }

                        let from_ch_selected = !self.channels.view_open
                            && self.watch.extras.sidebar_tab
                                == watch_extras::SidebarTab::FromChannel;
                        if render_pill(ui, &current_channel, from_ch_selected) {
                            clicked_tab = Some(watch_extras::SidebarTab::FromChannel);
                        }

                        for (idx, chip) in self.watch.extras.related_chips.iter().enumerate() {
                            let title = chip.title.trim();
                            if title.is_empty() {
                                continue;
                            }
                            let is_selected = !self.channels.view_open
                                && self.watch.extras.sidebar_tab
                                    != watch_extras::SidebarTab::FromChannel
                                && (self.watch.extras.selected_related_chip == Some(idx)
                                    || (first_is_all
                                        && self.watch.extras.selected_related_chip.is_none()
                                        && idx == 0));
                            if render_pill(ui, title, is_selected) {
                                clicked_tab = Some(watch_extras::SidebarTab::All);
                                clicked_related_chip = Some((Some(idx), chip.continuation.clone()));
                            }
                        }
                    } else {
                        let tabs = [
                            (
                                if pt { "Todos" } else { "All" }.to_owned(),
                                watch_extras::SidebarTab::All,
                            ),
                            (current_channel, watch_extras::SidebarTab::FromChannel),
                            (
                                if pt { "Relacionados" } else { "Related" }.to_owned(),
                                watch_extras::SidebarTab::Related,
                            ),
                            (
                                if pt { "Para voc\u{00ea}" } else { "For you" }.to_owned(),
                                watch_extras::SidebarTab::ForYou,
                            ),
                        ];
                        for (label, tab) in &tabs {
                            let is_selected =
                                self.watch.extras.sidebar_tab == *tab && !self.channels.view_open;
                            if render_pill(ui, label, is_selected) {
                                clicked_tab = Some(*tab);
                            }
                        }
                    }
                });
            });

        if let Some(tab) = clicked_tab {
            self.channels.view_open = false;
            self.watch.extras.sidebar_tab = tab;
            self.watch.extras.tab = watch_extras::Tab::Results;
            if matches!(tab, watch_extras::SidebarTab::Transcript) {
                self.open_transcript_panel(ctx);
            }
        }
        if let Some((chip_idx, continuation)) = clicked_related_chip {
            self.select_watch_related_chip(chip_idx, continuation, ctx);
        }
    }

    pub(crate) fn ensure_watch_sidebar_cache(&mut self) {
        let feed_first_url = self.feed.videos.first().map(|v| v.url.as_str());
        let channel_target_key = self
            .channels
            .page
            .as_ref()
            .map(|p| p.canonical_url.as_str());
        let channel_videos_len = self.channels.page.as_ref().map_or(0, |p| p.videos.len());
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str());
        let related_key = (
            self.watch.extras.related_videos.len(),
            self.watch
                .extras
                .related_videos
                .first()
                .map(|v| v.url.clone()),
            self.watch.extras.selected_related_chip,
        );

        let up_to_date = self.watch.sidebar_cache.valid
            && self.watch.sidebar_cache.feed_generation == self.feed.generation
            && self.watch.sidebar_cache.feed_signal_revision == self.feed.signal_revision
            && self.watch.sidebar_cache.metadata_revision == self.feed.metadata_revision
            && self.watch.sidebar_cache.feed_videos_len == self.feed.videos.len()
            && self.watch.sidebar_cache.feed_first_url.as_deref() == feed_first_url
            && self.watch.sidebar_cache.channel_target_key.as_deref() == channel_target_key
            && self.watch.sidebar_cache.channel_videos_len == channel_videos_len
            && self.watch.sidebar_cache.selected_url.as_deref() == selected_url
            && self.watch.sidebar_cache.sidebar_tab == self.watch.extras.sidebar_tab
            && self.watch.sidebar_cache.playlist_urls_len == self.feed.home.playlist_urls.len()
            && self.watch.extras.cached_sidebar_related_key.as_ref() == Some(&related_key);
        if up_to_date {
            return;
        }
        self.watch.extras.cached_sidebar_related_key = Some(related_key);

        let mut previous_heights: HashMap<String, (i32, f32)> = self
            .watch
            .sidebar_cache
            .cards
            .drain(..)
            .filter_map(|card| card.measured_height.map(|h| (card.video.url, h)))
            .collect();

        let related_meta_map: HashMap<&str, &VideoMetadata> = self
            .watch
            .extras
            .related_videos
            .iter()
            .zip(self.watch.extras.related_metadata.iter())
            .map(|(v, m)| (v.url.as_str(), m))
            .collect();
        let lookup_meta = |url: &str| -> Option<&VideoMetadata> {
            self.metadata_for(url)
                .or_else(|| related_meta_map.get(url).copied())
        };

        let is_from_channel = matches!(
            self.watch.extras.sidebar_tab,
            watch_extras::SidebarTab::FromChannel
        );
        let has_real_related = !self.watch.extras.related_videos.is_empty();
        let extra_cap = self.channels.page.as_ref().map_or(0, |p| p.videos.len())
            + self.watch.extras.related_videos.len();
        let mut seen_urls = HashSet::with_capacity(self.feed.videos.len() + extra_cap);
        let mut candidate_videos: Vec<&Video> =
            Vec::with_capacity(self.feed.videos.len() + extra_cap);

        let current_vid_id = self
            .selected
            .as_ref()
            .and_then(|v| history::youtube_video_id(&v.url));
        for video in &self.watch.extras.related_videos {
            let is_current = selected_url == Some(video.url.as_str())
                || (current_vid_id.is_some()
                    && history::youtube_video_id(&video.url) == current_vid_id);
            if !is_current && seen_urls.insert(video.url.as_str()) {
                candidate_videos.push(video);
            }
        }
        let prioritized_related_len = candidate_videos.len();

        if prioritized_related_len == 0 || is_from_channel {
            for video in &self.feed.videos {
                if seen_urls.insert(video.url.as_str()) {
                    candidate_videos.push(video);
                }
            }
            if let Some(page) = &self.channels.page {
                for ch_item in &page.videos {
                    if seen_urls.insert(ch_item.video.url.as_str()) {
                        candidate_videos.push(&ch_item.video);
                    }
                }
            }
        }

        if is_from_channel {
            if let Some(current_video) = self.selected.as_ref() {
                let current_meta = lookup_meta(&current_video.url);
                let live_info = self.watch.live.info.as_ref().filter(|_| {
                    self.watch.live.video_url.as_deref() == Some(current_video.url.as_str())
                });
                let current_channel_id_hint = live_info.and_then(|info| info.channel_id.as_deref());
                let current_channel_name_hint = live_info.and_then(|info| info.channel.as_deref());
                candidate_videos.retain(|video| {
                    let candidate_meta = lookup_meta(&video.url);
                    sidebar_video_matches_current_channel(
                        current_video,
                        current_meta,
                        current_channel_id_hint,
                        current_channel_name_hint,
                        video,
                        candidate_meta,
                    )
                });
            } else {
                candidate_videos.clear();
            }
        }

        let mut cards = Vec::new();
        if !candidate_videos.is_empty() {
            let ordered_indices: Vec<usize> =
                if has_real_related && !is_from_channel && prioritized_related_len > 0 {
                    (0..prioritized_related_len).collect()
                } else {
                    let current_video = self.selected.as_ref();
                    let current_meta = current_video.and_then(|v| lookup_meta(&v.url));
                    let profile =
                        UserProfile::build(&self.history.data, &self.queue.data, current_video);
                    let rank_tab = if is_from_channel {
                        // `candidate_videos` is already filtered locally to the active channel,
                        // preserving channel-ID matches even when display names differ.
                        watch_extras::SidebarTab::All
                    } else {
                        self.watch.extras.sidebar_tab
                    };
                    let scored = RecommendationEngine::rank_sidebar_video_indices(
                        &candidate_videos,
                        &self.video_metadata,
                        &profile,
                        current_video,
                        current_meta,
                        rank_tab,
                    );
                    if scored.is_empty() {
                        if is_from_channel {
                            Vec::new()
                        } else {
                            (0..candidate_videos.len()).collect()
                        }
                    } else {
                        scored
                            .into_iter()
                            .map(|(orig_idx, _, _)| orig_idx)
                            .collect()
                    }
                };
            let now_epoch = recommendations::current_unix_epoch();
            cards.reserve(ordered_indices.len());
            for orig_idx in ordered_indices {
                let video = candidate_videos[orig_idx];
                let metadata = lookup_meta(&video.url);
                let is_playlist = self.feed.home.playlist_urls.contains(&video.url);
                let channel_target = metadata.and_then(search::channel_target_for_metadata);
                let history_entry =
                    history::youtube_video_id(&video.url).and_then(|id| self.history.data.get(&id));
                let in_history = history_entry.is_some();
                let progress_ratio = history_entry.and_then(|entry| {
                    let duration_ms = entry.duration_ms? as f32;
                    if duration_ms <= 0.0 || entry.position_ms == 0 {
                        None
                    } else {
                        Some((entry.position_ms as f32 / duration_ms).clamp(0.04, 1.0))
                    }
                });
                let is_music = recommendations::is_music_video(video, metadata);
                let is_new = recommendations::is_recent_upload(metadata, now_epoch);
                let title_truncated = truncate_text_chars(&video.title, 74);
                let channel_label = format_sidebar_channel_label(&video.channel, metadata);
                let subline = format_youtube_card_subline(&video.duration, metadata);
                let measured_height = previous_heights.remove(&video.url);
                cards.push(app_state::CachedSidebarCard {
                    video: video.clone(),
                    is_playlist,
                    channel_target,
                    progress_ratio,
                    in_history,
                    is_music,
                    is_new,
                    title_truncated,
                    channel_label,
                    subline,
                    measured_height,
                });
            }
        }

        self.watch.sidebar_cache = app_state::WatchSidebarCache {
            valid: true,
            feed_generation: self.feed.generation,
            feed_signal_revision: self.feed.signal_revision,
            metadata_revision: self.feed.metadata_revision,
            feed_videos_len: self.feed.videos.len(),
            feed_first_url: feed_first_url.map(str::to_owned),
            channel_target_key: channel_target_key.map(str::to_owned),
            channel_videos_len,
            selected_url: selected_url.map(str::to_owned),
            sidebar_tab: self.watch.extras.sidebar_tab,
            playlist_urls_len: self.feed.home.playlist_urls.len(),
            cards,
        };
    }

    pub(crate) fn watch_sidebar_feed_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
    ) -> Option<Video> {
        self.ensure_watch_sidebar_cache();
        if self.watch.sidebar_cache.cards.is_empty() {
            let loading_feed = self.feed.search.task.is_running()
                || self.feed.home.batch_task.is_running()
                || self.feed.home.stream_task.is_running();
            if loading_feed {
                let dark_theme = self.is_dark_theme();
                skeleton::watch_sidebar_skeleton(ui, ui.available_width(), dark_theme);
            } else if matches!(
                self.watch.extras.sidebar_tab,
                watch_extras::SidebarTab::FromChannel
            ) {
                let palette = self.palette();
                let empty_text = self.pick(
                    "Nenhum outro v\u{00ed}deo deste canal carregado na lista atual.",
                    "No other videos from this channel in the current list.",
                );
                let show_all_label = self.pick("Mostrar todos", "Show all");
                let open_channel_label = self.pick("Abrir canal", "Open channel");
                let channel_target = self.selected.as_ref().and_then(|video| {
                    self.metadata_for(&video.url)
                        .and_then(search::channel_target_for_metadata)
                        .or_else(|| {
                            (self.watch.metadata_cache.valid
                                && self.watch.metadata_cache.video_url.as_deref()
                                    == Some(video.url.as_str()))
                            .then(|| self.watch.metadata_cache.channel_target.clone())
                            .flatten()
                        })
                });
                let mut switch_to_all = false;
                let mut open_target: Option<ChannelTarget> = None;
                ui.label(RichText::new(empty_text).weak());
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if styled_compact_button(ui, show_all_label, false, &palette).clicked() {
                        switch_to_all = true;
                    }
                    if let Some(target) = &channel_target
                        && styled_compact_button(ui, open_channel_label, false, &palette).clicked()
                    {
                        open_target = Some(target.clone());
                    }
                });
                if switch_to_all {
                    self.watch.extras.sidebar_tab = watch_extras::SidebarTab::All;
                }
                if let Some(target) = open_target {
                    self.open_channel(target, ctx);
                }
            } else {
                let empty_text = self.pick(
                    "Nenhuma recomenda\u{00e7}\u{00e3}o no momento.",
                    "No recommendations right now.",
                );
                ui.label(RichText::new(empty_text).weak());
                if !self.history.data.entries().is_empty() {
                    ui.add_space(10.0);
                    return self.history_section_ui(
                        ui,
                        &format!("{scroll_id}_history"),
                        true,
                        None,
                        None,
                    );
                }
            }
            return None;
        }

        let palette = self.palette();
        let cache_generation = self.feed.generation;
        let cache_signal = self.feed.signal_revision;
        let mut cards = std::mem::take(&mut self.watch.sidebar_cache.cards);
        let mut chosen = None;
        let mut playlist_to_open: Option<String> = None;
        let mut channel_to_open: Option<ChannelTarget> = None;

        self.thumbnails.begin_pass();
        let target_width = ui.available_width();
        let width_key = target_width.round() as i32;
        let visible_rect = ui
            .clip_rect()
            .expand2(egui::vec2(0.0, thumbnails::VIEWPORT_OVERSCAN_PX + 220.0));
        for (index, card) in cards.iter_mut().enumerate() {
            let cursor_top = ui.cursor().top();
            let offscreen_height = card.measured_height.and_then(|(w_key, height)| {
                (w_key == width_key
                    && height > 0.0
                    && (cursor_top + height < visible_rect.top()
                        || cursor_top > visible_rect.bottom()))
                .then_some(height)
            });
            if let Some(height) = offscreen_height {
                ui.allocate_exact_size(
                    egui::vec2(target_width.max(1.0), height),
                    egui::Sense::hover(),
                );
            } else {
                let resp = ui.push_id((scroll_id, index), |ui| {
                    ui.scope(|ui| {
                        ui.set_max_width(target_width);
                        Self::recommendation_card_contents_ui(
                            &mut self.thumbnails,
                            ui,
                            ctx,
                            &palette,
                            &card.video,
                            true,
                            card.progress_ratio,
                            card.in_history,
                            card.is_music,
                            card.is_new,
                            &card.title_truncated,
                            card.channel_label.as_deref(),
                            card.subline.as_deref(),
                        )
                    })
                });
                card.measured_height = Some((width_key, resp.inner.response.rect.height()));
                let (clicked, channel_clicked) = resp.inner.inner;
                if clicked {
                    if card.is_playlist {
                        playlist_to_open = Some(card.video.url.clone());
                    } else {
                        chosen = Some(card.video.clone());
                    }
                }
                if channel_clicked && let Some(target) = &card.channel_target {
                    channel_to_open = Some(target.clone());
                }
            }
            ui.add_space(8.0);
        }
        self.thumbnails.end_pass(ctx);

        if self.watch.sidebar_cache.valid
            && self.watch.sidebar_cache.feed_generation == cache_generation
            && self.watch.sidebar_cache.feed_signal_revision == cache_signal
        {
            self.watch.sidebar_cache.cards = cards;
        }
        if let Some(url) = playlist_to_open {
            self.start_browser_feed(url, false, ctx);
        }
        if let Some(target) = channel_to_open {
            self.open_channel(target, ctx);
        }
        chosen
    }

    pub(crate) fn watch_sidebar_playlist_panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &ThemePalette,
        scroll_id: &str,
    ) {
        let Some(mut pl) = self.watch.active_playlist.take() else {
            return;
        };
        let pt = self.ui_pt();
        let mut close_panel = false;
        let mut selected_pos: Option<usize> = None;
        let mut add_all_to_queue = false;
        let mut open_full_playlist = false;

        let total_display = pl.total_display_count().max(1);
        let cur_num = (pl.current_pos + 1).min(total_display);
        let progress_ratio_str = format!("{cur_num} / {total_display}");
        let pl_id = pl.playlist_id.clone();
        let seed_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            .max(1);

        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(12.0)
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if pl.collapsed {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_max_width((ui.available_width() - 62.0).max(120.0));
                                    let next_hint = pl
                                        .peek_next()
                                        .map(|(_, v)| v.title)
                                        .or_else(|| pl.current_video().map(|v| v.title.clone()))
                                        .unwrap_or_default();
                                    let top_label = if pt {
                                        format!("Próximo: {next_hint}")
                                    } else {
                                        format!("Next: {next_hint}")
                                    };
                                    ui.label(
                                        RichText::new(truncate_text_chars(&top_label, 42))
                                            .size(13.5)
                                            .strong()
                                            .color(palette.channel_fg),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {progress_ratio_str}",
                                            truncate_text_chars(&pl.title, 30)
                                        ))
                                        .size(12.0)
                                        .color(palette.neutral_status_fg),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let (close_rect, close_resp) = ui.allocate_exact_size(
                                            egui::vec2(26.0, 26.0),
                                            egui::Sense::click(),
                                        );
                                        if close_resp.hovered() {
                                            ui.painter().rect_filled(
                                                close_rect,
                                                13.0,
                                                palette.card_alt_stroke.gamma_multiply(0.45),
                                            );
                                        }
                                        paint_close_x_icon(ui, close_rect, palette.icon_fg);
                                        if close_resp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(if pt {
                                                "Fechar playlist"
                                            } else {
                                                "Close playlist"
                                            })
                                            .clicked()
                                        {
                                            close_panel = true;
                                        }

                                        let (chev_rect, chev_resp) = ui.allocate_exact_size(
                                            egui::vec2(26.0, 26.0),
                                            egui::Sense::click(),
                                        );
                                        if chev_resp.hovered() {
                                            ui.painter().rect_filled(
                                                chev_rect,
                                                13.0,
                                                palette.card_alt_stroke.gamma_multiply(0.45),
                                            );
                                        }
                                        paint_chevron_down_icon(
                                            ui,
                                            chev_rect,
                                            palette.icon_fg,
                                            false,
                                        );
                                        if chev_resp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(if pt {
                                                "Expandir playlist"
                                            } else {
                                                "Expand playlist"
                                            })
                                            .clicked()
                                        {
                                            pl.collapsed = false;
                                        }
                                    },
                                );
                            });
                        });
                } else {
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 14,
                            right: 14,
                            top: 12,
                            bottom: 8,
                        })
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_max_width((ui.available_width() - 62.0).max(120.0));
                                    ui.label(
                                        RichText::new(truncate_text_chars(&pl.title, 44))
                                            .size(17.0)
                                            .strong()
                                            .color(palette.channel_fg),
                                    )
                                    .on_hover_text(&pl.title);
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::TOP),
                                    |ui| {
                                        let (close_rect, close_resp) = ui.allocate_exact_size(
                                            egui::vec2(26.0, 26.0),
                                            egui::Sense::click(),
                                        );
                                        if close_resp.hovered() {
                                            ui.painter().rect_filled(
                                                close_rect,
                                                13.0,
                                                palette.card_alt_stroke.gamma_multiply(0.45),
                                            );
                                        }
                                        paint_close_x_icon(ui, close_rect, palette.icon_fg);
                                        if close_resp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(if pt {
                                                "Fechar playlist"
                                            } else {
                                                "Close playlist"
                                            })
                                            .clicked()
                                        {
                                            close_panel = true;
                                        }

                                        let (chev_rect, chev_resp) = ui.allocate_exact_size(
                                            egui::vec2(26.0, 26.0),
                                            egui::Sense::click(),
                                        );
                                        if chev_resp.hovered() {
                                            ui.painter().rect_filled(
                                                chev_rect,
                                                13.0,
                                                palette.card_alt_stroke.gamma_multiply(0.45),
                                            );
                                        }
                                        paint_chevron_down_icon(
                                            ui,
                                            chev_rect,
                                            palette.icon_fg,
                                            true,
                                        );
                                        if chev_resp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(if pt {
                                                "Recolher playlist"
                                            } else {
                                                "Collapse playlist"
                                            })
                                            .clicked()
                                        {
                                            pl.collapsed = true;
                                        }
                                    },
                                );
                            });

                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                match pl.privacy {
                                    playlist_sync::PlaylistPrivacy::Private => {
                                        let (icon_r, _) = ui.allocate_exact_size(
                                            egui::vec2(14.0, 16.0),
                                            egui::Sense::hover(),
                                        );
                                        paint_lock_icon(ui, icon_r, palette.neutral_status_fg);
                                        ui.label(
                                            RichText::new(pl.privacy.label(pt))
                                                .size(12.0)
                                                .color(palette.neutral_status_fg),
                                        );
                                    }
                                    playlist_sync::PlaylistPrivacy::Unlisted => {
                                        let (icon_r, _) = ui.allocate_exact_size(
                                            egui::vec2(16.0, 16.0),
                                            egui::Sense::hover(),
                                        );
                                        paint_unlisted_link_icon(
                                            ui,
                                            icon_r,
                                            palette.neutral_status_fg,
                                        );
                                        ui.label(
                                            RichText::new(pl.privacy.label(pt))
                                                .size(12.0)
                                                .color(palette.neutral_status_fg),
                                        );
                                    }
                                    playlist_sync::PlaylistPrivacy::Public
                                    | playlist_sync::PlaylistPrivacy::Unknown => {}
                                }
                                if !pl.owner.trim().is_empty() {
                                    ui.label(
                                        RichText::new(truncate_text_chars(&pl.owner, 26))
                                            .size(12.0)
                                            .color(palette.channel_fg),
                                    );
                                }
                                ui.label(
                                    RichText::new(format!("- {progress_ratio_str}"))
                                        .size(12.0)
                                        .color(palette.neutral_status_fg),
                                );
                            });

                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let (loop_r, loop_resp) = ui.allocate_exact_size(
                                    egui::vec2(32.0, 28.0),
                                    egui::Sense::click(),
                                );
                                if pl.loop_playlist {
                                    ui.painter().rect_filled(
                                        loop_r,
                                        14.0,
                                        palette.active_button_bg,
                                    );
                                } else if loop_resp.hovered() {
                                    ui.painter().rect_filled(
                                        loop_r,
                                        14.0,
                                        palette.card_alt_stroke.gamma_multiply(0.45),
                                    );
                                }
                                paint_playlist_loop_icon(
                                    ui,
                                    loop_r,
                                    if pl.loop_playlist {
                                        palette.active_button_fg
                                    } else {
                                        palette.icon_fg
                                    },
                                );
                                if loop_resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(if pt {
                                        "Repetir playlist"
                                    } else {
                                        "Loop playlist"
                                    })
                                    .clicked()
                                {
                                    pl.toggle_loop();
                                }

                                let (shuf_r, shuf_resp) = ui.allocate_exact_size(
                                    egui::vec2(32.0, 28.0),
                                    egui::Sense::click(),
                                );
                                if pl.shuffle {
                                    ui.painter().rect_filled(
                                        shuf_r,
                                        14.0,
                                        palette.active_button_bg,
                                    );
                                } else if shuf_resp.hovered() {
                                    ui.painter().rect_filled(
                                        shuf_r,
                                        14.0,
                                        palette.card_alt_stroke.gamma_multiply(0.45),
                                    );
                                }
                                paint_playlist_shuffle_icon(
                                    ui,
                                    shuf_r,
                                    if pl.shuffle {
                                        palette.active_button_fg
                                    } else {
                                        palette.icon_fg
                                    },
                                );
                                if shuf_resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(if pt {
                                        "Ordem aleatória"
                                    } else {
                                        "Shuffle playlist"
                                    })
                                    .clicked()
                                {
                                    pl.toggle_shuffle(seed_time);
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let menu_id =
                                            ui.make_persistent_id(("watch_pl_menu", &pl_id));
                                        let dots = paint_more_dots_button(ui, palette);
                                        if dots.clicked() {
                                            egui::Popup::toggle_id(ui.ctx(), menu_id);
                                        }
                                        egui::Popup::from_toggle_button_response(&dots)
                                            .id(menu_id)
                                            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                                            .show(|ui| {
                                                ui.set_min_width(195.0);
                                                if search_menu_item_ui(
                                                    ui,
                                                    SearchMenuIcon::Queue,
                                                    if pt {
                                                        "Adicionar tudo à fila"
                                                    } else {
                                                        "Add all to queue"
                                                    },
                                                    palette,
                                                )
                                                .clicked()
                                                {
                                                    add_all_to_queue = true;
                                                }
                                                if !pl.is_mix
                                                    && search_menu_item_ui(
                                                        ui,
                                                        SearchMenuIcon::Queue,
                                                        if pt {
                                                            "Ver playlist completa"
                                                        } else {
                                                            "View full playlist"
                                                        },
                                                        palette,
                                                    )
                                                    .clicked()
                                                {
                                                    open_full_playlist = true;
                                                }
                                                ui.separator();
                                                if search_menu_item_ui(
                                                    ui,
                                                    SearchMenuIcon::Trash,
                                                    if pt {
                                                        "Fechar playlist"
                                                    } else {
                                                        "Close playlist"
                                                    },
                                                    palette,
                                                )
                                                .clicked()
                                                {
                                                    close_panel = true;
                                                }
                                            });
                                    },
                                );
                            });
                        });

                    let (hair_r, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 1.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().line_segment(
                        [
                            egui::pos2(hair_r.left(), hair_r.center().y),
                            egui::pos2(hair_r.right(), hair_r.center().y),
                        ],
                        egui::Stroke::new(1.0, palette.card_alt_stroke.gamma_multiply(0.7)),
                    );

                    let order_len = pl.play_order.len();
                    let row_h = 68.0_f32;
                    let mut scroll = egui::ScrollArea::vertical()
                        .id_salt(("watch_playlist_panel_scroll", scroll_id, &pl_id))
                        .max_height(420.0)
                        .auto_shrink([false, false]);
                    if pl.scroll_to_current {
                        let initial_offset = (pl.current_pos as f32 * row_h - 140.0).max(0.0);
                        scroll = scroll.vertical_scroll_offset(initial_offset);
                        pl.scroll_to_current = false;
                    }

                    self.thumbnails.begin_pass();
                    scroll.show_rows(ui, row_h, order_len, |ui, row_range| {
                        let avail_w = ui.available_width().max(200.0);
                        for pos in row_range {
                            let Some(video) = pl.video_at_order_pos(pos) else {
                                continue;
                            };
                            let is_active = pos == pl.current_pos;
                            let thumb_url = if !video.thumbnail.trim().is_empty() {
                                video.thumbnail.clone()
                            } else if let Some(vid_id) = history::youtube_video_id(&video.url) {
                                format!("https://i.ytimg.com/vi/{vid_id}/hqdefault.jpg")
                            } else {
                                String::new()
                            };
                            let history_entry = history::youtube_video_id(&video.url)
                                .and_then(|id| self.history.data.get(&id));
                            let progress_ratio = history_entry.and_then(|entry| {
                                let duration_ms = entry.duration_ms? as f32;
                                if duration_ms <= 0.0 || entry.position_ms == 0 {
                                    None
                                } else {
                                    Some((entry.position_ms as f32 / duration_ms).clamp(0.04, 1.0))
                                }
                            });

                            ui.push_id(("watch_pl_row", &pl_id, pos), |ui| {
                                let (row_rect, row_resp) = ui.allocate_exact_size(
                                    egui::vec2(avail_w, row_h),
                                    egui::Sense::click(),
                                );
                                if is_active {
                                    ui.painter().rect_filled(
                                        row_rect,
                                        0.0,
                                        palette.card_playing_bg,
                                    );
                                } else if row_resp.hovered() {
                                    ui.painter().rect_filled(
                                        row_rect,
                                        0.0,
                                        palette.card_alt_stroke.gamma_multiply(0.35),
                                    );
                                }
                                let content_rect = row_rect.shrink2(egui::vec2(6.0, 6.0));
                                let mut row_clicked = row_resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked();
                                ui.scope_builder(
                                    egui::UiBuilder::new().max_rect(content_rect),
                                    |ui| {
                                        ui.set_clip_rect(content_rect);
                                        ui.horizontal_top(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            let (gutter_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(20.0, 56.0),
                                                egui::Sense::hover(),
                                            );
                                            if is_active {
                                                paint_small_play_triangle(
                                                    ui,
                                                    gutter_rect.center(),
                                                    4.5,
                                                    palette.channel_fg,
                                                );
                                            } else {
                                                ui.painter().text(
                                                    gutter_rect.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    format!("{}", pos + 1),
                                                    egui::FontId::proportional(11.5),
                                                    palette.neutral_status_fg,
                                                );
                                            }
                                            let thumb_size = egui::vec2(100.0, 56.0);
                                            let thumb_resp = if !thumb_url.trim().is_empty() {
                                                self.thumbnails.show_rounded(
                                                    ui, ctx, &thumb_url, thumb_size, 6.0,
                                                )
                                            } else {
                                                let (r, resp) = ui.allocate_exact_size(
                                                    thumb_size,
                                                    egui::Sense::click(),
                                                );
                                                ui.painter().rect_filled(r, 6.0, palette.card_bg);
                                                resp
                                            };
                                            paint_thumbnail_overlays(
                                                ui,
                                                thumb_resp.rect,
                                                &video.duration,
                                                progress_ratio,
                                                false,
                                            );
                                            if thumb_resp.clicked() {
                                                row_clicked = true;
                                            }
                                            ui.vertical(|ui| {
                                                let text_w =
                                                    (content_rect.width() - 134.0).max(70.0);
                                                ui.set_max_width(text_w);
                                                let title_btn = ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(truncate_text_chars(
                                                                &video.title,
                                                                52,
                                                            ))
                                                            .size(13.0)
                                                            .strong()
                                                            .color(palette.channel_fg),
                                                        )
                                                        .frame(false)
                                                        .wrap(),
                                                    )
                                                    .on_hover_cursor(
                                                        egui::CursorIcon::PointingHand,
                                                    );
                                                if title_btn.clicked() {
                                                    row_clicked = true;
                                                }
                                                if !video.channel.trim().is_empty() {
                                                    ui.label(
                                                        RichText::new(truncate_text_chars(
                                                            &video.channel,
                                                            32,
                                                        ))
                                                        .size(11.5)
                                                        .color(palette.neutral_status_fg),
                                                    );
                                                }
                                            });
                                        });
                                    },
                                );
                                if row_clicked {
                                    selected_pos = Some(pos);
                                }
                            });
                        }
                    });
                    self.thumbnails.end_pass(ctx);

                    if pl.loading_more {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    crate::skeleton::visible_spinner(ui);
                                    ui.label(
                                        RichText::new(if pt {
                                            "Carregando mais vídeos..."
                                        } else {
                                            "Loading more videos..."
                                        })
                                        .size(12.0)
                                        .color(palette.neutral_status_fg),
                                    );
                                });
                            });
                    }
                }
            });

        if close_panel {
            if let Some(cancel) = self.watch.mix_cancel.take() {
                cancel.cancel();
            }
            self.watch.mix_rx = None;
            self.watch.mix_active_request_id = None;
            return;
        }

        if add_all_to_queue && !pl.videos.is_empty() {
            let _ = self.enqueue_all_videos(&pl.videos);
        }
        if open_full_playlist {
            let pid = pl.playlist_id.clone();
            let title = pl.title.clone();
            let local_idx = pl.local_collection_idx;
            self.close_video(ctx);
            if pid == "WL" {
                self.ui.browse_section = BrowseSection::WatchLater;
                self.on_enter_watch_later_section(ctx);
            } else if pid == "LL" {
                self.ui.browse_section = BrowseSection::LikedVideos;
                self.start_liked_videos_fetch(false, ctx);
            } else if let Some(idx) = local_idx {
                self.ui.browse_section = BrowseSection::Playlists;
                self.library.selected_collection_idx = Some(idx);
            } else {
                self.open_remote_playlist_detail(pid, Some(title), None, ctx);
            }
            return;
        }

        if let Some(pos) = selected_pos
            && let Some(video) = pl.select_pos(pos)
        {
            self.playback.pending_open_keep_playlist = true;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(video);
        }
        self.watch.active_playlist = Some(pl);
    }

    pub(crate) fn watch_sidebar_results_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
    ) -> Option<Video> {
        self.sync_theme_visuals(ctx);
        self.ensure_history_synchronized();
        let palette = self.palette();
        if !self.channels.view_open && self.watch.extras.is_transcript_open() {
            self.subtitles_ui(ui, ctx, scroll_id);
            ui.add_space(12.0);
        }
        self.watch_sidebar_playlist_panel_ui(ui, ctx, &palette, scroll_id);
        if self.watch.active_playlist.is_some() {
            ui.add_space(12.0);
        }
        self.watch_sidebar_tabs_ui(ui, ctx, &palette);
        ui.add_space(10.0);
        if self.channels.view_open {
            return self.channel_section_ui(ui, ctx, &format!("{scroll_id}_channel"), true, None);
        }
        self.watch_sidebar_feed_ui(ui, ctx, scroll_id)
    }
}

pub(crate) fn is_channel_verified(metadata: Option<&VideoMetadata>) -> bool {
    metadata.is_some_and(|meta| meta.channel_verified)
}

pub(crate) fn is_watch_channel_verified(
    metadata: Option<&VideoMetadata>,
    channel_page: Option<&ChannelPage>,
    channel_target: Option<&ChannelTarget>,
) -> bool {
    if is_channel_verified(metadata) {
        return true;
    }
    let Some(page) = channel_page.filter(|page| page.verified == Some(true)) else {
        return false;
    };
    if let Some(target) = channel_target {
        if &page.target == target {
            return true;
        }
        if let (ChannelTarget::ChannelId(target_id), Some(page_id)) =
            (target, page.channel_id.as_deref())
            && target_id.eq_ignore_ascii_case(page_id.trim())
        {
            return true;
        }
    }
    if let (Some(meta_id), Some(page_id)) = (
        metadata.and_then(|m| m.channel_id.as_deref()),
        page.channel_id.as_deref(),
    ) && !meta_id.trim().is_empty()
        && meta_id.trim().eq_ignore_ascii_case(page_id.trim())
    {
        return true;
    }
    false
}

pub(crate) fn format_sidebar_channel_label(
    channel: &str,
    metadata: Option<&VideoMetadata>,
) -> Option<String> {
    if channel.is_empty() {
        return None;
    }
    if is_channel_verified(metadata) {
        Some(format!("{} \u{2714}", truncate_text_chars(channel, 34)))
    } else {
        Some(truncate_text_chars(channel, 38))
    }
}

pub(crate) fn sidebar_video_matches_current_channel(
    current: &Video,
    current_meta: Option<&VideoMetadata>,
    current_channel_id_hint: Option<&str>,
    current_channel_name_hint: Option<&str>,
    candidate: &Video,
    candidate_meta: Option<&VideoMetadata>,
) -> bool {
    if candidate.url == current.url {
        return false;
    }
    if let (Some(cur_vid), Some(cand_vid)) = (
        history::youtube_video_id(&current.url),
        history::youtube_video_id(&candidate.url),
    ) && cur_vid == cand_vid
    {
        return false;
    }

    let current_ch_id = current_meta
        .and_then(|m| m.channel_id.as_deref())
        .or(current_channel_id_hint)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            let trimmed = current.channel.trim();
            FastTube::looks_like_youtube_channel_id(trimmed).then_some(trimmed)
        });
    let candidate_ch_id = candidate_meta
        .and_then(|m| m.channel_id.as_deref())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            let trimmed = candidate.channel.trim();
            FastTube::looks_like_youtube_channel_id(trimmed).then_some(trimmed)
        });

    if let (Some(cur_id), Some(cand_id)) = (current_ch_id, candidate_ch_id) {
        return cur_id.eq_ignore_ascii_case(cand_id);
    }

    let current_name = (!current.channel.trim().is_empty())
        .then_some(current.channel.as_str())
        .or(current_channel_name_hint)
        .unwrap_or("");
    let current_key = recommendations::normalize_channel_key(current_name);
    if current_key.is_empty() {
        return false;
    }
    let candidate_key = recommendations::normalize_channel_key(&candidate.channel);
    !candidate_key.is_empty() && candidate_key == current_key
}
