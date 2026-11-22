use super::*;
use eframe::egui::RichText;

impl FastTube {
    pub(crate) fn toggle_mini_player(&mut self, ctx: &egui::Context) {
        let (inner_rect, outer_rect, maximized, monitor_size) = ctx.input(|input| {
            let vp = input.viewport();
            (vp.inner_rect, vp.outer_rect, vp.maximized, vp.monitor_size)
        });
        if !self.playback.mini_player {
            self.playback.in_app_mini_player = false;
            self.playback.player_context_menu = None;
            let was_fullscreen = self.playback.fullscreen;
            let fullscreen_restore_maximized =
                std::mem::take(&mut self.playback.fullscreen_restore_maximized);
            if was_fullscreen {
                self.playback.fullscreen = false;
                self.playback.fullscreen_sync_retries = 0;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
            }
            let was_maximized = maximized.unwrap_or(false) || fullscreen_restore_maximized;
            self.playback.mini_player_restore_maximized = was_maximized;
            if !was_maximized && !was_fullscreen {
                self.playback.mini_player_restore_size = inner_rect.map(|rect| rect.size());
                self.playback.mini_player_restore_pos = outer_rect.map(|rect| rect.min);
            }
            let target_size = self
                .playback
                .mini_player_saved_size
                .filter(|size| size.x >= 260.0 && size.y >= 146.0)
                .unwrap_or_else(|| egui::vec2(480.0, 270.0));
            let target_pos = self.playback.mini_player_saved_pos.or_else(|| {
                monitor_size.map(|mon| {
                    egui::pos2(
                        (mon.x - target_size.x - 32.0).max(16.0),
                        (mon.y - target_size.y - 72.0).max(16.0),
                    )
                })
            });
            self.playback.mini_player = true;
            self.playback.mini_player_settled = false;
            self.playback.mini_player_pending_enter = Some(app_state::MiniPlayerPendingApply {
                target_size,
                target_pos,
                frames_left: 6,
            });
            // Clear maximized state before resizing; on Windows, InnerSize on WS_MAXIMIZE locks the window at top-left.
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                260.0, 146.0,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));
            if let Some(pos) = target_pos {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
        } else {
            if self.playback.mini_player_settled && maximized != Some(true) {
                if let Some(size) = inner_rect
                    .map(|rect| rect.size())
                    .filter(|size| size.x >= 260.0 && size.y >= 146.0)
                {
                    self.playback.mini_player_saved_size = Some(size);
                }
                if let Some(pos) = outer_rect.map(|rect| rect.min) {
                    self.playback.mini_player_saved_pos = Some(pos);
                }
            }
            self.playback.mini_player = false;
            self.playback.player_context_menu = None;
            self.playback.mini_player_pending_enter = None;
            self.playback.mini_player_settled = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                480.0, 420.0,
            )));
            if self.playback.mini_player_restore_maximized {
                self.playback.mini_player_restore_maximized = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            } else {
                let restore_size = self
                    .playback
                    .mini_player_restore_size
                    .take()
                    .unwrap_or_else(|| egui::vec2(1280.0, 800.0));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(restore_size));
                if let Some(pos) = self.playback.mini_player_restore_pos.take() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                }
            }
        }
        self.sync_player_window_visibility();
    }

    pub(crate) fn sync_mini_player_viewport(&mut self, ctx: &egui::Context) {
        if !self.playback.mini_player {
            return;
        }
        let (is_maximized, is_fullscreen, inner_rect, outer_rect) = ctx.input(|input| {
            let vp = input.viewport();
            (
                vp.maximized == Some(true),
                vp.fullscreen == Some(true),
                vp.inner_rect,
                vp.outer_rect,
            )
        });
        if let Some(mut pending) = self.playback.mini_player_pending_enter {
            if (is_fullscreen || is_maximized) && pending.frames_left > 0 {
                if is_fullscreen {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                }
                if is_maximized {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                }
                pending.frames_left -= 1;
                self.playback.mini_player_pending_enter = Some(pending);
                ctx.request_repaint();
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                260.0, 146.0,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(pending.target_size));
            if let Some(pos) = pending.target_pos {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
            self.playback.mini_player_pending_enter = None;
            self.playback.mini_player_settled = true;
            return;
        }
        if is_maximized {
            // Maximizing (or Aero-snapping to top) exits PiP gracefully instead of leaving a huge chromeless window.
            self.playback.mini_player = false;
            self.playback.mini_player_settled = false;
            self.playback.mini_player_restore_maximized = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                480.0, 420.0,
            )));
            self.sync_player_window_visibility();
            return;
        }
        if let Some(size) = inner_rect
            .map(|rect| rect.size())
            .filter(|size| size.x >= 260.0 && size.y >= 146.0 && size.x < 1200.0)
        {
            self.playback.mini_player_saved_size = Some(size);
        }
        if let Some(pos) = outer_rect.map(|rect| rect.min) {
            self.playback.mini_player_saved_pos = Some(pos);
        }
    }

    pub(crate) fn compute_mini_player_size(column_width: f32, available_height: f32) -> egui::Vec2 {
        let avail_w = column_width.max(1.0);
        let avail_h = available_height.max(1.0);
        let width = avail_w.min(avail_h * 16.0 / 9.0);
        let height = width * 9.0 / 16.0;
        egui::vec2(width, height)
    }

    fn mini_player_thumbnail_surface(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        size: egui::Vec2,
    ) -> (egui::Rect, bool) {
        let thumb_url = if !video.thumbnail.trim().is_empty() {
            video.thumbnail.clone()
        } else if let Some(vid) = history::youtube_video_id(&video.url) {
            format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg")
        } else {
            String::new()
        };
        self.thumbnails.begin_pass();
        let resp = if !thumb_url.is_empty() {
            self.thumbnails.show_rounded(ui, ctx, &thumb_url, size, 0.0)
        } else {
            let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        self.thumbnails.end_pass(ctx);
        (resp.rect, resp.clicked())
    }

    pub(crate) fn compute_in_app_mini_player_size(viewport_width: f32) -> egui::Vec2 {
        let card_w = (viewport_width * 0.22)
            .clamp(264.0, 336.0)
            .min((viewport_width - 24.0).max(216.0))
            .round();
        let video_h = (card_w * 9.0 / 16.0).round();
        egui::vec2(card_w, video_h)
    }

    pub(crate) fn in_app_mini_player_ui(&mut self, ctx: &egui::Context) {
        let Some(video) = self.selected.clone() else {
            return;
        };
        self.sync_player_window_visibility();
        let palette = self.palette();
        let pt = self.ui_pt();
        let content = ctx.input(|i| i.content_rect());
        let mini_size = Self::compute_in_app_mini_player_size(content.width());
        let card_w = mini_size.x;
        let video_h = mini_size.y;

        #[cfg(windows)]
        let mut can_overlay_hover = self
            .player
            .as_ref()
            .is_none_or(player::Player::is_render_api);
        #[cfg(not(windows))]
        let can_overlay_hover = true;

        let mut expand_clicked = false;
        let mut toggle_pause_clicked = false;
        let mut prev_clicked = false;
        let mut next_clicked = false;
        let mut toggle_mute_clicked = false;
        let mut set_volume_target: Option<f32> = None;
        let mut desktop_pip_clicked = false;
        let mut close_clicked = false;
        let mut seek_target_secs: Option<f64> = None;

        let mut paused = self.playback.mini_player_paused;
        let mut muted = self.playback.mini_player_muted;
        let mut volume_percent = self.playback.mini_player_volume;
        let mut position = self
            .playback
            .active
            .as_ref()
            .map_or(0.0, |a| a.position_ms as f64 / 1000.0);
        let mut duration = self
            .playback
            .active
            .as_ref()
            .and_then(|a| a.duration_ms)
            .filter(|&d| d > 0)
            .map(|d| d as f64 / 1000.0);
        let mut is_live = self.playback.active.as_ref().is_some_and(|a| a.is_live);
        let can_prev_video = self.has_previous_video_target();
        let can_next = self.has_next_video_target();
        let has_queue = !self.queue.data.is_empty();

        egui::Area::new(egui::Id::new("fasttube_in_app_mini_player"))
            .order(egui::Order::Middle)
            .anchor(
                egui::Align2::RIGHT_BOTTOM,
                egui::vec2(
                    -IN_APP_MINI_PLAYER_ANCHOR_OFFSET,
                    -IN_APP_MINI_PLAYER_ANCHOR_OFFSET,
                ),
            )
            .default_size(egui::vec2(
                card_w + 2.0,
                video_h
                    + 2.0
                    + if can_overlay_hover {
                        0.0
                    } else {
                        MINI_PLAYER_FALLBACK_BAR_HEIGHT
                    },
            ))
            .movable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(if can_overlay_hover {
                        Color32::BLACK
                    } else {
                        palette.card_bg
                    })
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(if can_overlay_hover { 0.0 } else { 6.0 })
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 1,
                        color: Color32::from_black_alpha(120),
                    })
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_width(card_w);
                        ui.set_max_width(card_w);
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                        let mut video_rect_opt: Option<egui::Rect> = None;
                        let mut surface_clicked = false;

                        #[cfg(windows)]
                        if self.player.is_some() {
                            let is_render_api = self
                                .player
                                .as_ref()
                                .is_some_and(player::Player::is_render_api);
                            can_overlay_hover = is_render_api;
                            let hide_video = !is_render_api
                                && (self.has_modal_covering_mini_player()
                                    || self.ui.navigation_open);
                            let surface = self.player_surface_and_telemetry(
                                ui,
                                ctx,
                                egui::vec2(card_w, video_h),
                                hide_video,
                            );
                            if let Some((video_area, telemetry)) = surface {
                                for event in telemetry.events.iter().cloned() {
                                    self.handle_queue_player_event(event);
                                }
                                if let Some(error) = telemetry.poll_error.clone() {
                                    self.message = error;
                                    self.report_pending_queue_load_error();
                                }
                                let now = Instant::now();
                                let (pos, dur, pending_seek, clear_start_option) = self
                                    .process_player_telemetry(
                                        &video,
                                        telemetry.raw_path.as_deref(),
                                        telemetry.raw_position,
                                        telemetry.raw_duration,
                                        telemetry.ended,
                                        now,
                                    );
                                self.update_live_playback_state(
                                    telemetry.paused,
                                    telemetry.raw_position,
                                    telemetry.raw_duration,
                                    telemetry.seekable,
                                    now,
                                );
                                if !telemetry.ended
                                    && self.queue.pending_video.as_ref() == Some(&video)
                                    && self.playback.active.as_ref().is_some_and(|active| {
                                        active.has_live_sample
                                            && active.url == video.url
                                            && self.raw_path_matches_video(
                                                telemetry.raw_path.as_deref(),
                                                &video.url,
                                            )
                                    })
                                {
                                    self.report_pending_queue_playback_observed();
                                }
                                if clear_start_option && let Some(player) = &self.player {
                                    let _ = player.set("options/start", "none");
                                }
                                if let Some(target_secs) = pending_seek
                                    && let Some(player) = &self.player
                                    && let Err(error) = player.seek(target_secs, "absolute+exact")
                                {
                                    if let Some(active) = &mut self.playback.active {
                                        active.resume_seek_pending = true;
                                    }
                                    self.message = error;
                                }
                                paused = telemetry.paused;
                                muted = telemetry.muted;
                                volume_percent = telemetry.raw_volume as f32;
                                self.playback.mini_player_paused = paused;
                                self.playback.mini_player_muted = muted;
                                self.playback.mini_player_volume = volume_percent;
                                position = pos;
                                duration = dur;
                                if let Some(active) = &self.playback.active {
                                    is_live = active.is_live;
                                }
                                if let Some((rect, resp)) = video_area {
                                    let resp = if can_overlay_hover {
                                        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                                    } else {
                                        resp
                                    };
                                    video_rect_opt = Some(rect);
                                    surface_clicked = resp.clicked();
                                }
                            }
                        } else {
                            let (rect, clicked) = self.mini_player_thumbnail_surface(
                                ui,
                                ctx,
                                &video,
                                egui::vec2(card_w, video_h),
                            );
                            video_rect_opt = Some(rect);
                            surface_clicked = clicked;
                        }

                        #[cfg(not(windows))]
                        {
                            let (rect, clicked) = self.mini_player_thumbnail_surface(
                                ui,
                                ctx,
                                &video,
                                egui::vec2(card_w, video_h),
                            );
                            video_rect_opt = Some(rect);
                            surface_clicked = clicked;
                        }

                        let can_prev = can_prev_video || (!is_live && position > 1.0);

                        if let Some(vrect) = video_rect_opt
                            && can_overlay_hover
                        {
                            let layout = MiniPlayerOverlayLayout::compute(vrect);
                            let seek_id = egui::Id::new("fasttube_mini_player_seek");
                            let volume_id = egui::Id::new("fasttube_mini_player_volume");
                            let hover_anim_id = egui::Id::new("fasttube_mini_player_hover_t");
                            let dragging_seek = ctx.is_being_dragged(seek_id);
                            let dragging_volume = ctx.is_being_dragged(volume_id);
                            let hovered_video =
                                ui.rect_contains_pointer(vrect) || dragging_seek || dragging_volume;
                            let scrim_t =
                                ctx.animate_bool_with_time(hover_anim_id, hovered_video, 0.14);

                            let mut overlay_ui =
                                ui.new_child(egui::UiBuilder::new().max_rect(vrect));
                            overlay_ui.set_clip_rect(vrect.intersect(ui.clip_rect()));

                            if scrim_t > 0.005 {
                                let vignette_alpha = (34.0 * scrim_t).round() as u8;
                                overlay_ui.painter().rect_filled(
                                    vrect,
                                    0.0,
                                    Color32::from_black_alpha(vignette_alpha),
                                );
                                let top_alpha = (175.0 * scrim_t).round() as u8;
                                overlay::paint_vertical_gradient(
                                    overlay_ui.painter(),
                                    layout.top_scrim,
                                    Color32::from_black_alpha(top_alpha),
                                    Color32::TRANSPARENT,
                                );
                                let bottom_alpha = (190.0 * scrim_t).round() as u8;
                                overlay::paint_vertical_gradient(
                                    overlay_ui.painter(),
                                    layout.bottom_scrim,
                                    Color32::TRANSPARENT,
                                    Color32::from_black_alpha(bottom_alpha),
                                );
                            }

                            let mut consumed_click = false;
                            let mut pointer_over_control = false;
                            let mut seek_hot = false;

                            if !is_live && let Some(dur) = duration.filter(|&d| d > 0.0) {
                                let seek_resp = overlay_ui
                                    .interact(
                                        layout.seek_hit,
                                        seek_id,
                                        egui::Sense::click_and_drag(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                seek_hot = seek_resp.hovered() || seek_resp.dragged();
                                if seek_hot {
                                    pointer_over_control = true;
                                }
                                if (seek_resp.clicked() || seek_resp.dragged())
                                    && let Some(pos) = seek_resp.interact_pointer_pos()
                                {
                                    let pct = ((pos.x - vrect.left()) / vrect.width().max(1.0))
                                        .clamp(0.0, 1.0)
                                        as f64;
                                    seek_target_secs = Some(dur * pct);
                                    consumed_click = true;
                                }
                            }

                            if hovered_video {
                                if layout.title.width() >= 24.0 {
                                    let mut title_ui = overlay_ui.new_child(
                                        egui::UiBuilder::new()
                                            .max_rect(layout.title)
                                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                    );
                                    title_ui.set_clip_rect(
                                        layout.title.intersect(overlay_ui.clip_rect()),
                                    );
                                    let title_resp = title_ui.add(
                                        egui::Label::new(
                                            RichText::new(&video.title)
                                                .size(11.5)
                                                .strong()
                                                .color(Color32::from_rgba_unmultiplied(
                                                    255, 255, 255, 238,
                                                )),
                                        )
                                        .truncate()
                                        .selectable(false),
                                    );
                                    if !video.title.trim().is_empty() {
                                        let tip = if video.channel.trim().is_empty() {
                                            video.title.clone()
                                        } else {
                                            format!("{}\n{}", video.title, video.channel)
                                        };
                                        let _ = title_resp.on_hover_text(tip);
                                    }
                                }

                                let expand_tip = if pt {
                                    "Ampliar player"
                                } else {
                                    "Expand player"
                                };
                                let expand_resp = mini_player_overlay_icon_button(
                                    &mut overlay_ui,
                                    layout.top_expand,
                                    egui::Id::new("fasttube_mini_player_expand"),
                                    true,
                                    MiniPlayerOverlayIcon::Expand,
                                )
                                .on_hover_text(expand_tip);
                                if expand_resp.hovered() || expand_resp.clicked() {
                                    pointer_over_control = true;
                                }
                                if expand_resp.clicked() {
                                    expand_clicked = true;
                                    consumed_click = true;
                                }

                                let pip_tip = if pt {
                                    "Picture-in-Picture na área de trabalho"
                                } else {
                                    "Desktop Picture-in-Picture"
                                };
                                let pip_resp = mini_player_overlay_icon_button(
                                    &mut overlay_ui,
                                    layout.top_pip,
                                    egui::Id::new("fasttube_mini_player_pip"),
                                    true,
                                    MiniPlayerOverlayIcon::DesktopPip,
                                )
                                .on_hover_text(pip_tip);
                                if pip_resp.hovered() || pip_resp.clicked() {
                                    pointer_over_control = true;
                                }
                                if pip_resp.clicked() {
                                    desktop_pip_clicked = true;
                                    consumed_click = true;
                                }

                                let close_tip = if pt {
                                    "Fechar miniplayer"
                                } else {
                                    "Close miniplayer"
                                };
                                let close_resp = mini_player_overlay_icon_button(
                                    &mut overlay_ui,
                                    layout.top_close,
                                    egui::Id::new("fasttube_mini_player_close"),
                                    true,
                                    MiniPlayerOverlayIcon::Close,
                                )
                                .on_hover_text(close_tip);
                                if close_resp.hovered() || close_resp.clicked() {
                                    pointer_over_control = true;
                                }
                                if close_resp.clicked() {
                                    close_clicked = true;
                                    consumed_click = true;
                                }

                                if !dragging_seek {
                                    let prev_tip = if can_prev_video {
                                        if pt {
                                            "Vídeo anterior"
                                        } else {
                                            "Previous video"
                                        }
                                    } else if can_prev {
                                        if pt {
                                            "Reiniciar vídeo"
                                        } else {
                                            "Restart video"
                                        }
                                    } else if pt {
                                        "Nenhum vídeo anterior"
                                    } else {
                                        "No previous video"
                                    };
                                    let prev_resp = mini_player_overlay_circle_button(
                                        &mut overlay_ui,
                                        layout.center_prev,
                                        egui::Id::new("fasttube_mini_player_prev"),
                                        can_prev,
                                        MiniPlayerOverlayIcon::Previous,
                                    )
                                    .on_hover_text(prev_tip);
                                    if prev_resp.hovered() || prev_resp.clicked() {
                                        pointer_over_control = true;
                                    }
                                    if can_prev && prev_resp.clicked() {
                                        prev_clicked = true;
                                        consumed_click = true;
                                    }

                                    let play_tip = if paused {
                                        if pt { "Reproduzir" } else { "Play" }
                                    } else if pt {
                                        "Pausar"
                                    } else {
                                        "Pause"
                                    };
                                    let play_resp = mini_player_overlay_play_button(
                                        &mut overlay_ui,
                                        layout.center_play,
                                        egui::Id::new("fasttube_mini_player_play"),
                                        paused,
                                    )
                                    .on_hover_text(play_tip);
                                    if play_resp.clicked() {
                                        toggle_pause_clicked = true;
                                        consumed_click = true;
                                    }

                                    let next_tip = if can_next {
                                        if has_queue {
                                            if pt {
                                                "Próximo da fila"
                                            } else {
                                                "Next in queue"
                                            }
                                        } else if pt {
                                            "Próximo vídeo"
                                        } else {
                                            "Next video"
                                        }
                                    } else if pt {
                                        "Nenhum próximo vídeo"
                                    } else {
                                        "No next video"
                                    };
                                    let next_resp = mini_player_overlay_circle_button(
                                        &mut overlay_ui,
                                        layout.center_next,
                                        egui::Id::new("fasttube_mini_player_next"),
                                        can_next,
                                        MiniPlayerOverlayIcon::Next,
                                    )
                                    .on_hover_text(next_tip);
                                    if next_resp.hovered() || next_resp.clicked() {
                                        pointer_over_control = true;
                                    }
                                    if can_next && next_resp.clicked() {
                                        next_clicked = true;
                                        consumed_click = true;
                                    }
                                }

                                let mute_tip = if muted || volume_percent <= 0.5 {
                                    if pt { "Ativar som (M)" } else { "Unmute (M)" }
                                } else if pt {
                                    "Silenciar (M)"
                                } else {
                                    "Mute (M)"
                                };
                                let mute_resp = mini_player_overlay_icon_button(
                                    &mut overlay_ui,
                                    layout.bottom_mute,
                                    egui::Id::new("fasttube_mini_player_mute"),
                                    true,
                                    MiniPlayerOverlayIcon::Volume {
                                        muted,
                                        volume_percent,
                                    },
                                )
                                .on_hover_text(mute_tip);
                                if mute_resp.hovered() || mute_resp.clicked() {
                                    pointer_over_control = true;
                                }
                                if mute_resp.clicked() {
                                    toggle_mute_clicked = true;
                                    consumed_click = true;
                                }

                                let (new_vol, vol_resp) = mini_player_overlay_volume_slider(
                                    &mut overlay_ui,
                                    layout.bottom_volume,
                                    volume_id,
                                    volume_percent,
                                    muted,
                                );
                                if vol_resp.hovered() || vol_resp.dragged() || vol_resp.clicked() {
                                    pointer_over_control = true;
                                }
                                if let Some(vol) = new_vol {
                                    set_volume_target = Some(vol);
                                    consumed_click = true;
                                } else if !self.autoscroll.is_active() {
                                    let vol_area = mute_resp.rect.union(vol_resp.rect);
                                    let over_volume = mute_resp.hovered()
                                        || vol_resp.hovered()
                                        || ctx
                                            .input(|i| i.pointer.hover_pos())
                                            .is_some_and(|p| vol_area.contains(p));
                                    if over_volume {
                                        let wheel_y = ctx.input(|i| i.smooth_scroll_delta.y);
                                        if wheel_y.is_finite() && wheel_y != 0.0 {
                                            ctx.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
                                        }
                                        if let Some(next_vol) = compute_wheel_volume_target(
                                            volume_percent,
                                            muted,
                                            wheel_y,
                                            5.0,
                                        ) {
                                            set_volume_target = Some(next_vol);
                                        }
                                    }
                                }

                                if layout.bottom_time.width() >= 48.0 {
                                    let time_painter = overlay_ui.painter();
                                    if is_live {
                                        let live_label = if pt { "AO VIVO" } else { "LIVE" };
                                        let text_color =
                                            Color32::from_rgba_unmultiplied(255, 255, 255, 230);
                                        let text_galley = time_painter.layout_no_wrap(
                                            live_label.to_owned(),
                                            egui::FontId::proportional(10.0),
                                            text_color,
                                        );
                                        let text_pos = egui::pos2(
                                            layout.bottom_time.right() - text_galley.size().x,
                                            layout.bottom_time.center().y
                                                - text_galley.size().y * 0.5,
                                        );
                                        let dot_pos = egui::pos2(
                                            (text_pos.x - 6.0)
                                                .max(layout.bottom_time.left() + 3.0),
                                            layout.bottom_time.center().y,
                                        );
                                        time_painter.circle_filled(
                                            dot_pos,
                                            2.5,
                                            Color32::from_rgb(255, 0, 51),
                                        );
                                        time_painter.galley(text_pos, text_galley, text_color);
                                    } else if let Some(dur) = duration.filter(|&d| d > 0.0) {
                                        let shown_pos =
                                            seek_target_secs.unwrap_or(position).clamp(0.0, dur);
                                        let label = format!(
                                            "{} / {}",
                                            overlay::format_time(shown_pos),
                                            overlay::format_time(dur)
                                        );
                                        time_painter.text(
                                            egui::pos2(
                                                layout.bottom_time.right(),
                                                layout.bottom_time.center().y,
                                            ),
                                            egui::Align2::RIGHT_CENTER,
                                            label,
                                            egui::FontId::proportional(10.5),
                                            Color32::from_rgba_unmultiplied(255, 255, 255, 210),
                                        );
                                    }
                                }
                            }

                            let bar_h = if seek_hot {
                                4.5
                            } else if hovered_video {
                                3.0
                            } else {
                                2.5
                            };
                            let bar_rect = egui::Rect::from_min_max(
                                egui::pos2(vrect.left(), vrect.bottom() - bar_h),
                                vrect.max,
                            );
                            let track_alpha = if hovered_video { 75 } else { 52 };
                            overlay_ui.painter().rect_filled(
                                bar_rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(255, 255, 255, track_alpha),
                            );
                            let shown_pos = seek_target_secs.unwrap_or(position);
                            let frac = if is_live {
                                1.0
                            } else if let Some(dur) = duration.filter(|&d| d > 0.0) {
                                (shown_pos / dur).clamp(0.0, 1.0) as f32
                            } else {
                                0.0
                            };
                            if frac > 0.0 {
                                let fill_rect = egui::Rect::from_min_max(
                                    bar_rect.min,
                                    egui::pos2(
                                        bar_rect.left() + bar_rect.width() * frac,
                                        bar_rect.bottom(),
                                    ),
                                );
                                overlay_ui.painter().rect_filled(
                                    fill_rect,
                                    0.0,
                                    Color32::from_rgb(255, 0, 51),
                                );
                                if seek_hot {
                                    let knob_x = fill_rect.right().clamp(
                                        bar_rect.left() + 4.5,
                                        (bar_rect.right() - 4.5).max(bar_rect.left() + 4.5),
                                    );
                                    overlay_ui.painter().circle_filled(
                                        egui::pos2(knob_x, bar_rect.center().y),
                                        4.5,
                                        Color32::from_rgb(255, 0, 51),
                                    );
                                }
                            }

                            if surface_clicked && !consumed_click && !pointer_over_control {
                                toggle_pause_clicked = true;
                            }
                        }

                        if !can_overlay_hover {
                            let inner_w = (card_w - 12.0).max(200.0);
                            egui::Frame::new()
                                .inner_margin(egui::Margin::symmetric(6, 4))
                                .show(ui, |ui| {
                                    ui.set_width(inner_w);
                                    ui.set_max_width(inner_w);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(inner_w, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.x = 3.0;
                                            if mini_player_fallback_icon_button(
                                                ui,
                                                can_prev,
                                                MiniPlayerOverlayIcon::Previous,
                                                &palette,
                                            )
                                            .on_hover_text(if can_prev_video {
                                                if pt {
                                                    "Vídeo anterior"
                                                } else {
                                                    "Previous video"
                                                }
                                            } else if can_prev {
                                                if pt {
                                                    "Reiniciar vídeo"
                                                } else {
                                                    "Restart video"
                                                }
                                            } else if pt {
                                                "Nenhum vídeo anterior"
                                            } else {
                                                "No previous video"
                                            })
                                            .clicked()
                                            {
                                                prev_clicked = true;
                                            }
                                            if mini_player_fallback_icon_button(
                                                ui,
                                                true,
                                                MiniPlayerOverlayIcon::PlayPause { paused },
                                                &palette,
                                            )
                                            .on_hover_text(if paused {
                                                if pt { "Reproduzir" } else { "Play" }
                                            } else if pt {
                                                "Pausar"
                                            } else {
                                                "Pause"
                                            })
                                            .clicked()
                                            {
                                                toggle_pause_clicked = true;
                                            }
                                            if mini_player_fallback_icon_button(
                                                ui,
                                                can_next,
                                                MiniPlayerOverlayIcon::Next,
                                                &palette,
                                            )
                                            .on_hover_text(if can_next {
                                                if pt {
                                                    "Próximo vídeo"
                                                } else {
                                                    "Next video"
                                                }
                                            } else if pt {
                                                "Nenhum próximo vídeo"
                                            } else {
                                                "No next video"
                                            })
                                            .clicked()
                                            {
                                                next_clicked = true;
                                            }
                                            let mute_resp = mini_player_fallback_icon_button(
                                                ui,
                                                true,
                                                MiniPlayerOverlayIcon::Volume {
                                                    muted,
                                                    volume_percent,
                                                },
                                                &palette,
                                            )
                                            .on_hover_text(
                                                if muted || volume_percent <= 0.5 {
                                                    if pt {
                                                        "Ativar som (M)"
                                                    } else {
                                                        "Unmute (M)"
                                                    }
                                                } else if pt {
                                                    "Silenciar (M)"
                                                } else {
                                                    "Mute (M)"
                                                },
                                            );
                                            if mute_resp.clicked() {
                                                toggle_mute_clicked = true;
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.spacing_mut().item_spacing.x = 3.0;
                                                    if mini_player_fallback_icon_button(
                                                        ui,
                                                        true,
                                                        MiniPlayerOverlayIcon::Close,
                                                        &palette,
                                                    )
                                                    .on_hover_text(if pt {
                                                        "Fechar miniplayer"
                                                    } else {
                                                        "Close miniplayer"
                                                    })
                                                    .clicked()
                                                    {
                                                        close_clicked = true;
                                                    }
                                                    if mini_player_fallback_icon_button(
                                                        ui,
                                                        true,
                                                        MiniPlayerOverlayIcon::DesktopPip,
                                                        &palette,
                                                    )
                                                    .on_hover_text(if pt {
                                                        "Picture-in-Picture na área de trabalho"
                                                    } else {
                                                        "Desktop Picture-in-Picture"
                                                    })
                                                    .clicked()
                                                    {
                                                        desktop_pip_clicked = true;
                                                    }
                                                    if mini_player_fallback_icon_button(
                                                        ui,
                                                        true,
                                                        MiniPlayerOverlayIcon::Expand,
                                                        &palette,
                                                    )
                                                    .on_hover_text(if pt {
                                                        "Ampliar player"
                                                    } else {
                                                        "Expand player"
                                                    })
                                                    .clicked()
                                                    {
                                                        expand_clicked = true;
                                                    }
                                                    let vol_w =
                                                        ui.available_width().clamp(32.0, 64.0);
                                                    let (vol_rect, _) = ui.allocate_exact_size(
                                                        egui::vec2(vol_w, 22.0),
                                                        egui::Sense::hover(),
                                                    );
                                                    let (new_vol, vol_resp) =
                                                        mini_player_overlay_volume_slider_with_colors(
                                                            ui,
                                                            vol_rect,
                                                            egui::Id::new(
                                                                "fasttube_mini_player_fallback_volume",
                                                            ),
                                                            volume_percent,
                                                            muted,
                                                            palette
                                                                .neutral_status_fg
                                                                .gamma_multiply(0.35),
                                                            palette.header_button_fg,
                                                            palette.header_button_fg,
                                                        );
                                                    if let Some(vol) = new_vol {
                                                        set_volume_target = Some(vol);
                                                    } else if !self.autoscroll.is_active()
                                                        && (mute_resp.hovered()
                                                            || vol_resp.hovered())
                                                    {
                                                        let wheel_y = ctx
                                                            .input(|i| i.smooth_scroll_delta.y);
                                                        if wheel_y.is_finite() && wheel_y != 0.0 {
                                                            ctx.input_mut(|i| {
                                                                i.smooth_scroll_delta.y = 0.0;
                                                            });
                                                        }
                                                        if let Some(next_vol) =
                                                            compute_wheel_volume_target(
                                                                volume_percent,
                                                                muted,
                                                                wheel_y,
                                                                5.0,
                                                            )
                                                        {
                                                            set_volume_target = Some(next_vol);
                                                        }
                                                    }
                                                },
                                            );
                                        },
                                    );
                                });
                        }
                    });
            });

        if let Some(target) = seek_target_secs {
            self.seek_to_seconds(target);
        }
        if let Some(vol) = set_volume_target {
            self.set_in_app_mini_player_volume(vol);
        }
        if toggle_mute_clicked {
            self.toggle_in_app_mini_player_mute();
        }
        if toggle_pause_clicked {
            self.toggle_in_app_mini_player_playback();
        }
        if prev_clicked {
            self.trigger_in_app_mini_player_previous(is_live, position);
        } else if next_clicked {
            self.trigger_in_app_mini_player_next();
        }
        if close_clicked {
            self.close_video(ctx);
        } else if desktop_pip_clicked {
            self.toggle_mini_player(ctx);
        } else if expand_clicked {
            self.exit_in_app_mini_player(ctx);
        }
    }
}

