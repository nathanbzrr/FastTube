use super::*;
#[cfg(windows)]
use eframe::egui::RichText;

impl FastTube {
    #[cfg(windows)]
    pub(crate) fn fullscreen_playback_transport_ui(
        &mut self,
        ui: &mut egui::Ui,
        bar: &FullscreenBarContext,
        telemetry: &PlayerTelemetry,
        actions: &mut FullscreenControlsActions,
    ) {
        let selected_url = self.selected.as_ref().map(|v| v.url.clone());
        let Some(player) = &self.player else {
            return;
        };
        let icon = if telemetry.paused || telemetry.ended {
            Icon::Play
        } else {
            Icon::Pause
        };
        if ui
            .add(egui::Button::image(icon.image(bar.palette.icon_fg, 18.0)))
            .on_hover_text(if telemetry.paused || telemetry.ended {
                "Reproduzir (Espaço ou K)"
            } else {
                "Pausar (Espaço ou K)"
            })
            .clicked()
        {
            if telemetry.ended && bar.is_live {
                actions.go_live_clicked = true;
            } else {
                let result = if telemetry.ended {
                    player
                        .seek(0.0, "absolute+exact")
                        .and_then(|()| player.set("pause", "no"))
                } else {
                    player.set("pause", if telemetry.paused { "no" } else { "yes" })
                };
                match result {
                    Ok(()) => {
                        let next_paused = if telemetry.ended {
                            false
                        } else {
                            !telemetry.paused
                        };
                        Self::record_player_osd(
                            ui.ctx(),
                            if next_paused {
                                "\u{23f8} Pausado"
                            } else {
                                "\u{25b6} Reproduzindo"
                            },
                        );
                        if let Some(url) = selected_url.as_deref() {
                            Self::update_player_osd_snapshot_state(ui.ctx(), url, |snap| {
                                snap.paused = next_paused;
                            });
                        }
                        if telemetry.ended {
                            actions.manual_seek_recorded = Some(0.0);
                        } else if !telemetry.paused {
                            actions.flush_on_pause = true;
                        }
                    }
                    Err(error) => self.message = error,
                }
            }
        }
        if styled_compact_button_enabled(ui, bar.has_queued, "⏭ Próximo", false, &bar.palette)
            .on_hover_text("Avançar manualmente para o próximo vídeo da fila")
            .clicked()
        {
            actions.next_clicked = true;
        }
        if !bar.is_live {
            let step = self.seek_step_secs();
            let step_f = f64::from(step);
            if styled_compact_button(ui, &format!("−{step}s"), false, &bar.palette)
                .on_hover_text(format!("Retroceder {step} segundos (←)"))
                .clicked()
            {
                match player.seek(-step_f, "relative") {
                    Ok(()) => {
                        Self::record_player_osd(ui.ctx(), format!("\u{23ea} -{step}s"));
                        actions.manual_seek_recorded = Some((bar.position - step_f).max(0.0));
                    }
                    Err(error) => self.message = error,
                }
            }
            if styled_compact_button(ui, &format!("+{step}s"), false, &bar.palette)
                .on_hover_text(format!("Avançar {step} segundos (→)"))
                .clicked()
            {
                match player.seek(step_f, "relative") {
                    Ok(()) => {
                        Self::record_player_osd(ui.ctx(), format!("\u{23e9} +{step}s"));
                        let next = bar
                            .duration
                            .map(|dur| (bar.position + step_f).min(dur))
                            .unwrap_or(bar.position + step_f);
                        actions.manual_seek_recorded = Some(next);
                    }
                    Err(error) => self.message = error,
                }
            }
        }
        if bar.is_live {
            let dot = if bar.at_live_edge {
                Color32::from_rgb(255, 0, 51)
            } else {
                Color32::from_rgb(155, 155, 160)
            };
            let live_text = if bar.at_live_edge {
                "● Ao vivo".to_owned()
            } else {
                format!(
                    "● Ao vivo ({})",
                    overlay::format_live_behind(bar.live_behind_secs)
                )
            };
            let tooltip = if bar.at_live_edge {
                "Você está assistindo ao vivo"
            } else {
                "Voltar para a transmissão ao vivo"
            };
            if ui
                .add(
                    egui::Button::new(RichText::new(live_text).small().strong().color(dot))
                        .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tooltip)
                .clicked()
            {
                actions.go_live_clicked = true;
            }
        } else {
            ui.label(
                RichText::new(format!(
                    "{} / {}",
                    format_time(self.playback.seek_drag.unwrap_or(bar.position)),
                    bar.duration
                        .map(format_time)
                        .unwrap_or_else(|| "--:--".into())
                ))
                .small()
                .weak(),
            );
        }
    }