pub(crate) const IN_APP_MINI_PLAYER_ANCHOR_OFFSET: f32 = 14.0;
const MINI_PLAYER_FALLBACK_BAR_HEIGHT: f32 = 30.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct MiniPlayerOverlayLayout {
    pub(crate) top_scrim: egui::Rect,
    pub(crate) bottom_scrim: egui::Rect,
    pub(crate) title: egui::Rect,
    pub(crate) top_expand: egui::Rect,
    pub(crate) top_pip: egui::Rect,
    pub(crate) top_close: egui::Rect,
    pub(crate) center_prev: egui::Rect,
    pub(crate) center_play: egui::Rect,
    pub(crate) center_next: egui::Rect,
    pub(crate) bottom_mute: egui::Rect,
    pub(crate) bottom_volume: egui::Rect,
    pub(crate) bottom_time: egui::Rect,
    pub(crate) seek_hit: egui::Rect,
}

impl MiniPlayerOverlayLayout {
    pub(crate) fn compute(vrect: egui::Rect) -> Self {
        let top_scrim_h = 40.0_f32.min(vrect.height() * 0.34);
        let bottom_scrim_h = 46.0_f32.min(vrect.height() * 0.38);
        let top_scrim = egui::Rect::from_min_max(
            vrect.min,
            egui::pos2(
                vrect.right(),
                (vrect.top() + top_scrim_h).min(vrect.bottom()),
            ),
        );
        let bottom_scrim = egui::Rect::from_min_max(
            egui::pos2(
                vrect.left(),
                (vrect.bottom() - bottom_scrim_h).max(vrect.top()),
            ),
            vrect.max,
        );

        let top_y = vrect.top() + 5.0;
        let btn = 22.0_f32;
        let gap = 2.0_f32;
        let right_pad = 6.0_f32;

        let top_close = egui::Rect::from_min_size(
            egui::pos2(vrect.right() - right_pad - btn, top_y),
            egui::vec2(btn, btn),
        );
        let top_pip = egui::Rect::from_min_size(
            egui::pos2(top_close.left() - gap - btn, top_y),
            egui::vec2(btn, btn),
        );
        let top_expand = egui::Rect::from_min_size(
            egui::pos2(top_pip.left() - gap - btn, top_y),
            egui::vec2(btn, btn),
        );
        let title_left = vrect.left() + 8.0;
        let title_right = (top_expand.left() - 6.0).max(title_left);
        let title = egui::Rect::from_min_max(
            egui::pos2(title_left, top_y),
            egui::pos2(title_right, top_y + btn),
        );

        let center = vrect.center();
        let center_play = egui::Rect::from_center_size(center, egui::vec2(36.0, 36.0));
        let side_btn = 26.0_f32;
        let side_gap = 12.0_f32;
        let center_prev = egui::Rect::from_min_size(
            egui::pos2(
                center_play.left() - side_gap - side_btn,
                center.y - side_btn * 0.5,
            ),
            egui::vec2(side_btn, side_btn),
        );
        let center_next = egui::Rect::from_min_size(
            egui::pos2(center_play.right() + side_gap, center.y - side_btn * 0.5),
            egui::vec2(side_btn, side_btn),
        );

        let seek_hit = egui::Rect::from_min_max(
            egui::pos2(vrect.left(), (vrect.bottom() - 10.0).max(vrect.top())),
            vrect.max,
        );

        let bottom_y = (seek_hit.top() - btn - 2.0).max(vrect.top());
        let bottom_mute = egui::Rect::from_min_size(
            egui::pos2(vrect.left() + 6.0, bottom_y),
            egui::vec2(btn, btn),
        );
        let vol_w = (vrect.width() * 0.24).clamp(48.0, 68.0);
        let bottom_volume = egui::Rect::from_min_size(
            egui::pos2(bottom_mute.right() + 2.0, bottom_y),
            egui::vec2(vol_w, btn),
        );
        let time_left = bottom_volume.right() + 8.0;
        let time_right = (vrect.right() - 8.0).max(time_left);
        let bottom_time = egui::Rect::from_min_max(
            egui::pos2(time_left, bottom_y),
            egui::pos2(time_right, bottom_y + btn),
        );

        Self {
            top_scrim,
            bottom_scrim,
            title,
            top_expand,
            top_pip,
            top_close,
            center_prev,
            center_play,
            center_next,
            bottom_mute,
            bottom_volume,
            bottom_time,
            seek_hit,
        }
    }
}

#[derive(Clone, Copy)]
enum MiniPlayerOverlayIcon {
    Previous,
    Next,
    PlayPause { paused: bool },
    Volume { muted: bool, volume_percent: f32 },
    Expand,
    DesktopPip,
    Close,
}

fn mini_player_overlay_icon_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    enabled: bool,
    icon: MiniPlayerOverlayIcon,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
            painter.rect_filled(
                rect,
                6.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, 34),
            );
        }
        let fg = if enabled {
            if response.hovered() {
                Color32::WHITE
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 224)
            }
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 84)
        };
        paint_mini_player_icon(painter, rect.center(), icon, fg);
    }
    response
}

fn mini_player_overlay_circle_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    enabled: bool,
    icon: MiniPlayerOverlayIcon,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = rect.width().min(rect.height()) * 0.5;
        let bg_alpha = if !enabled {
            68
        } else if response.hovered() || response.is_pointer_button_down_on() {
            175
        } else {
            115
        };
        painter.circle_filled(rect.center(), radius, Color32::from_black_alpha(bg_alpha));
        if enabled && response.hovered() {
            painter.circle_stroke(
                rect.center(),
                radius,
                egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 42)),
            );
        }
        let fg = if enabled {
            Color32::WHITE
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 92)
        };
        paint_mini_player_icon(painter, rect.center(), icon, fg);
    }
    response
}