    #[cfg(windows)]
    pub(crate) fn fullscreen_bookmark_and_mini_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        position: f64,
        library_changed: &mut bool,
    ) {
        let palette = self.palette();
        ui.add_sized(
            [90.0, 22.0],
            egui::TextEdit::singleline(&mut self.library.bookmark_note)
                .hint_text("Nota do marcador"),
        );
        if styled_compact_button(ui, "＋ Marcador", false, &palette).clicked()
            && self.add_current_bookmark(video, position)
        {
            *library_changed = true;
        }
        if styled_compact_button(
            ui,
            if self.playback.mini_player {
                "Sair do mini"
            } else {
                "Mini player"
            },
            self.playback.mini_player,
            &palette,
        )
        .clicked()
        {
            self.toggle_mini_player(ctx);
        }
    }

    #[cfg(windows)]
    fn apply_mpv_volume_with_osd(
        player: &player::Player,
        ctx: &egui::Context,
        video_url: Option<&str>,
        volume: f64,
    ) -> Result<(), String> {
        player.set("volume", &format!("{volume:.0}"))?;
        let vol_rounded = volume.round() as i32;
        Self::record_player_osd(
            ctx,
            if vol_rounded == 0 {
                "\u{1f507} Mudo".to_owned()
            } else {
                format!("\u{1f50a} {vol_rounded}%")
            },
        );
        if let Some(url) = video_url {
            Self::update_player_osd_snapshot_state(ctx, url, |snap| {
                snap.volume_rounded = vol_rounded;
            });
        }
        Ok(())
    }

    #[cfg(windows)]
    pub(crate) fn fullscreen_audio_speed_chapters_ui(
        &mut self,
        ui: &mut egui::Ui,
        compact_controls: bool,
        muted: bool,
        palette: ThemePalette,
    ) {
        let selected_url = self.selected.as_ref().map(|v| v.url.clone());
        let autoscroll_active = self.autoscroll.is_active();
        let Some(player) = &self.player else {
            return;
        };
        ui.add_space(6.0);
        let sound_icon = if muted { Icon::Muted } else { Icon::Volume };
        let mut volume = player.number("volume").unwrap_or(100.0).clamp(0.0, 100.0);
        let mute_resp = ui
            .add(egui::Button::image(sound_icon.image(palette.icon_fg, 17.0)))
            .on_hover_text("Silenciar ou ativar som (M)");
        if mute_resp.clicked() {
            match player.set("mute", if muted { "no" } else { "yes" }) {
                Ok(()) => {
                    let next_muted = !muted;
                    let vol_rounded = volume.round() as i32;
                    Self::record_player_osd(
                        ui.ctx(),
                        if next_muted || vol_rounded == 0 {
                            "\u{1f507} Mudo".to_owned()
                        } else {
                            format!("\u{1f50a} {vol_rounded}%")
                        },
                    );
                    if let Some(url) = selected_url.as_deref() {
                        Self::update_player_osd_snapshot_state(ui.ctx(), url, |snap| {
                            snap.muted = next_muted;
                        });
                    }
                }
                Err(error) => self.message = error,
            }
        }
        let vol_resp = ui
            .add_sized(
                [76.0, 20.0],
                egui::Slider::new(&mut volume, 0.0..=100.0).show_value(false),
            )
            .on_hover_text(format!("Volume: {:.0}%", volume));
        if vol_resp.changed() {
            if let Err(error) =
                Self::apply_mpv_volume_with_osd(player, ui.ctx(), selected_url.as_deref(), volume)
            {
                self.message = error;
            }
        } else if !autoscroll_active {
            let volume_rect = mute_resp.rect.union(vol_resp.rect);
            let over_volume = mute_resp.hovered()
                || vol_resp.hovered()
                || ui
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|p| volume_rect.contains(p));
            if over_volume {
                let wheel_y = ui.input(|i| i.smooth_scroll_delta.y);
                if wheel_y.is_finite() && wheel_y != 0.0 {
                    ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
                }
                if let Some(next_vol) =
                    compute_wheel_volume_target(volume as f32, muted, wheel_y, 5.0)
                {
                    let mut unmute_ok = true;
                    if muted && next_vol > 0.0 {
                        if let Err(error) = player.set("mute", "no") {
                            self.message = error;
                            unmute_ok = false;
                        } else if let Some(url) = selected_url.as_deref() {
                            Self::update_player_osd_snapshot_state(ui.ctx(), url, |snap| {
                                snap.muted = false;
                            });
                        }
                    }
                    if unmute_ok
                        && let Err(error) = Self::apply_mpv_volume_with_osd(
                            player,
                            ui.ctx(),
                            selected_url.as_deref(),
                            f64::from(next_vol),
                        )
                    {
                        self.message = error;
                    }
                }
            }
        }
        ui.separator();
        let mut speed = player.number("speed").unwrap_or(1.0);
        let mut chosen_speed: Option<f64> = None;
        egui::ComboBox::from_id_salt("speed")
            .selected_text(if compact_controls {
                format!("{speed:.2}x")
            } else {
                format!("Velocidade {speed:.2}x")
            })
            .show_ui(ui, |ui| {
                for choice in overlay::SPEED_PRESETS {
                    if ui
                        .selectable_value(&mut speed, choice, format!("{choice:.2}\u{00d7}"))
                        .changed()
                    {
                        chosen_speed = Some(choice);
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
                        if ui.selectable_label(false, title).clicked()
                            && let Some(time) = player.number(&format!("chapter-list/{index}/time"))
                        {
                            let _ = player.seek(time, "absolute");
                            Self::record_player_osd(
                                ui.ctx(),
                                format!("\u{23e9} {}", format_time(time)),
                            );
                        }
                    }
                });
        }
        if let Some(choice) = chosen_speed {
            self.apply_and_remember_playback_speed(choice);
            Self::record_player_osd(ui.ctx(), format!("\u{26a1} {choice:.2}x"));
            if let Some(url) = selected_url.as_deref() {
                Self::update_player_osd_snapshot_state(ui.ctx(), url, |snap| {
                    snap.speed_hundredths = (choice * 100.0).round() as i32;
                });
            }
        }
    }

    #[cfg(windows)]
    pub(crate) fn fullscreen_quality_combo_ui(
        &mut self,
        ui: &mut egui::Ui,
        video: &Video,
        bar: &FullscreenBarContext,
        telemetry: &PlayerTelemetry,
    ) {
        let Some(player) = &self.player else {
            return;
        };
        let mut quality = self.playback.quality;
        let current_height = player.number("height").map(|height| height as u32);
        let quality_label = match quality {
            Some(height) if bar.compact_controls => format!("{height}p"),
            Some(height) => format!("Qualidade: {height}p"),
            None => {
                let preferred = self
                    .settings
                    .playback
                    .value
                    .max_height
                    .map(|height| format!("até {height}p"))
                    .unwrap_or_else(|| "automática".into());
                current_height
                    .map(|height| {
                        if bar.compact_controls {
                            format!("{preferred} ({height}p)")
                        } else {
                            format!("Qualidade: {preferred} ({height}p)")
                        }
                    })
                    .unwrap_or_else(|| format!("Qualidade: {preferred} (não confirmada)"))
            }
        };
        let quality_tip = if self.is_playback_metadata_loading() {
            "Consultando as resoluções disponíveis...".to_owned()
        } else {
            self.playback
                .quality_error
                .clone()
                .unwrap_or_else(|| "Escolha a resolução disponível para este vídeo".into())
        };
        let available_qualities = if let Some(ref manifest) = self.playback.current_manifest
            && !manifest.is_expired()
            && !manifest.is_live
            && history::youtube_video_id(&video.url).as_deref() == Some(manifest.video_id.as_str())
        {
            let heights = manifest.available_heights();
            if heights.is_empty() {
                self.playback.qualities.clone()
            } else {
                heights
            }
        } else {
            self.playback.qualities.clone()
        };
        ui.add_enabled_ui(!available_qualities.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("quality")
                .selected_text(quality_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut quality, None, "Automática");
                    for &height in &available_qualities {
                        ui.selectable_value(&mut quality, Some(height), format!("{height}p"));
                    }
                })
                .response
                .on_hover_text(quality_tip);
        });
        if quality != self.playback.quality {
            self.apply_player_quality(
                video,
                quality,
                bar.position,
                telemetry.raw_path.as_deref(),
                telemetry.raw_position,
            );
        }
    }

    #[cfg(windows)]
    pub(crate) fn fullscreen_subtitle_tracks_combo_ui(
        &mut self,
        ui: &mut egui::Ui,
        compact_controls: bool,
    ) {
        let Some(player) = &self.player else {
            return;
        };
        let subtitle_tracks: Vec<(String, String)> = player
            .subtitle_tracks()
            .into_iter()
            .map(|track| {
                let label = if !track.title.is_empty() {
                    track.title
                } else if !track.lang.is_empty() {
                    track.lang
                } else {
                    format!("Faixa {}", track.id)
                };
                (track.id, label)
            })
            .collect();
        if !subtitle_tracks.is_empty() {
            let snap = player.snapshot();
            let current = if snap.observed {
                snap.sid.unwrap_or_else(|| "no".into())
            } else {
                player.property("sid").unwrap_or_else(|| "no".into())
            };
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
                    ui.selectable_value(&mut selected, "no".to_owned(), "Desativadas");
                    for (id, label) in &subtitle_tracks {
                        ui.selectable_value(&mut selected, id.clone(), label);
                    }
                });
            if selected != current {
                self.watch.extras.cancel_active_subtitle();
                let result = if selected == "no" {
                    player.disable_subtitles()
                } else {
                    player
                        .set("sid", &selected)
                        .and_then(|()| player.set("sub-visibility", "yes"))
                };
                if let Err(error) = result {
                    self.message = error;
                }
            }
        }
    }

    #[cfg(windows)]
    pub(crate) fn fullscreen_view_toggles_ui(
        ui: &mut egui::Ui,
        bar: &FullscreenBarContext,
        actions: &mut FullscreenControlsActions,
    ) {
        if ui
            .button("⚙")
            .on_hover_text("Abrir ou fechar configurações do player")
            .clicked()
        {
            actions.settings_toggled = true;
        }
        let theater_button_label = if bar.compact_controls {
            if bar.theater_mode { "Normal" } else { "Teatro" }
        } else if bar.theater_mode {
            "▭ Normal"
        } else {
            "▭ Teatro"
        };
        let theater_button = egui::Button::new(RichText::new(theater_button_label).color(
            if bar.theater_mode {
                bar.palette.active_button_fg
            } else {
                bar.palette.theater_inactive_fg
            },
        ))
        .fill(if bar.theater_mode {
            bar.palette.active_button_bg
        } else {
            bar.palette.theater_inactive_bg
        });
        if ui
            .add_enabled(!bar.fullscreen, theater_button)
            .on_hover_text(if bar.fullscreen {
                "Modo teatro indisponível enquanto a tela cheia estiver ativa"
            } else if bar.theater_mode {
                "Sair do modo teatro e restaurar visualização padrão (T)"
            } else {
                "Ativar modo teatro na janela (T)"
            })
            .clicked()
        {
            actions.theater_toggled = true;
        }
        let screen_icon = if bar.fullscreen {
            Icon::Minimize
        } else {
            Icon::Maximize
        };
        if ui
            .add(egui::Button::image(
                screen_icon.image(bar.palette.icon_fg, 17.0),
            ))
            .on_hover_text("Tela cheia (F ou Esc para sair)")
            .clicked()
        {
            actions.fullscreen_toggled = true;
        }
        if ui
            .add(egui::Button::image(
                Icon::Close.image(bar.palette.icon_fg, 17.0),
            ))
            .on_hover_text("Fechar vídeo")
            .clicked()
        {
            actions.close_clicked = true;
        }
    }

    #[cfg(windows)]
    pub(crate) fn fullscreen_controls_bar_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        bar: &FullscreenBarContext,
        telemetry: &PlayerTelemetry,
        actions: &mut FullscreenControlsActions,
    ) {
        if self.player.is_none() {
            return;
        }
        egui::Frame::new()
            .fill(bar.palette.player_bar_bg)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_max_width(bar.width);
                ui.horizontal_wrapped(|ui| {
                    self.fullscreen_playback_transport_ui(ui, bar, telemetry, actions);
                    if !bar.compact_controls {
                        self.fullscreen_bookmark_and_mini_ui(
                            ui,
                            ctx,
                            video,
                            bar.position,
                            &mut actions.library_changed,
                        );
                    }
                    self.fullscreen_audio_speed_chapters_ui(
                        ui,
                        bar.compact_controls,
                        telemetry.muted,
                        bar.palette,
                    );
                    self.fullscreen_quality_combo_ui(ui, video, bar, telemetry);
                    self.fullscreen_subtitle_tracks_combo_ui(ui, bar.compact_controls);
                    Self::fullscreen_view_toggles_ui(ui, bar, actions);
                });
            });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fullscreen_seek_ui(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        position: f64,
        duration: Option<f64>,
        is_live: bool,
        is_live_dvr: bool,
        video: &Video,
        now: Instant,
    ) {
        let seekable = duration.is_some() && (!is_live || is_live_dvr);
        let mut seek = if is_live && !is_live_dvr {
            duration.unwrap_or(1.0)
        } else {
            self.playback.seek_drag.unwrap_or(position)
        };
        let response = ui
            .scope(|ui| {
                ui.spacing_mut().slider_width = width;
                ui.add_enabled(
                    seekable,
                    egui::Slider::new(&mut seek, 0.0..=duration.unwrap_or(1.0))
                        .show_value(false)
                        .trailing_fill(true),
                )
            })
            .inner;
        if seekable
            && (response.hovered() || response.dragged())
            && response.rect.width() > 2.0
            && let Some(dur) = duration.filter(|d| d.is_finite() && *d > 0.0)
        {
            let pointer_x = if response.dragged() {
                Some(
                    response.rect.left()
                        + response.rect.width() * ((seek / dur) as f32).clamp(0.0, 1.0),
                )
            } else {
                ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()))
                    .map(|p| p.x)
            };
            if let Some(anchor_x) = pointer_x {
                let frac =
                    ((anchor_x - response.rect.left()) / response.rect.width()).clamp(0.0, 1.0);
                let preview_secs = f64::from(frac) * dur;
                let time_text = if is_live {
                    if frac >= 0.985 {
                        "Ao vivo".to_owned()
                    } else {
                        overlay::format_live_behind((dur - preview_secs).max(0.0))
                    }
                } else {
                    format_time(preview_secs)
                };
                let storyboard_frame = if !is_live
                    && self.playback.storyboard_video_url.as_deref() == Some(video.url.as_str())
                    && let Some((sheet_url, uv, aspect_ratio)) =
                        self.playback.storyboard.as_ref().and_then(|spec| {
                            let aspect = if spec.thumb_width > 0 && spec.thumb_height > 0 {
                                spec.thumb_width as f32 / spec.thumb_height as f32
                            } else {
                                16.0 / 9.0
                            };
                            spec.lookup(preview_secs)
                                .map(|(url, uv)| (url.to_owned(), uv, aspect))
                        })
                    && let Some((texture_id, _)) = self
                        .thumbnails
                        .resolve_storyboard_texture(ui.ctx(), &sheet_url)
                {
                    Some((texture_id, uv, aspect_ratio))
                } else {
                    None
                };

                let window_rect = ui.ctx().content_rect();
                let bounds_left = response.rect.left().max(window_rect.left() + 6.0);
                let bounds_right = response.rect.right().min(window_rect.right() - 6.0);
                let max_inner_w = (bounds_right - bounds_left - 16.0).max(48.0);
                let thumb_size = storyboard_frame.map(|(_, _, aspect_ratio)| {
                    let ratio = if aspect_ratio.is_finite() && aspect_ratio > 0.2 {
                        aspect_ratio.clamp(0.5, 3.0)
                    } else {
                        16.0 / 9.0
                    };
                    let mut w = (response.rect.width() * 0.24)
                        .clamp(112.0, 168.0)
                        .min(max_inner_w);
                    let mut h = w / ratio;
                    if h > 102.0 {
                        h = 102.0;
                        w = (h * ratio).min(max_inner_w);
                    }
                    egui::vec2(w.round(), h.round())
                });
                let fg_layer =
                    egui::LayerId::new(egui::Order::Foreground, ui.id().with("seek_hover_preview"));
                let painter = ui.ctx().layer_painter(fg_layer);
                let time_galley = painter.layout_no_wrap(
                    time_text,
                    egui::FontId::proportional(11.5),
                    Color32::WHITE,
                );
                let pad = egui::vec2(8.0, 6.0);
                let thumb_gap = if thumb_size.is_some() { 6.0 } else { 0.0 };
                let content_w = thumb_size
                    .map_or(0.0, |sz| sz.x)
                    .max(time_galley.size().x)
                    .min(max_inner_w);
                let content_h =
                    thumb_size.map_or(0.0, |sz| sz.y + thumb_gap) + time_galley.size().y;
                let pill_w = content_w + pad.x * 2.0;
                let pill_h = content_h + pad.y * 2.0;
                let pill_x = if bounds_right - bounds_left <= pill_w {
                    bounds_left
                } else {
                    (anchor_x - pill_w * 0.5).clamp(bounds_left, bounds_right - pill_w)
                };
                let pill_y = (response.rect.top() - pill_h - 8.0).max(window_rect.top() + 6.0);
                let pill_rect = egui::Rect::from_min_size(
                    egui::pos2(pill_x, pill_y),
                    egui::vec2(pill_w, pill_h),
                );
                painter.rect_filled(
                    pill_rect,
                    egui::CornerRadius::same(10),
                    Color32::from_rgba_unmultiplied(15, 16, 20, 240),
                );
                painter.rect_stroke(
                    pill_rect,
                    egui::CornerRadius::same(10),
                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 42)),
                    egui::StrokeKind::Inside,
                );
                let mut cursor_y = pill_rect.top() + pad.y;
                if let (Some((texture_id, uv, _)), Some(thumb_sz)) = (storyboard_frame, thumb_size)
                {
                    let thumb_x = pill_rect.center().x - thumb_sz.x * 0.5;
                    let thumb_rect =
                        egui::Rect::from_min_size(egui::pos2(thumb_x, cursor_y), thumb_sz);
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .layer_id(fg_layer)
                            .max_rect(thumb_rect),
                        |ui| {
                            egui::Image::from_texture((texture_id, thumb_sz))
                                .uv(uv)
                                .corner_radius(8.0)
                                .paint_at(ui, thumb_rect);
                        },
                    );
                    painter.rect_stroke(
                        thumb_rect,
                        egui::CornerRadius::same(8),
                        egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 44)),
                        egui::StrokeKind::Inside,
                    );
                    cursor_y += thumb_sz.y + thumb_gap;
                }
                let time_pos =
                    egui::pos2(pill_rect.center().x - time_galley.size().x * 0.5, cursor_y);
                painter.galley(time_pos, time_galley, Color32::WHITE);
            }
        }
        if seekable && response.dragged() {
            self.playback.seek_drag = Some(seek);
        }
        if seekable && (response.drag_stopped() || (response.changed() && !response.dragged())) {
            if is_live
                && let Some(dur) = duration
                && dur > 0.0
                && (dur <= 1.5 || (seek / dur) >= 0.985 || seek >= (dur - 1.5).max(0.0))
            {
                self.playback.seek_drag = None;
                self.go_live_edge(video);
                return;
            }
            let target = if is_live {
                duration.map_or(seek, |dur| seek.min((dur - 1.5).max(0.0)))
            } else {
                seek
            };
            let seek_res = self
                .player
                .as_ref()
                .map(|player| player.seek(target, "absolute+exact"))
                .unwrap_or(Ok(()));
            match seek_res {
                Ok(()) => {
                    let icon = if target >= position {
                        "\u{23e9}"
                    } else {
                        "\u{23ea}"
                    };
                    Self::record_player_osd(ui.ctx(), format!("{icon} {}", format_time(target)));
                    self.record_manual_seek(target, duration, now);
                }
                Err(error) => self.message = error,
            }
            self.playback.seek_drag = None;
        }
    }

    pub(crate) fn update_player_overlay_pointer_activity(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
    ) {
        let (pointer_pos, any_down, any_btn_edge, wheel_scrolled) = ctx.input(|i| {
            let pos = i.pointer.hover_pos().or_else(|| i.pointer.interact_pos());
            let wheel = i
                .raw
                .events
                .iter()
                .any(|ev| matches!(ev, egui::Event::MouseWheel { .. }));
            let btn_edge = i.pointer.any_pressed() || i.pointer.any_released();
            (pos, i.pointer.any_down(), btn_edge, wheel)
        });
        if let Some(pos) = pointer_pos.filter(|p| rect.contains(*p)) {
            let moved = self
                .playback
                .overlay_last_pointer_pos
                .is_none_or(|prev| prev.distance_sq(pos) > 0.25);
            if moved || any_down || any_btn_edge || wheel_scrolled {
                self.playback.overlay_last_activity = Instant::now();
            }
            self.playback.overlay_last_pointer_pos = Some(pos);
        } else {
            self.playback.overlay_last_pointer_pos = None;
        }
    }

    pub(crate) fn refresh_stats_for_nerds(&mut self, force: bool) {
        if !force
            && self
                .playback
                .stats_for_nerds_last_poll
                .is_some_and(|t| t.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.playback.stats_for_nerds_last_poll = Some(Instant::now());
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let configured_fallback = self
                .settings
                .prefs
                .value
                .hwdec_mode
                .as_mpv_value()
                .to_owned();
            self.playback.diagnostics = diagnostics::collect_from_property_reader(|prop| {
                player.property(prop).or_else(|| {
                    (prop == diagnostics::PROP_HWDEC).then(|| configured_fallback.clone())
                })
            });
            self.playback.stats_for_nerds = app_state::PlayerNerdsStats {
                audio_codec: player.property("audio-codec-name"),
                fps: player
                    .double_cstr(c"estimated-vf-fps")
                    .or_else(|| player.double_cstr(c"container-fps"))
                    .filter(|v| v.is_finite() && *v > 0.0),
                dropped_frames: player
                    .int_cstr(c"frame-drop-count")
                    .map(|v| v.max(0) as u64),
                video_bitrate_bps: player
                    .double_cstr(c"video-bitrate")
                    .filter(|v| v.is_finite() && *v > 0.0),
                audio_bitrate_bps: player
                    .double_cstr(c"audio-bitrate")
                    .filter(|v| v.is_finite() && *v > 0.0),
            };
        }
    }

    fn viewport_pixel_size(video_rect: egui::Rect, ctx: &egui::Context) -> Option<VideoSize> {
        let ppp = ctx.pixels_per_point().max(0.1);
        let w = (video_rect.width() * ppp).round().max(0.0) as u32;
        let h = (video_rect.height() * ppp).round().max(0.0) as u32;
        VideoSize::new(w, h)
    }

    fn build_youtube_url_at_current_time(video_url: &str, position_secs: f64) -> String {
        let secs = if position_secs.is_finite() && position_secs >= 0.0 {
            position_secs.floor() as u64
        } else {
            0
        };
        if let Some(id) = history::youtube_video_id(video_url) {
            format!("https://youtu.be/{id}?t={secs}")
        } else {
            crate::build_youtube_share_url(video_url, Some(secs))
        }
    }

    pub(crate) fn build_current_player_debug_info_json(
        &mut self,
        video_rect: Option<egui::Rect>,
        playback: &OverlayPlaybackContext,
        ctx: &egui::Context,
    ) -> String {
        self.refresh_stats_for_nerds(true);
        let video_id = history::youtube_video_id(&playback.video.url);
        let viewport_size = video_rect.and_then(|r| Self::viewport_pixel_size(r, ctx));
        #[cfg(windows)]
        let backend = if self
            .player
            .as_ref()
            .is_some_and(player::Player::is_render_api)
        {
            "libmpv-opengl"
        } else {
            "libmpv-hwnd"
        };
        #[cfg(not(windows))]
        let backend = "unavailable";

        let failure_category =
            (!self.message.trim().is_empty()).then(|| diagnostics::classify_failure(&self.message));
        let buffer_profile = buffer_profile_name(self.settings.buffer.profile);
        diagnostics::player_debug_info_json(&diagnostics::PlayerDebugInfoInput {
            app_version: env!("CARGO_PKG_VERSION"),
            video_id: video_id.as_deref(),
            video_size: self.playback.diagnostics.size,
            viewport_size,
            fps: self.playback.stats_for_nerds.fps,
            dropped_frames: self.playback.stats_for_nerds.dropped_frames,
            video_codec: self.playback.diagnostics.codec.as_deref(),
            audio_codec: self.playback.stats_for_nerds.audio_codec.as_deref(),
            hwdec_configured: self.playback.diagnostics.configured_hwdec.as_deref(),
            hwdec_effective: &self.playback.diagnostics.effective_decoding,
            speed: playback.speed,
            volume_percent: playback.raw_volume as f32,
            muted: playback.muted,
            buffer_profile,
            buffer_ahead_secs: playback.buffered_secs,
            is_live: playback.is_live,
            audio_only: self.playback.audio_only,
            loop_video: playback.loop_video,
            player_backend: backend,
            failure_category,
        })
    }

    pub(crate) fn build_stats_for_nerds_data(
        &self,
        video_rect: egui::Rect,
        playback: &OverlayPlaybackContext,
        ctx: &egui::Context,
    ) -> overlay::StatsForNerdsData {
        let pt = self.ui_pt();
        let video_id = history::youtube_video_id(&playback.video.url)
            .unwrap_or_else(|| self.pick("indispon\u{00ed}vel", "unavailable").to_owned());
        let vp_str = Self::viewport_pixel_size(video_rect, ctx)
            .map(|s| format!("{}x{}", s.width, s.height))
            .unwrap_or_else(|| "--".to_owned());
        let dropped = self.playback.stats_for_nerds.dropped_frames.unwrap_or(0);
        let viewport_and_frames = if pt {
            format!("{vp_str} / {dropped} descartados")
        } else {
            format!("{vp_str} / {dropped} dropped")
        };

        let res_str = self
            .playback
            .diagnostics
            .size
            .map(|s| format!("{}x{}", s.width, s.height))
            .or_else(|| playback.current_height.map(|h| format!("{h}p")))
            .unwrap_or_else(|| "--".to_owned());
        let fps_str = self
            .playback
            .stats_for_nerds
            .fps
            .map(|f| format!(" @ {:.0} fps", f.round()))
            .unwrap_or_default();
        let current_res = format!("{res_str}{fps_str}");

        let volume_and_speed = if playback.muted {
            format!(
                "0% ({}) / {:.2}x",
                self.pick("mudo", "muted"),
                playback.speed
            )
        } else {
            format!(
                "{:.0}% / {:.2}x",
                playback.raw_volume.round(),
                playback.speed
            )
        };

        let v_codec = self.playback.diagnostics.codec.as_deref().unwrap_or("--");
        let a_codec = self
            .playback
            .stats_for_nerds
            .audio_codec
            .as_deref()
            .unwrap_or("--");
        let codecs = format!("{v_codec} / {a_codec}");
        let decoding = self
            .playback
            .diagnostics
            .effective_decoding
            .short_label_for_lang(self.settings.prefs.value.language);

        let profile_label = buffer_profile_name(self.settings.buffer.profile);
        let buffer_health = match playback
            .buffered_secs
            .filter(|s| s.is_finite() && *s >= 0.0)
        {
            Some(secs) => format!("{secs:.1}s ({profile_label})"),
            None => format!("-- ({profile_label})"),
        };

        let total_bps = self
            .playback
            .stats_for_nerds
            .video_bitrate_bps
            .unwrap_or(0.0)
            + self
                .playback
                .stats_for_nerds
                .audio_bitrate_bps
                .unwrap_or(0.0);
        let bitrate = if total_bps >= 1_000_000.0 {
            Some(format!("{:.2} Mbps", total_bps / 1_000_000.0))
        } else if total_bps >= 1_000.0 {
            Some(format!("{:.0} kbps", total_bps / 1_000.0))
        } else {
            None
        };

        overlay::StatsForNerdsData {
            pt,
            video_id,
            viewport_and_frames,
            current_res,
            volume_and_speed,
            codecs,
            decoding,
            buffer_health,
            bitrate,
        }
    }

    pub(crate) fn handle_player_context_action(
        &mut self,
        action: overlay::PlayerContextAction,
        playback: &OverlayPlaybackContext,
        video_rect: egui::Rect,
        ctx: &egui::Context,
    ) {
        use overlay::{OverlayAction, PlayerContextAction};
        match action {
            PlayerContextAction::ToggleLoop => {
                self.handle_player_overlay_action(OverlayAction::ToggleLoop, playback, ctx);
                if !playback.is_live {
                    let msg = if self.playback.loop_video {
                        self.pick("Repetição ativada.", "Loop enabled.")
                    } else {
                        self.pick("Repetição desativada.", "Loop disabled.")
                    };
                    self.toasts.push(toast::ToastKind::Info, msg);
                }
            }
            PlayerContextAction::ToggleMiniPlayer => {
                self.toggle_mini_player(ctx);
            }
            PlayerContextAction::CopyVideoUrl => {
                let url = crate::build_youtube_share_url(&playback.video.url, None);
                match copy_text_to_clipboard(ctx, &url) {
                    Ok(()) => {
                        self.toasts.push(
                            toast::ToastKind::Success,
                            self.pick("URL do v\u{00ed}deo copiada.", "Video URL copied."),
                        );
                    }
                    Err(err) => self.message = err,
                }
            }
            PlayerContextAction::CopyVideoUrlAtCurrentTime => {
                let url =
                    Self::build_youtube_url_at_current_time(&playback.video.url, playback.position);
                match copy_text_to_clipboard(ctx, &url) {
                    Ok(()) => {
                        self.toasts.push(
                            toast::ToastKind::Success,
                            self.pick(
                                "URL com o momento atual copiada.",
                                "Timestamped video URL copied.",
                            ),
                        );
                    }
                    Err(err) => self.message = err,
                }
            }
            PlayerContextAction::CopyEmbedCode => {
                if let Some(id) = history::youtube_video_id(&playback.video.url) {
                    let embed = crate::build_youtube_embed_code(&id, &playback.video.title, None);
                    match copy_text_to_clipboard(ctx, &embed) {
                        Ok(()) => {
                            self.toasts.push(
                                toast::ToastKind::Success,
                                self.pick(
                                    "C\u{00f3}digo de incorporação copiado.",
                                    "Embed code copied.",
                                ),
                            );
                        }
                        Err(err) => self.message = err,
                    }
                }
            }
            PlayerContextAction::CopyDebugInfo => {
                let json =
                    self.build_current_player_debug_info_json(Some(video_rect), playback, ctx);
                match copy_text_to_clipboard(ctx, &json) {
                    Ok(()) => {
                        self.toasts.push(
                            toast::ToastKind::Success,
                            self.pick(
                                "Informa\u{00e7}\u{00f5}es de depura\u{00e7}\u{00e3}o copiadas.",
                                "Debug info copied.",
                            ),
                        );
                    }
                    Err(err) => self.message = err,
                }
            }
            PlayerContextAction::TroubleshootPlayback => {
                if self.playback.fullscreen {
                    self.toggle_fullscreen(ctx);
                }
                if self.playback.mini_player {
                    self.toggle_mini_player(ctx);
                }
                self.playback.player_menu = None;
                self.playback.player_context_menu = None;
                self.open_settings_tab(app_state::SettingsTab::BackupDiagnostics, ctx);
            }
            PlayerContextAction::ToggleStatsForNerds => {
                let next = !self.playback.stats_for_nerds_open;
                self.playback.stats_for_nerds_open = next;
                if next {
                    self.refresh_stats_for_nerds(true);
                }
            }
        }
    }

    pub(crate) fn player_overlay_ui(
        &mut self,
        ui: &mut egui::Ui,
        video_area: Option<(egui::Rect, egui::Response)>,
        playback: &OverlayPlaybackContext,
        ctx: &egui::Context,
    ) -> Option<overlay::OverlayAction> {
        let mut overlay_action = None;
        let was_context_menu_open = self.playback.player_context_menu.is_some();
        if let Some((rect, response)) = video_area {
            if !self.autoscroll.is_active() {
                self.update_player_overlay_pointer_activity(ctx, rect);
            }
            let pointer_pos =
                ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()));
            let pointer_keeps_visible = overlay::should_keep_overlay_visible_for_pointer(
                playback.fullscreen,
                pointer_pos,
                rect,
            );
            let idle_secs = self.playback.overlay_last_activity.elapsed().as_secs_f64();
            let menu_open = self.playback.player_menu.is_some();
            let context_menu_open = self.playback.player_context_menu.is_some();
            let scrubbing = self.playback.seek_drag.is_some()
                || ui.input(|i| {
                    i.pointer.any_down() && i.pointer.hover_pos().is_some_and(|p| rect.contains(p))
                });
            let visible = overlay::overlay_visible_with_state(
                pointer_keeps_visible,
                playback.paused,
                playback.ended,
                menu_open || context_menu_open,
                scrubbing,
                idle_secs,
            );
            let style = self.overlay_style();
            let active_subtitle_text = if self.watch.extras.active_subtitle_url.is_some()
                && !self.watch.extras.active_subtitle_cues.is_empty()
            {
                let pos_ms = (playback.position.max(0.0) * 1000.0).round() as u64;
                transcript::active_cue_index(&self.watch.extras.active_subtitle_cues, pos_ms)
                    .map(|idx| self.watch.extras.active_subtitle_cues[idx].text.clone())
            } else {
                None
            };
            let sb = &self.settings.prefs.value.sponsorblock;
            let sponsor_segments = if sb.enabled && sb.show_timeline_markers && !playback.is_live {
                self.watch
                    .sponsorblock
                    .filtered_segments(sb)
                    .map(|seg| overlay::OverlaySponsorSegment {
                        start_secs: seg.start_secs,
                        end_secs: seg.end_secs,
                        category_label: seg.category.label_pt(),
                        color: seg.category.color(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let active_sponsor_segment = if sb.enabled
                && sb.skip_mode == sponsorblock::SponsorSkipMode::ManualButton
                && !playback.is_live
            {
                self.watch
                    .sponsorblock
                    .segment_at(playback.position, sb)
                    .map(|seg| overlay::OverlaySponsorSegment {
                        start_secs: seg.start_secs,
                        end_secs: seg.end_secs,
                        category_label: seg.category.label_pt(),
                        color: seg.category.color(),
                    })
            } else {
                None
            };
            let sponsor_skip_toast = if sb.show_skip_toast {
                self.watch
                    .sponsorblock
                    .active_skip_toast(playback.now)
                    .map(str::to_owned)
            } else {
                None
            };
            let (manifest_audio_tracks, overlay_qualities) = {
                let active_manifest = self.playback.current_manifest.as_ref().filter(|m| {
                    !m.is_expired()
                        && !m.is_live
                        && history::youtube_video_id(&playback.video.url).as_deref()
                            == Some(m.video_id.as_str())
                });
                let audio = active_manifest
                    .map(|m| {
                        manifest_overlay_audio_tracks(
                            m,
                            self.playback.preferred_audio_lang.as_deref(),
                        )
                    })
                    .filter(|tracks| !tracks.is_empty());
                let qualities = active_manifest
                    .map(|m| m.available_heights())
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| playback.qualities.clone());
                (audio, qualities)
            };
            if let Some(ref tracks) = manifest_audio_tracks {
                self.playback.audio_tracks = tracks.clone();
                self.playback.audio_tracks_video_url = Some(playback.video.url.clone());
            }
            #[cfg(windows)]
            if manifest_audio_tracks.is_none() {
                let prev_menu_id = egui::Id::new("watch_audio_tracks_prev_menu");
                let prev_menu = ctx
                    .data(|d| d.get_temp::<Option<overlay::OverlayMenu>>(prev_menu_id))
                    .flatten();
                let video_changed = self.playback.audio_tracks_video_url.as_deref()
                    != Some(playback.video.url.as_str());
                if video_changed {
                    self.playback.audio_tracks.clear();
                    self.playback.audio_tracks_video_url = Some(playback.video.url.clone());
                    self.playback.audio_tracks_last_poll = None;
                }
                let needs_audio_poll = should_poll_audio_tracks(
                    video_changed,
                    self.playback.current_file_loaded,
                    self.playback.player_menu,
                    prev_menu,
                    self.playback.audio_tracks.is_empty(),
                    self.playback.audio_tracks_last_poll.map(|t| t.elapsed()),
                );
                if prev_menu != self.playback.player_menu {
                    ctx.data_mut(|d| d.insert_temp(prev_menu_id, self.playback.player_menu));
                }
                if needs_audio_poll && let Some(player) = &self.player {
                    let tracks = player
                        .audio_tracks()
                        .into_iter()
                        .map(|t| overlay::OverlayAudioTrack {
                            id: t.id,
                            label: t.label,
                            selected: t.selected,
                        })
                        .collect();
                    self.playback.audio_tracks = tracks;
                    self.playback.audio_tracks_video_url = Some(playback.video.url.clone());
                    self.playback.audio_tracks_last_poll = Some(Instant::now());
                }
            }
            let hover_pointer =
                ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()));
            let storyboard_preview = if !playback.is_live
                && self.playback.storyboard_video_url.as_deref()
                    == Some(playback.video.url.as_str())
                && let Some(hover_secs) =
                    overlay::hover_preview_time_secs(rect, playback.duration, hover_pointer)
                && let Some((sheet_url, uv, aspect_ratio)) =
                    self.playback.storyboard.as_ref().and_then(|spec| {
                        let aspect = if spec.thumb_width > 0 && spec.thumb_height > 0 {
                            spec.thumb_width as f32 / spec.thumb_height as f32
                        } else {
                            16.0 / 9.0
                        };
                        spec.lookup(hover_secs)
                            .map(|(url, uv)| (url.to_owned(), uv, aspect))
                    })
                && let Some((texture_id, _size)) =
                    self.thumbnails.resolve_storyboard_texture(ctx, &sheet_url)
            {
                Some(overlay::OverlayStoryboardFrame {
                    texture_id,
                    uv,
                    aspect_ratio,
                })
            } else {
                None
            };
            let (sleep_timer_option, sleep_timer_remaining_secs) =
                Self::current_sleep_timer_overlay_state(ctx, playback.now);
            let seek_ripple = Self::active_seek_ripple_overlay_state(ctx);
            let up_next = self.resolve_up_next_overlay_info(ctx, playback);
            let telemetry = overlay::OverlayTelemetry {
                pt: self.ui_pt(),
                title: playback.video.title.clone(),
                paused: playback.paused,
                ended: playback.ended,
                muted: playback.muted,
                position_secs: playback.position,
                duration_secs: playback.duration,
                buffered_secs: playback.buffered_secs,
                volume_percent: playback.raw_volume as f32,
                speed: playback.speed,
                seek_step_secs: self.seek_step_secs(),
                current_height: playback.current_height,
                selected_quality: playback.selected_quality,
                qualities: overlay_qualities,
                qualities_loading: playback.qualities_loading,
                has_previous: playback.has_previous,
                has_queued: playback.has_queued,
                has_active_subtitle: playback.has_active_subtitle,
                active_subtitle_text,
                subtitles: playback.subtitles.clone(),
                subtitles_loading: playback.subtitles_loading,
                audio_tracks: if self.playback.player_menu.is_some()
                    || manifest_audio_tracks.is_some()
                {
                    self.playback.audio_tracks.clone()
                } else {
                    Vec::new()
                },
                audio_eq: self.playback.audio_eq,
                brightness: self.playback.brightness,
                contrast: self.playback.contrast,
                saturation: self.playback.saturation,
                storyboard_preview,
                chapters: playback.chapters.clone(),
                sponsor_segments,
                active_sponsor_segment,
                sponsor_skip_toast,
                theater: playback.theater,
                fullscreen: playback.fullscreen,
                mini_player: playback.mini_player,
                autoplay: playback.autoplay,
                loop_video: playback.loop_video,
                loop_a_secs: self.playback.loop_a_secs,
                loop_b_secs: self.playback.loop_b_secs,
                audio_only: self.playback.audio_only,
                is_live: playback.is_live,
                is_live_dvr: playback.is_live_dvr,
                at_live_edge: playback.at_live_edge,
                live_behind_secs: playback.live_behind_secs,
                menu: self.playback.player_menu,
                stable_volume: self.settings.prefs.value.audio_normalization,
                ambient_mode: self.settings.prefs.value.ambient_mode,
                show_remaining_time: self.settings.prefs.value.show_remaining_time,
                caption_style: self.settings.prefs.value.caption_style.clone(),
                ultrawide_zoom_mode: self.settings.prefs.value.ultrawide_zoom_mode,
                ultrawide_custom_zoom: self.settings.prefs.value.ultrawide_custom_zoom,
                sleep_timer_option,
                sleep_timer_remaining_secs,
                temp_speed_2x_active: Self::is_hold_speed_boost_active(ctx),
                seek_ripple,
                up_next,
                shorts_mode: self.is_watch_short_mode()
                    && !playback.fullscreen
                    && !playback.theater
                    && !playback.mini_player,
            };
            let live_info_for_video = self.watch.live.info.as_ref().filter(|_| {
                self.watch.live.video_url.as_deref() == Some(playback.video.url.as_str())
            });
            let live_rev = if live_info_for_video.is_some() {
                self.watch.live.info_revision
            } else {
                0
            };
            let heatmap_points =
                resolve_watch_heatmap_points(&mut self.watch.extras, live_info_for_video, live_rev);
            let (action, overlay_hit) = overlay::show_with_heatmap_hit_state(
                ui,
                rect,
                &telemetry,
                heatmap_points,
                &style,
                visible,
            );
            let mut over_bar = overlay_hit.over_controls;
            let mut over_volume = overlay_hit.over_volume;
            if self.is_watch_short_mode()
                && !playback.fullscreen
                && !playback.theater
                && !playback.mini_player
                && let Some(pos) = ctx.input(|i| i.pointer.hover_pos())
            {
                let block_rects = ctx.data(|d| {
                    d.get_temp::<Vec<egui::Rect>>(egui::Id::new(
                        "fasttube_shorts_overlay_block_rects",
                    ))
                });
                if block_rects
                    .as_ref()
                    .is_some_and(|rects| rects.iter().any(|r| r.contains(pos)))
                {
                    over_bar = true;
                    over_volume = false;
                }
            }
            let mut over_stats = false;
            if self.playback.stats_for_nerds_open {
                self.refresh_stats_for_nerds(false);
                let stats_data = self.build_stats_for_nerds_data(rect, playback, ctx);
                let (stats_action, pointer_over_stats) =
                    overlay::show_stats_for_nerds(ui, rect, &stats_data, &style);
                over_stats = pointer_over_stats;
                over_bar |= over_stats;
                if over_stats {
                    over_volume = false;
                }
                if let Some(act) = stats_action {
                    self.playback.overlay_last_activity = Instant::now();
                    match act {
                        overlay::StatsForNerdsAction::CopyDebugInfo => {
                            self.handle_player_context_action(
                                overlay::PlayerContextAction::CopyDebugInfo,
                                playback,
                                rect,
                                ctx,
                            );
                        }
                        overlay::StatsForNerdsAction::Close => {
                            self.playback.stats_for_nerds_open = false;
                        }
                    }
                }
            }

            let secondary_click_pos = ui.input(|i| {
                i.pointer
                    .button_clicked(egui::PointerButton::Secondary)
                    .then(|| i.pointer.interact_pos().or_else(|| i.pointer.hover_pos()))
                    .flatten()
            });
            let mut just_opened_context_menu = false;
            if let Some(click_pos) = secondary_click_pos {
                if rect.contains(click_pos) && !self.autoscroll.is_active() {
                    self.playback.player_context_menu = Some(click_pos);
                    self.playback.player_menu = None;
                    self.playback.overlay_last_activity = Instant::now();
                    just_opened_context_menu = true;
                } else {
                    self.playback.player_context_menu = None;
                }
            }

            if let Some(anchor) = self.playback.player_context_menu {
                let menu_state = overlay::PlayerContextMenuState {
                    pt: self.ui_pt(),
                    is_live: playback.is_live,
                    loop_video: playback.loop_video,
                    mini_player: playback.mini_player,
                    can_copy_embed: history::youtube_video_id(&playback.video.url).is_some(),
                    stats_for_nerds_open: self.playback.stats_for_nerds_open,
                };
                let (ctx_action, menu_rect) =
                    overlay::show_context_menu(ctx, anchor, &menu_state, &style);
                if pointer_pos.is_some_and(|p| menu_rect.contains(p)) {
                    over_bar = true;
                    over_volume = false;
                }
                if let Some(act) = ctx_action {
                    self.playback.player_context_menu = None;
                    self.playback.overlay_last_activity = Instant::now();
                    self.handle_player_context_action(act, playback, rect, ctx);
                } else if was_context_menu_open
                    && !just_opened_context_menu
                    && ui.input(|i| {
                        i.pointer.button_released(egui::PointerButton::Primary)
                            && i.pointer
                                .interact_pos()
                                .or_else(|| i.pointer.hover_pos())
                                .is_some_and(|p| !menu_rect.contains(p))
                    })
                {
                    self.playback.player_context_menu = None;
                    self.playback.overlay_last_activity = Instant::now();
                }
            }

            let mut hold_consumed_click = false;
            if !playback.is_live && !was_context_menu_open && !just_opened_context_menu {
                hold_consumed_click = self
                    .update_hold_speed_boost(ctx, rect, &response, playback, over_bar, menu_open);
            }

            // Mouse-wheel volume only when the cursor is over the volume control area.
            let wheel_y = ui.input(|i| i.smooth_scroll_delta.y);
            let can_handle_wheel_volume = !was_context_menu_open
                && !just_opened_context_menu
                && !menu_open
                && !self.autoscroll.is_active();
            let scroll_guard_id = egui::Id::new("fasttube_volume_wheel_scroll_guard");
            let mut scroll_guard = ctx
                .data(|d| d.get_temp::<VolumeWheelScrollGuard>(scroll_guard_id))
                .unwrap_or_default();
            if can_handle_wheel_volume {
                let over_volume_target =
                    over_volume && pointer_pos.is_some_and(|p| rect.contains(p));
                let has_vertical_wheel = wheel_y.is_finite() && wheel_y.abs() >= 0.5;
                if scroll_guard.allow_volume_wheel(
                    over_volume_target,
                    pointer_pos,
                    has_vertical_wheel,
                    playback.now,
                ) {
                    if wheel_y.is_finite() && wheel_y != 0.0 {
                        ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
                    }
                    if let Some(next_vol) = compute_wheel_volume_target(
                        playback.raw_volume as f32,
                        playback.muted,
                        wheel_y,
                        5.0,
                    ) {
                        if playback.muted && next_vol > 0.0 {
                            self.handle_player_overlay_action(
                                overlay::OverlayAction::ToggleMute,
                                playback,
                                ctx,
                            );
                        }
                        overlay_action = Some(overlay::OverlayAction::SetVolume(next_vol));
                        self.playback.overlay_last_activity = Instant::now();
                    }
                }
            }
            ctx.data_mut(|d| d.insert_temp(scroll_guard_id, scroll_guard));

            let click_debounce_id = egui::Id::new("fasttube_player_click_debounce");
            let mut click_debounce = ctx
                .data(|d| d.get_temp::<PlayerClickDebounceState>(click_debounce_id))
                .unwrap_or_default();

            if let Some(action) = action
                && !was_context_menu_open
                && !just_opened_context_menu
            {
                click_debounce.cancel();
                self.playback.overlay_last_activity = Instant::now();
                overlay_action = Some(action);
            } else if !was_context_menu_open
                && !just_opened_context_menu
                && self.playback.player_context_menu.is_none()
                && !over_bar
                && !hold_consumed_click
            {
                if response.double_clicked() {
                    self.playback.overlay_last_activity = Instant::now();
                    let click_x = response
                        .interact_pointer_pos()
                        .or(pointer_pos)
                        .map_or(rect.center().x, |p| p.x);
                    let zone = classify_player_click_zone(click_x, rect);
                    match click_debounce.on_double_click(zone) {
                        PlayerPointerGesture::SeekBack10
                            if !playback.is_live || playback.is_live_dvr =>
                        {
                            Self::record_seek_ripple(ctx, PlayerClickZone::Left);
                            overlay_action = Some(overlay::OverlayAction::SeekBack10);
                        }
                        PlayerPointerGesture::SeekForward10
                            if !playback.is_live || playback.is_live_dvr =>
                        {
                            Self::record_seek_ripple(ctx, PlayerClickZone::Right);
                            overlay_action = Some(overlay::OverlayAction::SeekForward10);
                        }
                        _ => {
                            overlay_action = Some(overlay::OverlayAction::ToggleFullscreen);
                        }
                    }
                } else if playback.mini_player
                    && !menu_open
                    && self.playback.seek_drag.is_none()
                    && response.drag_started_by(egui::PointerButton::Primary)
                {
                    click_debounce.cancel();
                    self.playback.overlay_last_activity = Instant::now();
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                } else if response.clicked() {
                    self.playback.overlay_last_activity = Instant::now();
                    if menu_open {
                        click_debounce.cancel();
                        overlay_action = Some(overlay::OverlayAction::SetMenu(None));
                    } else if telemetry.shorts_mode {
                        click_debounce.cancel();
                        overlay_action = Some(overlay::OverlayAction::TogglePlay);
                    } else {
                        click_debounce.on_single_click(playback.now);
                        ctx.request_repaint_after(DOUBLE_CLICK_DEBOUNCE_WINDOW);
                    }
                }
            }

            if overlay_action.is_none()
                && matches!(
                    click_debounce.poll_ready_single_click(playback.now),
                    PlayerPointerGesture::TogglePlay
                )
            {
                overlay_action = Some(overlay::OverlayAction::TogglePlay);
            }
            if click_debounce.is_pending() {
                ctx.request_repaint_after(Duration::from_millis(24));
            }
            ctx.data_mut(|d| d.insert_temp(click_debounce_id, click_debounce));

            let effective_visible = visible || self.playback.player_context_menu.is_some();
            let hide_fullscreen_cursor = playback.fullscreen
                && !effective_visible
                && !over_stats
                && !self.autoscroll.is_active()
                && !self.ui.shortcuts_open
                && !self.ui.command_palette.open
                && !self.ui.settings_open
                && pointer_pos.is_some_and(|p| rect.contains(p));
            if hide_fullscreen_cursor {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            }
        }
        overlay_action
    }

    pub(crate) fn handle_player_overlay_action(
        &mut self,
        action: overlay::OverlayAction,
        playback: &OverlayPlaybackContext,
        ctx: &egui::Context,
    ) {
        use overlay::OverlayAction;
        match action {
            OverlayAction::GoLive => {
                self.go_live_edge(&playback.video);
            }
            OverlayAction::TogglePlay => {
                if playback.ended && playback.is_live {
                    self.go_live_edge(&playback.video);
                    return;
                }
                #[cfg(windows)]
                {
                    let result = if playback.ended {
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
                            .map(|player| {
                                player.set("pause", if playback.paused { "no" } else { "yes" })
                            })
                            .unwrap_or(Ok(()))
                    };
                    match result {
                        Ok(()) => {
                            let next_paused = if playback.ended {
                                false
                            } else {
                                !playback.paused
                            };
                            Self::record_player_osd(
                                ctx,
                                if next_paused {
                                    "\u{23f8} Pausado"
                                } else {
                                    "\u{25b6} Reproduzindo"
                                },
                            );
                            Self::update_player_osd_snapshot_state(
                                ctx,
                                &playback.video.url,
                                |snap| {
                                    snap.paused = next_paused;
                                },
                            );
                            if playback.ended {
                                self.record_manual_seek(0.0, playback.duration, playback.now);
                            } else if !playback.paused {
                                self.flush_playback_history_at(playback.now);
                            }
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
            OverlayAction::SeekBack10 => {
                if playback.is_live && !playback.is_live_dvr {
                    return;
                }
                let step_u32 = self.seek_step_secs();
                let step = f64::from(step_u32);
                #[cfg(windows)]
                {
                    let result = self
                        .player
                        .as_ref()
                        .map(|player| player.seek(-step, "relative"))
                        .unwrap_or(Ok(()));
                    match result {
                        Ok(()) => {
                            Self::record_player_osd(ctx, format!("\u{23ea} -{step_u32}s"));
                            self.record_manual_seek(
                                (playback.position - step).max(0.0),
                                playback.duration,
                                playback.now,
                            );
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
            OverlayAction::SeekForward10 => {
                if playback.is_live && !playback.is_live_dvr {
                    return;
                }
                let step_u32 = self.seek_step_secs();
                let step = f64::from(step_u32);
                if playback.is_live
                    && let Some(dur) = playback.duration
                    && (dur <= 1.5 || playback.position + step >= (dur - 1.5).max(0.0))
                {
                    self.go_live_edge(&playback.video);
                    return;
                }
                #[cfg(windows)]
                {
                    let target = if playback.is_live {
                        playback.duration.map_or(playback.position + step, |dur| {
                            (playback.position + step).min((dur - 1.5).max(0.0))
                        })
                    } else {
                        playback.duration.map_or(playback.position + step, |dur| {
                            (playback.position + step).min(dur)
                        })
                    };
                    let result = self
                        .player
                        .as_ref()
                        .map(|player| {
                            if playback.is_live {
                                player.seek(target, "absolute+exact")
                            } else {
                                player.seek(step, "relative")
                            }
                        })
                        .unwrap_or(Ok(()));
                    match result {
                        Ok(()) => {
                            Self::record_player_osd(ctx, format!("\u{23e9} +{step_u32}s"));
                            self.record_manual_seek(target, playback.duration, playback.now);
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
            OverlayAction::SeekTo(target) => {
                if playback.is_live && !playback.is_live_dvr {
                    return;
                }
                if playback.is_live
                    && let Some(dur) = playback.duration
                    && dur > 0.0
                    && (dur <= 1.5 || (target / dur) >= 0.985 || target >= (dur - 1.5).max(0.0))
                {
                    self.go_live_edge(&playback.video);
                    return;
                }
                let clamped = if playback.is_live {
                    playback.duration.map_or(target.max(0.0), |dur| {
                        target.max(0.0).min((dur - 1.5).max(0.0))
                    })
                } else {
                    target.max(0.0)
                };
                #[cfg(windows)]
                {
                    let result = self
                        .player
                        .as_ref()
                        .map(|player| player.seek(clamped, "absolute+exact"))
                        .unwrap_or(Ok(()));
                    match result {
                        Ok(()) => {
                            let icon = if clamped >= playback.position {
                                "\u{23e9}"
                            } else {
                                "\u{23ea}"
                            };
                            Self::record_player_osd(
                                ctx,
                                format!("{icon} {}", format_time(clamped)),
                            );
                            self.record_manual_seek(clamped, playback.duration, playback.now);
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
            OverlayAction::SkipSponsorSegment(target) => {
                if playback.is_live {
                    return;
                }
                let clamped = playback.duration.map_or(target.max(0.0), |dur| {
                    target.max(0.0).min((dur - 0.1).max(0.0))
                });
                #[cfg(windows)]
                {
                    let result = self
                        .player
                        .as_ref()
                        .map(|player| player.seek(clamped, "absolute+exact"))
                        .unwrap_or(Ok(()));
                    match result {
                        Ok(()) => {
                            self.record_manual_seek(clamped, playback.duration, playback.now);
                            self.record_manual_sponsor_skip(
                                playback.position,
                                clamped,
                                playback.now,
                            );
                        }
                        Err(error) => self.message = error,
                    }
                }
                #[cfg(not(windows))]
                {
                    self.record_manual_seek(clamped, playback.duration, playback.now);
                    self.record_manual_sponsor_skip(playback.position, clamped, playback.now);
                }
            }
            OverlayAction::ToggleMute => {
                #[cfg(windows)]
                {
                    let result = self
                        .player
                        .as_ref()
                        .map(|player| player.set("mute", if playback.muted { "no" } else { "yes" }))
                        .unwrap_or(Ok(()));
                    match result {
                        Ok(()) => {
                            let next_muted = !playback.muted;
                            let vol_rounded = playback.raw_volume.round() as i32;
                            Self::record_player_osd(
                                ctx,
                                if next_muted || vol_rounded == 0 {
                                    "\u{1f507} Mudo".to_owned()
                                } else {
                                    format!("\u{1f50a} {vol_rounded}%")
                                },
                            );
                            Self::update_player_osd_snapshot_state(
                                ctx,
                                &playback.video.url,
                                |snap| {
                                    snap.muted = next_muted;
                                },
                            );
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
            OverlayAction::SetVolume(volume) => {
                #[cfg(windows)]
                if let Some(player) = &self.player
                    && let Err(error) = Self::apply_mpv_volume_with_osd(
                        player,
                        ctx,
                        Some(&playback.video.url),
                        f64::from(volume),
                    )
                {
                    self.message = error;
                }
            }
            OverlayAction::Previous => {
                self.trigger_previous_video_action(playback.is_live, playback.position, false);
            }
            OverlayAction::Next => {
                self.trigger_next_video_action(false);
            }
            OverlayAction::ToggleAutoplay => {
                if !playback.is_live {
                    self.playback.autoplay = !self.playback.autoplay;
                    self.playback.autoplay_eof_handled = false;
                }
            }
            OverlayAction::ToggleLoop => {
                if !playback.is_live {
                    self.playback.loop_video = !self.playback.loop_video;
                    #[cfg(windows)]
                    if let Some(player) = &self.player
                        && let Err(error) = player.set(
                            "loop-file",
                            if self.playback.loop_video {
                                "inf"
                            } else {
                                "no"
                            },
                        )
                    {
                        self.message = error;
                    }
                }
            }
            OverlayAction::ToggleAudioOnly => {
                self.toggle_audio_only_mode();
            }
            OverlayAction::ToggleCaptions => self.toggle_player_captions(ctx),
            OverlayAction::SelectSubtitle(id) => {
                self.select_overlay_subtitle(id.as_deref(), ctx);
            }
            OverlayAction::SelectAudioTrack(id) => {
                if !id.is_empty() && !id.bytes().all(|b| b.is_ascii_digit()) {
                    self.playback.preferred_audio_lang = Some(id.clone());
                }
                #[cfg(windows)]
                if let Some(player) = &self.player {
                    let switched_direct_url = if let Some(ref manifest) =
                        self.playback.current_manifest
                        && !manifest.is_expired()
                        && !manifest.is_live
                        && history::youtube_video_id(&playback.video.url).as_deref()
                            == Some(manifest.video_id.as_str())
                        && let Some(pair) = crate::stream_manifest::select_stream_pair(
                            manifest,
                            self.settings.playback.value.max_height,
                            self.playback.quality,
                            self.settings.prefs.value.video_codec_preference,
                            Some(id.as_str()),
                            self.playback.audio_only,
                        ) {
                        player
                            .switch_direct_stream(
                                &pair.to_direct_stream_options(None, self.playback.audio_only),
                            )
                            .is_ok()
                            .then_some(pair.video_url)
                    } else {
                        None
                    };
                    if let Some(direct_url) = switched_direct_url {
                        self.playback.preferred_audio_lang = Some(id.clone());
                        self.playback.active_direct_stream_url = Some(direct_url);
                        for track in &mut self.playback.audio_tracks {
                            track.selected = track.id.eq_ignore_ascii_case(&id);
                        }
                        self.playback.audio_tracks_last_poll = None;
                        self.message.clear();
                    } else if let Err(error) = player.set_audio_track(&id) {
                        self.message = error;
                    } else {
                        for track in &mut self.playback.audio_tracks {
                            track.selected = track.id == id;
                        }
                        self.playback.audio_tracks_last_poll = None;
                    }
                }
            }
            OverlayAction::SetAudioEq(preset) => {
                self.playback.audio_eq = preset;
                #[cfg(windows)]
                if let Some(player) = &self.player
                    && let Err(error) = player
                        .apply_audio_filters(self.settings.prefs.value.audio_normalization, preset)
                {
                    self.message = error;
                }
            }
            OverlayAction::SetVideoEq {
                brightness,
                contrast,
                saturation,
            } => {
                self.playback.brightness = brightness.clamp(-50, 50);
                self.playback.contrast = contrast.clamp(-50, 50);
                self.playback.saturation = saturation.clamp(-50, 50);
                #[cfg(windows)]
                if let Some(player) = &self.player
                    && let Err(error) = player.set_video_eq(
                        self.playback.brightness,
                        self.playback.contrast,
                        self.playback.saturation,
                    )
                {
                    self.message = error;
                }
            }
            OverlayAction::ResetVideoEq => {
                self.reset_video_and_audio_eq();
            }
            OverlayAction::SetSpeed(speed) => {
                let clamped = speed.clamp(0.25, 4.0);
                self.apply_and_remember_playback_speed(clamped);
                Self::record_player_osd(ctx, format!("\u{26a1} {clamped:.2}x"));
                #[cfg(windows)]
                Self::update_player_osd_snapshot_state(ctx, &playback.video.url, |snap| {
                    snap.speed_hundredths = (clamped * 100.0).round() as i32;
                });
            }
            OverlayAction::SelectQuality(quality) => {
                #[cfg(windows)]
                self.apply_player_quality(
                    &playback.video,
                    quality,
                    playback.position,
                    playback.raw_path.as_deref(),
                    playback.raw_position,
                );
                #[cfg(not(windows))]
                let _ = quality;
            }
            OverlayAction::SeekChapter(target) => {
                #[cfg(windows)]
                {
                    let result = self
                        .player
                        .as_ref()
                        .map(|player| player.seek(target, "absolute"))
                        .unwrap_or(Ok(()));
                    match result {
                        Ok(()) => {
                            let icon = if target >= playback.position {
                                "\u{23e9}"
                            } else {
                                "\u{23ea}"
                            };
                            Self::record_player_osd(ctx, format!("{icon} {}", format_time(target)));
                            self.record_manual_seek(target, playback.duration, playback.now);
                        }
                        Err(error) => self.message = error,
                    }
                }
                #[cfg(not(windows))]
                let _ = target;
            }
            OverlayAction::OpenAppSettings => {
                if self.playback.fullscreen {
                    self.toggle_fullscreen(ctx);
                }
                if self.playback.mini_player {
                    self.toggle_mini_player(ctx);
                }
                self.playback.player_menu = None;
                self.ui.settings_open = true;
                self.sync_player_window_visibility();
            }
            OverlayAction::SetMenu(menu) => {
                let has_manifest_heights =
                    self.playback.current_manifest.as_ref().is_some_and(|m| {
                        !m.is_expired()
                            && !m.is_live
                            && history::youtube_video_id(&playback.video.url).as_deref()
                                == Some(m.video_id.as_str())
                            && !m.available_heights().is_empty()
                    });
                if menu == Some(overlay::OverlayMenu::Quality)
                    && self.playback.qualities.is_empty()
                    && !has_manifest_heights
                    && !self.playback.quality_task.is_running()
                    && let Some(url) = self.playback.quality_fallback_url.take()
                {
                    self.start_quality_metadata_fallback(url, ctx);
                }
                if menu == Some(overlay::OverlayMenu::Subtitles)
                    && !self.watch.extras.subtitles_loaded
                    && self.watch.extras.subtitles_loading.is_none()
                    && !self.is_playback_metadata_loading()
                {
                    self.watch.extras.start_subtitles(ctx);
                }
                self.playback.player_menu = menu;
            }
            OverlayAction::ToggleMiniPlayer => self.toggle_mini_player(ctx),
            OverlayAction::ToggleTheater => self.toggle_theater_mode(),
            OverlayAction::ToggleFullscreen => self.toggle_fullscreen(ctx),
            OverlayAction::ToggleRemainingTime => {
                let mut next = self.settings.prefs.value.clone();
                next.show_remaining_time = !next.show_remaining_time;
                self.settings.apply_app_preferences(next);
            }
            OverlayAction::ToggleStableVolume => self.toggle_stable_volume(),
            OverlayAction::ToggleAmbientMode => self.toggle_ambient_mode(),
            OverlayAction::SetSleepTimer(option) => match option {
                None => self.set_sleep_timer_minutes(ctx, None, false),
                Some(overlay::SleepTimerOption::Minutes(mins)) => {
                    self.set_sleep_timer_minutes(ctx, Some(mins), false);
                }
                Some(overlay::SleepTimerOption::EndOfVideo) => {
                    self.set_sleep_timer_minutes(ctx, None, true);
                }
            },
            OverlayAction::SetCaptionStyle(style) => {
                let mut next = self.settings.prefs.value.clone();
                next.caption_style = style.normalized();
                self.settings.apply_app_preferences(next);
            }
            OverlayAction::SetCaptionAutoTranslate(lang) => {
                let mut next = self.settings.prefs.value.clone();
                next.caption_style.translate_lang = lang;
                self.settings.apply_app_preferences(next);
            }
            OverlayAction::SetUltrawideZoom(mode, custom_zoom) => {
                let mut next = self.settings.prefs.value.clone();
                next.ultrawide_zoom_mode = mode;
                next.ultrawide_custom_zoom = custom_zoom.clamp(0.0, 0.5);
                self.settings.apply_app_preferences(next);
                let (panscan, zoom) = match mode {
                    settings::UltrawideZoomMode::Fit => (0.0, 0.0),
                    settings::UltrawideZoomMode::Crop21x9 => (1.0, 0.0),
                    settings::UltrawideZoomMode::Custom => (0.0, custom_zoom.clamp(0.0, 0.5)),
                };
                self.apply_ultrawide_zoom(panscan, zoom);
            }
            OverlayAction::PlayUpNextNow => self.play_up_next_now(ctx),
            OverlayAction::CancelUpNext => self.cancel_up_next_countdown(ctx),
            OverlayAction::Close => self.close_video(ctx),
        }
    }
}