fn mini_player_overlay_play_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    paused: bool,
) -> egui::Response {
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = rect.width().min(rect.height()) * 0.5;
        let bg_alpha = if response.hovered() || response.is_pointer_button_down_on() {
            195
        } else {
            150
        };
        painter.circle_filled(rect.center(), radius, Color32::from_black_alpha(bg_alpha));
        painter.circle_stroke(
            rect.center(),
            radius,
            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 48)),
        );
        paint_mini_player_icon(
            painter,
            rect.center(),
            MiniPlayerOverlayIcon::PlayPause { paused },
            Color32::WHITE,
        );
    }
    response
}

fn mini_player_fallback_icon_button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: MiniPlayerOverlayIcon,
    palette: &ThemePalette,
) -> egui::Response {
    let size = egui::vec2(22.0, 22.0);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
            painter.rect_filled(rect, 6.0, palette.card_alt_bg);
        }
        let fg = if enabled {
            palette.header_button_fg
        } else {
            palette.neutral_status_fg.gamma_multiply(0.5)
        };
        paint_mini_player_icon(painter, rect.center(), icon, fg);
    }
    response
}

fn paint_mini_player_icon(
    painter: &egui::Painter,
    c: egui::Pos2,
    icon: MiniPlayerOverlayIcon,
    fg: Color32,
) {
    match icon {
        MiniPlayerOverlayIcon::Previous => {
            let bar = egui::Rect::from_min_max(
                egui::pos2(c.x - 4.8, c.y - 4.2),
                egui::pos2(c.x - 3.0, c.y + 4.2),
            );
            painter.rect_filled(bar, 0.5, fg);
            let tri = vec![
                egui::pos2(c.x + 4.6, c.y - 4.2),
                egui::pos2(c.x + 4.6, c.y + 4.2),
                egui::pos2(c.x - 1.6, c.y),
            ];
            painter.add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
        }
        MiniPlayerOverlayIcon::Next => {
            let tri = vec![
                egui::pos2(c.x - 4.6, c.y - 4.2),
                egui::pos2(c.x - 4.6, c.y + 4.2),
                egui::pos2(c.x + 1.6, c.y),
            ];
            painter.add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
            let bar = egui::Rect::from_min_max(
                egui::pos2(c.x + 3.0, c.y - 4.2),
                egui::pos2(c.x + 4.8, c.y + 4.2),
            );
            painter.rect_filled(bar, 0.5, fg);
        }
        MiniPlayerOverlayIcon::PlayPause { paused } => {
            if paused {
                let tri = vec![
                    egui::pos2(c.x - 4.0, c.y - 6.2),
                    egui::pos2(c.x - 4.0, c.y + 6.2),
                    egui::pos2(c.x + 6.5, c.y),
                ];
                painter.add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
            } else {
                let left_bar = egui::Rect::from_min_max(
                    egui::pos2(c.x - 4.8, c.y - 5.8),
                    egui::pos2(c.x - 1.4, c.y + 5.8),
                );
                let right_bar = egui::Rect::from_min_max(
                    egui::pos2(c.x + 1.4, c.y - 5.8),
                    egui::pos2(c.x + 4.8, c.y + 5.8),
                );
                painter.rect_filled(left_bar, 1.0, fg);
                painter.rect_filled(right_bar, 1.0, fg);
            }
        }
        MiniPlayerOverlayIcon::Volume {
            muted,
            volume_percent,
        } => {
            let speaker = vec![
                egui::pos2(c.x - 5.2, c.y - 1.8),
                egui::pos2(c.x - 2.6, c.y - 1.8),
                egui::pos2(c.x + 0.6, c.y - 4.4),
                egui::pos2(c.x + 0.6, c.y + 4.4),
                egui::pos2(c.x - 2.6, c.y + 1.8),
                egui::pos2(c.x - 5.2, c.y + 1.8),
            ];
            painter.add(egui::Shape::convex_polygon(speaker, fg, egui::Stroke::NONE));
            if muted || volume_percent <= 0.5 {
                painter.line_segment(
                    [
                        egui::pos2(c.x + 2.2, c.y - 2.4),
                        egui::pos2(c.x + 5.8, c.y + 2.4),
                    ],
                    egui::Stroke::new(1.4, fg),
                );
                painter.line_segment(
                    [
                        egui::pos2(c.x + 5.8, c.y - 2.4),
                        egui::pos2(c.x + 2.2, c.y + 2.4),
                    ],
                    egui::Stroke::new(1.4, fg),
                );
            } else {
                let arc1 = vec![
                    egui::pos2(c.x + 2.1, c.y - 2.1),
                    egui::pos2(c.x + 3.2, c.y),
                    egui::pos2(c.x + 2.1, c.y + 2.1),
                ];
                painter.add(egui::Shape::line(arc1, egui::Stroke::new(1.3, fg)));
                if volume_percent > 45.0 {
                    let arc2 = vec![
                        egui::pos2(c.x + 4.0, c.y - 3.7),
                        egui::pos2(c.x + 5.7, c.y),
                        egui::pos2(c.x + 4.0, c.y + 3.7),
                    ];
                    painter.add(egui::Shape::line(arc2, egui::Stroke::new(1.3, fg)));
                }
            }
        }
        MiniPlayerOverlayIcon::Expand => {
            let stroke = egui::Stroke::new(1.5, fg);
            let tr = egui::pos2(c.x + 4.5, c.y - 4.5);
            let bl = egui::pos2(c.x - 4.5, c.y + 4.5);
            painter.line_segment([bl, tr], stroke);
            painter.line_segment([tr, egui::pos2(tr.x - 3.6, tr.y)], stroke);
            painter.line_segment([tr, egui::pos2(tr.x, tr.y + 3.6)], stroke);
            painter.line_segment([bl, egui::pos2(bl.x + 3.6, bl.y)], stroke);
            painter.line_segment([bl, egui::pos2(bl.x, bl.y - 3.6)], stroke);
        }
        MiniPlayerOverlayIcon::DesktopPip => {
            let outer = egui::Rect::from_center_size(c, egui::vec2(12.5, 9.5));
            painter.rect_stroke(
                outer,
                1.5,
                egui::Stroke::new(1.3, fg),
                egui::StrokeKind::Inside,
            );
            let inner = egui::Rect::from_min_max(
                egui::pos2(outer.right() - 6.0, outer.bottom() - 4.5),
                egui::pos2(outer.right() - 1.8, outer.bottom() - 1.8),
            );
            painter.rect_filled(inner, 0.8, fg);
        }
        MiniPlayerOverlayIcon::Close => {
            let stroke = egui::Stroke::new(1.6, fg);
            let d = 4.0_f32;
            painter.line_segment(
                [egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(c.x + d, c.y - d), egui::pos2(c.x - d, c.y + d)],
                stroke,
            );
        }
    }
}

fn mini_player_overlay_volume_slider(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    volume_percent: f32,
    muted: bool,
) -> (Option<f32>, egui::Response) {
    mini_player_overlay_volume_slider_with_colors(
        ui,
        rect,
        id,
        volume_percent,
        muted,
        Color32::from_rgba_unmultiplied(255, 255, 255, 84),
        Color32::WHITE,
        Color32::WHITE,
    )
}

#[allow(clippy::too_many_arguments)]
fn mini_player_overlay_volume_slider_with_colors(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    volume_percent: f32,
    muted: bool,
    track_color: Color32,
    fill_color: Color32,
    thumb_color: Color32,
) -> (Option<f32>, egui::Response) {
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let effective = if muted {
        0.0
    } else {
        volume_percent.clamp(0.0, 100.0)
    };
    let ratio = (effective / 100.0).clamp(0.0, 1.0);
    let mut new_volume = None;
    let track_left = rect.left() + 5.0;
    let track_right = (rect.right() - 5.0).max(track_left + 1.0);
    if (response.dragged() || response.clicked())
        && rect.width() > 10.0
        && let Some(pos) = response.interact_pointer_pos()
    {
        let frac = ((pos.x - track_left) / (track_right - track_left)).clamp(0.0, 1.0);
        new_volume = Some((frac * 100.0).round());
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let cy = rect.center().y;
        let track_rect = egui::Rect::from_min_max(
            egui::pos2(track_left, cy - 1.25),
            egui::pos2(track_right, cy + 1.25),
        );
        painter.rect_filled(track_rect, 1.25, track_color);
        let shown_ratio = new_volume.map(|v| v / 100.0).unwrap_or(ratio);
        let fill_x = track_left + (track_right - track_left) * shown_ratio;
        if fill_x > track_left {
            let fill_rect = egui::Rect::from_min_max(
                egui::pos2(track_left, cy - 1.25),
                egui::pos2(fill_x, cy + 1.25),
            );
            painter.rect_filled(fill_rect, 1.25, fill_color);
        }
        let thumb_radius = if response.hovered() || response.dragged() {
            5.0
        } else {
            4.0
        };
        painter.circle_filled(egui::pos2(fill_x, cy), thumb_radius, thumb_color);
    }
    let resp = response.on_hover_text(format!("Volume: {:.0}%", effective));
    (new_volume, resp)
}
