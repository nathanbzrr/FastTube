use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};

impl FastTube {
    pub(crate) fn paint_audio_only_placeholder(
        painter: &egui::Painter,
        rect: egui::Rect,
        selected: Option<&Video>,
    ) {
        if rect.width() <= 20.0 || rect.height() <= 20.0 {
            return;
        }
        let clipped = painter.with_clip_rect(rect);
        clipped.rect_filled(rect, 0.0, Color32::from_rgb(16, 18, 24));
        let glow_r = (rect.width().min(rect.height()) * 0.38).clamp(40.0, 170.0);
        clipped.circle_filled(
            rect.center(),
            glow_r,
            Color32::from_rgba_unmultiplied(62, 166, 255, 22),
        );
        clipped.circle_stroke(
            rect.center(),
            glow_r * 0.72,
            egui::Stroke::new(1.2, Color32::from_rgba_unmultiplied(62, 166, 255, 65)),
        );

        let badge_r = (rect.height() * 0.13).clamp(18.0, 34.0);
        let icon_center = if rect.height() >= 190.0 {
            egui::pos2(rect.center().x, rect.center().y - 34.0)
        } else {
            egui::pos2(rect.center().x, rect.center().y - 12.0)
        };
        clipped.circle_filled(
            icon_center,
            badge_r,
            Color32::from_rgba_unmultiplied(30, 44, 66, 235),
        );
        clipped.circle_stroke(
            icon_center,
            badge_r,
            egui::Stroke::new(1.5, Color32::from_rgb(90, 190, 255)),
        );
        let s = badge_r * 0.45;
        let note_color = Color32::from_rgb(235, 246, 255);
        clipped.circle_filled(
            egui::pos2(icon_center.x - s * 0.45, icon_center.y + s * 0.48),
            s * 0.34,
            note_color,
        );
        clipped.circle_filled(
            egui::pos2(icon_center.x + s * 0.55, icon_center.y + s * 0.32),
            s * 0.34,
            note_color,
        );
        clipped.line_segment(
            [
                egui::pos2(icon_center.x - s * 0.20, icon_center.y + s * 0.42),
                egui::pos2(icon_center.x - s * 0.20, icon_center.y - s * 0.58),
            ],
            egui::Stroke::new(2.0, note_color),
        );
        clipped.line_segment(
            [
                egui::pos2(icon_center.x + s * 0.80, icon_center.y + s * 0.26),
                egui::pos2(icon_center.x + s * 0.80, icon_center.y - s * 0.72),
            ],
            egui::Stroke::new(2.0, note_color),
        );
        clipped.line_segment(
            [
                egui::pos2(icon_center.x - s * 0.24, icon_center.y - s * 0.58),
                egui::pos2(icon_center.x + s * 0.84, icon_center.y - s * 0.72),
            ],
            egui::Stroke::new(3.0, note_color),
        );

        let badge_y = icon_center.y + badge_r + 16.0;
        clipped.text(
            egui::pos2(rect.center().x, badge_y),
            egui::Align2::CENTER_CENTER,
            "Modo somente \u{00e1}udio",
            egui::FontId::proportional(13.5),
            Color32::from_rgb(130, 210, 255),
        );

        if let Some(video) = selected {
            let max_text_w = (rect.width() - 48.0).max(80.0);
            if rect.height() >= 170.0 && !video.title.trim().is_empty() {
                let title_galley = clipped.layout(
                    video.title.clone(),
                    egui::FontId::proportional(15.0),
                    Color32::from_rgb(242, 244, 248),
                    max_text_w,
                );
                let title_pos = egui::pos2(
                    rect.center().x - title_galley.size().x * 0.5,
                    badge_y + 14.0,
                );
                let title_h = title_galley.size().y;
                clipped.galley(title_pos, title_galley, Color32::from_rgb(242, 244, 248));
                if !video.channel.trim().is_empty() && rect.height() >= 210.0 {
                    clipped.text(
                        egui::pos2(rect.center().x, title_pos.y + title_h + 12.0),
                        egui::Align2::CENTER_CENTER,
                        video.channel.trim(),
                        egui::FontId::proportional(12.5),
                        Color32::from_rgb(168, 174, 186),
                    );
                }
            }
        }
    }

    pub(crate) fn paint_hidden_video_notice(&self, ui: &egui::Ui, rect: egui::Rect) {
        if self.ui.settings_open {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "V\u{00ed}deo oculto temporariamente enquanto o menu de configura\u{00e7}\u{00f5}es est\u{00e1} aberto",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(165, 165, 175),
            );
        } else if self.playback.audio_only {
            Self::paint_audio_only_placeholder(ui.painter(), rect, self.selected.as_ref());
        }
    }

    fn player_osd_id() -> egui::Id {
        egui::Id::new("fasttube_player_osd_feedback")
    }

    #[cfg(windows)]
    fn player_osd_snapshot_id() -> egui::Id {
        egui::Id::new("fasttube_player_osd_snapshot")
    }

    pub(crate) fn record_player_osd(ctx: &egui::Context, label: impl Into<String>) {
        let feedback = PlayerOsdFeedback {
            label: label.into(),
            shown_at: Instant::now(),
        };
        ctx.data_mut(|d| d.insert_temp(Self::player_osd_id(), feedback));
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    #[cfg(windows)]
    fn update_player_osd_snapshot_state(
        ctx: &egui::Context,
        video_url: &str,
        mutate: impl FnOnce(&mut PlayerOsdSnapshot),
    ) {
        ctx.data_mut(|d| {
            let id = Self::player_osd_snapshot_id();
            if let Some(mut snap) = d.get_temp::<PlayerOsdSnapshot>(id)
                && snap.video_url == video_url
            {
                mutate(&mut snap);
                d.insert_temp(id, snap);
            }
        });
    }

    #[cfg(windows)]
    #[allow(clippy::too_many_arguments)]
    fn sync_player_osd_from_telemetry(
        &self,
        ctx: &egui::Context,
        video: &Video,
        telemetry: &PlayerTelemetry,
        position: f64,
        duration: Option<f64>,
        is_live: bool,
        is_live_dvr: bool,
    ) {
        if self.ui.settings_open {
            return;
        }
        let mut next = PlayerOsdSnapshot {
            video_url: video.url.clone(),
            paused: telemetry.paused,
            muted: telemetry.muted,
            volume_rounded: telemetry.raw_volume.round() as i32,
            speed_hundredths: (telemetry.speed * 100.0).round() as i32,
        };
        let prev = ctx.data(|d| d.get_temp::<PlayerOsdSnapshot>(Self::player_osd_snapshot_id()));
        if let Some(prev) = prev.filter(|p| p.video_url == video.url) {
            if prev.paused != next.paused {
                Self::record_player_osd(
                    ctx,
                    if next.paused {
                        "\u{23f8} Pausado"
                    } else {
                        "\u{25b6} Reproduzindo"
                    },
                );
            } else if prev.muted != next.muted || prev.volume_rounded != next.volume_rounded {
                if next.muted || next.volume_rounded == 0 {
                    Self::record_player_osd(ctx, "\u{1f507} Mudo");
                } else {
                    Self::record_player_osd(ctx, format!("\u{1f50a} {}%", next.volume_rounded));
                }
            } else if prev.speed_hundredths != next.speed_hundredths {
                Self::record_player_osd(
                    ctx,
                    format!("\u{26a1} {:.2}x", f64::from(next.speed_hundredths) / 100.0),
                );
            }
        }
        if (!is_live || is_live_dvr) && !ctx.egui_wants_keyboard_input() {
            let kb = &self.settings.prefs.value.keybindings;
            let short_mode = self.is_watch_short_mode();
            let default_step = self.seek_step_secs();
            let (back_secs, forward_secs, pct_seek) = ctx.input(|input| {
                let no_alt_ctrl = !input.modifiers.alt && !input.modifiers.ctrl;
                let back = if no_alt_ctrl && !short_mode && input.key_pressed(egui::Key::J) {
                    Some(10_u32)
                } else if settings::Keybindings::matches_input(input, &kb.seek_back) {
                    Some(default_step)
                } else {
                    None
                };
                let forward = if no_alt_ctrl && !short_mode && input.key_pressed(egui::Key::L) {
                    Some(10_u32)
                } else if settings::Keybindings::matches_input(input, &kb.seek_forward) {
                    Some(default_step)
                } else {
                    None
                };
                let pct = if no_alt_ctrl {
                    [
                        (egui::Key::Num0, 0.0),
                        (egui::Key::Num1, 0.1),
                        (egui::Key::Num2, 0.2),
                        (egui::Key::Num3, 0.3),
                        (egui::Key::Num4, 0.4),
                        (egui::Key::Num5, 0.5),
                        (egui::Key::Num6, 0.6),
                        (egui::Key::Num7, 0.7),
                        (egui::Key::Num8, 0.8),
                        (egui::Key::Num9, 0.9),
                    ]
                    .into_iter()
                    .find_map(|(k, p)| input.key_pressed(k).then_some(p))
                } else {
                    None
                };
                (back, forward, pct)
            });
            if let Some(step) = back_secs {
                Self::record_player_osd(ctx, format!("\u{23ea} -{step}s"));
            } else if let Some(step) = forward_secs {
                Self::record_player_osd(ctx, format!("\u{23e9} +{step}s"));
            } else if let Some(pct) = pct_seek
                && let Some(dur) = duration.filter(|d| d.is_finite() && *d > 0.0)
            {
                let target = dur * pct;
                let icon = if target >= position {
                    "\u{23e9}"
                } else {
                    "\u{23ea}"
                };
                Self::record_player_osd(ctx, format!("{icon} {}", format_time(target)));
            }
        }
        next.paused = telemetry.paused;
        ctx.data_mut(|d| d.insert_temp(Self::player_osd_snapshot_id(), next));
    }

    pub(crate) fn paint_player_osd_pill(ctx: &egui::Context, video_rect: egui::Rect) {
        if video_rect.width() < 120.0 || video_rect.height() < 80.0 {
            return;
        }
        let id = Self::player_osd_id();
        let Some(feedback) = ctx.data(|d| d.get_temp::<PlayerOsdFeedback>(id)) else {
            return;
        };
        const OSD_DURATION_MS: f32 = 550.0;
        let elapsed_ms = feedback.shown_at.elapsed().as_secs_f32() * 1000.0;
        if elapsed_ms >= OSD_DURATION_MS {
            ctx.data_mut(|d| d.remove::<PlayerOsdFeedback>(id));
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        let progress = (elapsed_ms / OSD_DURATION_MS).clamp(0.0, 1.0);
        let fade_t = ((progress - 0.22) / 0.78).clamp(0.0, 1.0);
        let alpha = 1.0 - fade_t * fade_t * (3.0 - 2.0 * fade_t);
        let bg_alpha = (214.0 * alpha).round() as u8;
        let border_alpha = (46.0 * alpha).round() as u8;
        let text_alpha = (255.0 * alpha).round() as u8;

        let layer_id = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("fasttube_player_osd_layer"),
        );
        let painter = ctx.layer_painter(layer_id).with_clip_rect(video_rect);
        let font_size = if video_rect.width() < 360.0 {
            13.0
        } else {
            15.0
        };
        let galley = painter.layout_no_wrap(
            feedback.label,
            egui::FontId::proportional(font_size),
            Color32::from_rgba_unmultiplied(248, 250, 255, text_alpha),
        );
        let pad = egui::vec2(16.0, 9.0);
        let pill_w = (galley.size().x + pad.x * 2.0)
            .max(54.0)
            .min((video_rect.width() - 24.0).max(40.0));
        let pill_h = (galley.size().y + pad.y * 2.0).max(34.0);
        let center_y = (video_rect.min.y + (video_rect.height() * 0.22).clamp(28.0, 108.0))
            .min(video_rect.max.y - pill_h * 0.5 - 8.0);
        let pill_rect = egui::Rect::from_center_size(
            egui::pos2(video_rect.center().x, center_y),
            egui::vec2(pill_w, pill_h),
        );
        let corner_radius = (pill_rect.height() * 0.5).round().clamp(14.0, 28.0);
        painter.rect_filled(
            pill_rect,
            corner_radius,
            Color32::from_rgba_unmultiplied(15, 17, 23, bg_alpha),
        );
        painter.rect_stroke(
            pill_rect,
            corner_radius,
            egui::Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, border_alpha),
            ),
            egui::StrokeKind::Inside,
        );
        let text_pos = egui::pos2(
            pill_rect.center().x - galley.size().x * 0.5,
            pill_rect.center().y - galley.size().y * 0.5,
        );
        painter.galley(
            text_pos,
            galley,
            Color32::from_rgba_unmultiplied(248, 250, 255, text_alpha),
        );
    }

    pub(crate) fn paint_hairline_divider(ui: &mut egui::Ui, palette: &ThemePalette) {
        let width = ui.available_width().max(1.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
        ui.painter().line_segment(
            [
                egui::pos2(rect.left(), rect.center().y),
                egui::pos2(rect.right(), rect.center().y),
            ],
            egui::Stroke::new(1.0, palette.card_alt_stroke),
        );
    }

    pub(crate) fn paint_comment_avatar_ring(ui: &egui::Ui, rect: egui::Rect) {
        ui.painter().circle_stroke(
            rect.center(),
            (rect.width().min(rect.height()) * 0.5 - 0.5).max(1.0),
            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(128, 128, 128, 45)),
        );
    }
}

#[derive(Clone)]
struct PlayerOsdFeedback {
    label: String,
    shown_at: Instant,
}

#[cfg(windows)]
#[derive(Clone)]
struct PlayerOsdSnapshot {
    video_url: String,
    paused: bool,
    muted: bool,
    volume_rounded: i32,
    speed_hundredths: i32,
}

impl FastTube {
    #[cfg(windows)]
    pub(crate) fn player_surface_and_telemetry(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        size: egui::Vec2,
        settings_open: bool,
    ) -> Option<(Option<(egui::Rect, egui::Response)>, PlayerTelemetry)> {
        let mut video_area: Option<(egui::Rect, egui::Response)> = None;
        let audio_only = self.playback.audio_only;
        let selected = self.selected.as_ref();
        let selected_url = selected.map(|v| v.url.as_str());
        let fullscreen = self.playback.fullscreen;
        let theater = self.playback.theater;
        let mini_player = self.playback.mini_player;
        let cached_raw_path = self.playback.cached_raw_path.clone();
        let cached_raw_path_poll = self.playback.cached_raw_path_poll;
        let player = self.player.as_mut()?;
        let (events, poll_error) = player.poll_events();

        let sense = if mini_player {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::click()
        };
        let (rect, response) = ui.allocate_exact_size(size, sense);
        response.surrender_focus();
        let corner_radius =
            if fullscreen || theater || mini_player || self.playback.in_app_mini_player {
                0.0
            } else {
                12.0
            };
        ui.painter()
            .rect_filled(rect, corner_radius, Color32::BLACK);
        if settings_open {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "V\u{00ed}deo oculto temporariamente enquanto o menu de configura\u{00e7}\u{00f5}es est\u{00e1} aberto",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(165, 165, 175),
            );
        } else if audio_only {
            Self::paint_audio_only_placeholder(ui.painter(), rect, selected);
            if player.is_render_api() {
                video_area = Some((rect, response));
            }
        } else if player.paint_into(ui.painter(), rect) {
            // Render API paints directly into egui, so the overlay stays above video without a native HWND.
            video_area = Some((rect, response));
        }
        player.resize(rect, ctx.pixels_per_point());

        let refresh_raw_path = !events.is_empty()
            || cached_raw_path.as_deref() != selected_url
            || cached_raw_path_poll.is_none_or(|t| t.elapsed() >= Duration::from_millis(500));
        let raw_path = if refresh_raw_path {
            player.property_cstr(c"path")
        } else {
            cached_raw_path
        };
        let width_num = player.double_cstr(c"width");
        let height_num = player.double_cstr(c"height");
        let video_size = VideoSize::from_numbers(width_num, height_num);
        let telemetry = PlayerTelemetry {
            events,
            poll_error,
            paused: player.flag_cstr(c"pause").unwrap_or(false),
            muted: player.flag_cstr(c"mute").unwrap_or(false),
            ended: player.flag_cstr(c"eof-reached").unwrap_or(false),
            seekable: player.flag_cstr(c"seekable"),
            raw_path,
            raw_position: player.double_cstr(c"time-pos").filter(|&pos| pos >= 0.0),
            raw_duration: player.double_cstr(c"duration").filter(|&dur| dur > 0.0),
            buffered_secs: player.buffered_duration_secs(),
            raw_volume: player
                .double_cstr(c"volume")
                .unwrap_or(100.0)
                .clamp(0.0, 100.0),
            speed: player.double_cstr(c"speed").unwrap_or(1.0),
            current_height: height_num.map(|height| height as u32),
            video_size,
        };
        if refresh_raw_path {
            self.playback.cached_raw_path = telemetry.raw_path.clone();
            self.playback.cached_raw_path_poll = Some(Instant::now());
        }
        Some((video_area, telemetry))
    }

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

    pub(crate) fn add_current_bookmark(&mut self, video: &Video, position: f64) -> bool {
        let Some(id) = history::youtube_video_id(&video.url) else {
            return false;
        };
        let mark = library::Bookmark {
            video_id: id,
            title: video.title.clone(),
            time_ms: (position.max(0.0) * 1000.0).round() as u64,
            note: std::mem::take(&mut self.library.bookmark_note),
        };
        if let Err(error) = self.library.data.add_bookmark(mark) {
            self.library.error = Some(format!("Marcador não salvo: {error}"));
            false
        } else {
            true
        }
    }

    #[cfg(windows)]
    pub(crate) fn apply_player_quality(
        &mut self,
        video: &Video,
        quality: Option<u32>,
        position: f64,
        raw_path: Option<&str>,
        raw_position: Option<f64>,
    ) {
        let Some(player) = &self.player else {
            return;
        };
        let ready = self
            .playback
            .active
            .as_ref()
            .is_some_and(|active| active.has_live_sample)
            && raw_path.is_some_and(|path| path == video.url)
            && raw_position.is_some();
        let preserve_position = self
            .playback
            .active
            .as_ref()
            .is_none_or(|active| !active.is_live || !active.at_live_edge);
        let result = if ready {
            player.set("options/start", "none").and_then(|()| {
                player.quality(
                    &video.url,
                    quality,
                    &self.settings.playback.value.format(),
                    preserve_position,
                )
            })
        } else {
            Err("Aguarde o vídeo carregar antes de alterar a qualidade.".into())
        };
        match result {
            Ok(()) => {
                self.playback.quality = quality;
                self.playback.cached_raw_path_poll = None;
                self.message.clear();
                if let Some(active) = &mut self.playback.active {
                    let pos_ms = (position * 1000.0).round() as u64;
                    if preserve_position && pos_ms > 0 {
                        active.resume_target_ms = Some(pos_ms);
                        active.resume_seek_pending = true;
                        active.start_option_cleared = false;
                    } else {
                        active.resume_target_ms = None;
                        active.resume_seek_pending = false;
                    }
                }
            }
            Err(error) => self.message = error,
        }
    }

    #[cfg(windows)]
    pub(crate) fn description_overlay_chapters(&self, url: &str) -> Vec<overlay::OverlayChapter> {
        let desc_chapters = if self.watch.metadata_cache.valid
            && self.watch.metadata_cache.video_url.as_deref() == Some(url)
        {
            self.watch.metadata_cache.description_chapters.clone()
        } else {
            let raw_desc = self
                .metadata_for(url)
                .and_then(|meta| meta.description.as_deref())
                .or_else(|| {
                    self.watch
                        .live
                        .info
                        .as_ref()
                        .filter(|_| self.watch.live.video_url.as_deref() == Some(url))
                        .and_then(|info| info.description.as_deref())
                });
            raw_desc
                .and_then(|d| clean_description(Some(d)))
                .map(|cleaned| watch_extras::extract_description_chapters(&cleaned))
                .unwrap_or_default()
        };
        desc_chapters
            .into_iter()
            .map(|ch| overlay::OverlayChapter {
                title: ch.title,
                time_secs: ch.seconds as f64,
            })
            .collect()
    }

    #[cfg(windows)]
    pub(crate) fn collect_player_chapters(&mut self) -> Vec<overlay::OverlayChapter> {
        let video_url = self.selected.as_ref().map(|v| v.url.clone());
        let Some(player) = &self.player else {
            return video_url
                .as_deref()
                .map(|url| self.description_overlay_chapters(url))
                .unwrap_or_default();
        };
        let chapter_count_i64 = player
            .int_cstr(c"chapter-list/count")
            .unwrap_or(0)
            .clamp(0, 500);
        let now = Instant::now();
        let needs_refresh = self.watch.overlay_cache.force_refresh
            || self.watch.overlay_cache.chapters_video_url.as_deref() != video_url.as_deref()
            || self.watch.overlay_cache.chapters_count != Some(chapter_count_i64)
            || self
                .watch
                .overlay_cache
                .chapters_last_poll
                .is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(1));
        if needs_refresh {
            let chapter_count = chapter_count_i64 as usize;
            let mut chapters = Vec::with_capacity(chapter_count);
            for index in 0..chapter_count {
                let Some(time_secs) = player.number(&format!("chapter-list/{index}/time")) else {
                    continue;
                };
                let title = player
                    .property(&format!("chapter-list/{index}/title"))
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| format!("Capítulo {}", index + 1));
                chapters.push(overlay::OverlayChapter { title, time_secs });
            }
            if chapters.is_empty()
                && let Some(url) = video_url.as_deref()
            {
                chapters.extend(self.description_overlay_chapters(url));
            }
            self.watch.overlay_cache.chapters_video_url = video_url;
            self.watch.overlay_cache.chapters_count = Some(chapter_count_i64);
            self.watch.overlay_cache.chapters_last_poll = Some(now);
            self.watch.overlay_cache.chapters = chapters;
        }
        self.watch.overlay_cache.chapters.clone()
    }

    fn subtitle_track_label(track: &search::SubtitleTrack) -> String {
        format!(
            "{}{}",
            track.language,
            if track.automatic {
                " (automática)"
            } else {
                ""
            }
        )
    }

    pub(crate) fn collect_overlay_subtitles(
        &mut self,
    ) -> (bool, Vec<overlay::OverlaySubtitleOption>) {
        let mut options = Vec::new();
        #[cfg(windows)]
        let (has_active_player_sub, active_sid) = {
            let active_sid = self
                .player
                .as_ref()
                .and_then(|player| player.property_cstr(c"sid"))
                .filter(|sid| !sid.is_empty() && sid != "no");
            let sub_visible = self
                .player
                .as_ref()
                .is_some_and(|player| player.flag_cstr(c"sub-visibility") != Some(false));
            (active_sid.is_some() && sub_visible, active_sid)
        };
        #[cfg(not(windows))]
        let (has_active_player_sub, _active_sid): (bool, Option<String>) = (false, None);

        let active_yt_url = self.watch.extras.active_subtitle_url.as_deref();
        let has_active_yt_sub = active_yt_url.is_some();

        #[cfg(windows)]
        if let Some(player) = &self.player {
            let video_url = self.selected.as_ref().map(|v| v.url.as_str());
            let count_i64 = player
                .int_cstr(c"track-list/count")
                .unwrap_or(0)
                .clamp(0, 200);
            let yt_empty = self.watch.extras.subtitles.is_empty();
            let now = Instant::now();
            let needs_refresh = self.watch.overlay_cache.force_refresh
                || self.watch.overlay_cache.mpv_subs_video_url.as_deref() != video_url
                || self.watch.overlay_cache.mpv_subs_track_count != Some(count_i64)
                || self.watch.overlay_cache.mpv_subs_yt_empty != yt_empty
                || self
                    .watch
                    .overlay_cache
                    .mpv_subs_last_poll
                    .is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(1));
            if needs_refresh {
                let count = count_i64 as usize;
                let mut mpv_subs = Vec::new();
                for index in 0..count {
                    if player
                        .property(&format!("track-list/{index}/type"))
                        .as_deref()
                        != Some("sub")
                    {
                        continue;
                    }
                    let Some(id) = player.property(&format!("track-list/{index}/id")) else {
                        continue;
                    };
                    let is_external = player
                        .property(&format!("track-list/{index}/external"))
                        .as_deref()
                        == Some("yes");
                    if is_external && !yt_empty {
                        continue;
                    }
                    let label = player
                        .property(&format!("track-list/{index}/title"))
                        .filter(|title| !title.is_empty())
                        .or_else(|| player.property(&format!("track-list/{index}/lang")))
                        .unwrap_or_else(|| format!("Faixa {id}"));
                    mpv_subs.push(app_state::CachedMpvSubtitleTrack { id, label });
                }
                self.watch.overlay_cache.mpv_subs_video_url = video_url.map(str::to_owned);
                self.watch.overlay_cache.mpv_subs_track_count = Some(count_i64);
                self.watch.overlay_cache.mpv_subs_yt_empty = yt_empty;
                self.watch.overlay_cache.mpv_subs_last_poll = Some(now);
                self.watch.overlay_cache.mpv_subs = mpv_subs;
                self.watch.overlay_cache.force_refresh = false;
            }

            for track in &self.watch.overlay_cache.mpv_subs {
                let active = !has_active_yt_sub
                    && has_active_player_sub
                    && active_sid.as_deref() == Some(track.id.as_str());
                options.push(overlay::OverlaySubtitleOption {
                    id: format!("mpv:{}", track.id),
                    label: track.label.clone(),
                    active,
                });
            }
        }

        let mut matched_yt = false;
        for track in &self.watch.extras.subtitles {
            let label = Self::subtitle_track_label(track);
            let active = !matched_yt && active_yt_url == Some(track.url.as_str());
            if active {
                matched_yt = true;
            }
            options.push(overlay::OverlaySubtitleOption {
                id: format!("yt:{}", track.url),
                label,
                active,
            });
        }

        (has_active_player_sub || has_active_yt_sub, options)
    }

    pub(crate) fn select_overlay_subtitle(&mut self, id: Option<&str>, ctx: &egui::Context) {
        self.watch.overlay_cache.mpv_subs_last_poll = None;
        self.watch.overlay_cache.force_refresh = true;
        let Some(raw) = id else {
            let (_, options) = self.collect_overlay_subtitles();
            if let Some(active) = options.into_iter().find(|opt| opt.active) {
                self.playback.last_subtitle_id = Some(active.id);
            }
            self.watch.extras.cancel_active_subtitle();
            #[cfg(windows)]
            if let Some(player) = &self.player
                && let Err(error) = player.disable_subtitles()
            {
                self.message = error;
            }
            return;
        };

        if let Some(sid) = raw.strip_prefix("mpv:") {
            self.watch.extras.cancel_active_subtitle();
            self.playback.last_subtitle_id = Some(raw.to_owned());
            #[cfg(windows)]
            if let Some(player) = &self.player {
                let result = player
                    .set("sid", sid)
                    .and_then(|()| player.set("sub-visibility", "yes"));
                if let Err(error) = result {
                    self.message = error;
                }
            }
            #[cfg(not(windows))]
            let _ = sid;
        } else if let Some(url) = raw.strip_prefix("yt:")
            && let Some(track) = self
                .watch
                .extras
                .subtitles
                .iter()
                .find(|t| t.url == url)
                .cloned()
        {
            if self.watch.extras.active_subtitle_url.as_deref() == Some(track.url.as_str())
                && !self.watch.extras.active_subtitle_cues.is_empty()
                && !self.watch.extras.is_subtitle_applying()
            {
                return;
            }
            let label = Self::subtitle_track_label(&track);
            #[cfg(windows)]
            if let Some(player) = &self.player {
                let _ = player.disable_subtitles();
            }
            self.playback.last_subtitle_id = Some(raw.to_owned());
            self.watch.extras.start_apply_subtitle(&track, label, ctx);
            self.apply_ready_subtitle();
        }
    }

    pub(crate) fn toggle_player_captions(&mut self, ctx: &egui::Context) {
        let (has_active, options) = self.collect_overlay_subtitles();
        if has_active {
            self.select_overlay_subtitle(None, ctx);
            return;
        }
        let candidate = self
            .playback
            .last_subtitle_id
            .as_ref()
            .filter(|prev| options.iter().any(|opt| &opt.id == *prev))
            .cloned()
            .or_else(|| {
                options
                    .iter()
                    .find(|opt| !opt.label.contains("(automática)"))
                    .or_else(|| options.first())
                    .map(|opt| opt.id.clone())
            });
        if let Some(target_id) = candidate {
            self.select_overlay_subtitle(Some(&target_id), ctx);
        } else {
            if !self.watch.extras.subtitles_loaded
                && self.watch.extras.subtitles_loading.is_none()
                && !self.is_playback_metadata_loading()
            {
                self.watch.extras.start_subtitles(ctx);
            }
            self.playback.player_menu = Some(overlay::OverlayMenu::Subtitles);
        }
    }

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
    pub(crate) fn fullscreen_audio_speed_chapters_ui(
        &mut self,
        ui: &mut egui::Ui,
        compact_controls: bool,
        muted: bool,
        palette: ThemePalette,
    ) {
        let selected_url = self.selected.as_ref().map(|v| v.url.clone());
        let Some(player) = &self.player else {
            return;
        };
        ui.add_space(6.0);
        let sound_icon = if muted { Icon::Muted } else { Icon::Volume };
        let mut volume = player.number("volume").unwrap_or(100.0).clamp(0.0, 100.0);
        if ui
            .add(egui::Button::image(sound_icon.image(palette.icon_fg, 17.0)))
            .on_hover_text("Silenciar ou ativar som (M)")
            .clicked()
        {
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
        if ui
            .add_sized(
                [76.0, 20.0],
                egui::Slider::new(&mut volume, 0.0..=100.0).show_value(false),
            )
            .on_hover_text(format!("Volume: {:.0}%", volume))
            .changed()
        {
            match player.set("volume", &format!("{volume:.0}")) {
                Ok(()) => {
                    let vol_rounded = volume.round() as i32;
                    Self::record_player_osd(
                        ui.ctx(),
                        if vol_rounded == 0 {
                            "\u{1f507} Mudo".to_owned()
                        } else {
                            format!("\u{1f50a} {vol_rounded}%")
                        },
                    );
                    if let Some(url) = selected_url.as_deref() {
                        Self::update_player_osd_snapshot_state(ui.ctx(), url, |snap| {
                            snap.volume_rounded = vol_rounded;
                        });
                    }
                }
                Err(error) => self.message = error,
            }
        }
        ui.separator();
        let mut speed = player.number("speed").unwrap_or(1.0);
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
                        self.playback.speed = choice;
                        match player.set("speed", &format!("{choice}")) {
                            Ok(()) => {
                                Self::record_player_osd(ui.ctx(), format!("\u{26a1} {choice:.2}x"));
                                if let Some(url) = selected_url.as_deref() {
                                    Self::update_player_osd_snapshot_state(ui.ctx(), url, |snap| {
                                        snap.speed_hundredths = (choice * 100.0).round() as i32;
                                    });
                                }
                            }
                            Err(error) => self.message = error,
                        }
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
        ui.add_enabled_ui(!self.playback.qualities.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("quality")
                .selected_text(quality_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut quality, None, "Automática");
                    for &height in &self.playback.qualities {
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
        let count = player.number("track-list/count").unwrap_or(0.0) as usize;
        let mut subtitle_tracks = Vec::new();
        for index in 0..count {
            if player
                .property(&format!("track-list/{index}/type"))
                .as_deref()
                == Some("sub")
                && let Some(id) = player.property(&format!("track-list/{index}/id"))
            {
                let label = player
                    .property(&format!("track-list/{index}/title"))
                    .filter(|title| !title.is_empty())
                    .or_else(|| player.property(&format!("track-list/{index}/lang")))
                    .unwrap_or_else(|| format!("Faixa {id}"));
                subtitle_tracks.push((id, label));
            }
        }
        if !subtitle_tracks.is_empty() {
            let current = player.property("sid").unwrap_or_else(|| "no".into());
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

    pub(crate) fn compute_mini_player_size(column_width: f32, available_height: f32) -> egui::Vec2 {
        let avail_w = column_width.max(1.0);
        let avail_h = available_height.max(1.0);
        let width = avail_w.min(avail_h * 16.0 / 9.0);
        let height = width * 9.0 / 16.0;
        egui::vec2(width, height)
    }

    #[cfg(windows)]
    pub(crate) fn player_ui_with_metadata(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        column_width: f32,
        available_height: f32,
        mode: WatchLayoutMode,
        show_metadata: bool,
    ) {
        let Some(video) = self.selected.clone() else {
            self.sync_player_window_visibility();
            return;
        };
        self.sync_player_window_visibility();
        let settings_open = self.ui.settings_open;
        let fullscreen = self.playback.fullscreen;
        let theater_mode = self.playback.theater;
        let chromeless = fullscreen || self.playback.mini_player;

        let player_size = if chromeless
            && !settings_open
            && self
                .player
                .as_ref()
                .is_some_and(player::Player::is_render_api)
        {
            egui::vec2(column_width.max(1.0), available_height.max(1.0))
        } else if self.playback.mini_player {
            Self::compute_mini_player_size(column_width, available_height)
        } else {
            compute_player_size_with_theater(
                column_width,
                available_height,
                fullscreen,
                theater_mode,
                mode,
            )
        };
        let width = player_size.x;
        let compact_controls = watch_controls_are_compact(width);

        let Some((video_area, telemetry)) =
            self.player_surface_and_telemetry(ui, ctx, player_size, settings_open)
        else {
            return;
        };
        for event in telemetry.events.iter().cloned() {
            self.handle_queue_player_event(event);
        }
        self.maybe_start_deferred_quality_fallback(ctx);
        if let Some(error) = telemetry.poll_error.clone() {
            self.message = error;
            self.report_pending_queue_load_error();
        }
        if (!chromeless && show_metadata) || video_area.is_none() {
            ui.add_space(6.0);
        }

        let now = Instant::now();
        let (position, duration, pending_seek, clear_start_option) = self.process_player_telemetry(
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
        let (is_live, is_live_dvr, at_live_edge, live_behind_secs) = self
            .playback
            .active
            .as_ref()
            .filter(|active| active.url == video.url)
            .map_or((false, false, false, 0.0), |active| {
                (
                    active.is_live,
                    active.live_dvr,
                    active.at_live_edge,
                    active.live_behind_secs,
                )
            });
        if !telemetry.ended
            && self.queue.pending_video.as_ref() == Some(&video)
            && self.playback.active.as_ref().is_some_and(|active| {
                active.has_live_sample
                    && active.url == video.url
                    && telemetry.raw_path.as_deref().is_some_and(|path| {
                        path == video.url
                            || (history::youtube_video_id(path).is_some()
                                && history::youtube_video_id(path)
                                    == history::youtube_video_id(&video.url))
                    })
            })
        {
            self.report_pending_queue_playback_observed();
        }
        if clear_start_option && let Some(player) = &self.player {
            let _ = player.set("options/start", "none");
        }
        if telemetry.speed.is_finite() && telemetry.speed > 0.0 {
            self.playback.speed = telemetry.speed;
        }
        if let Some(target_secs) = pending_seek {
            if let Some(player) = &self.player
                && let Err(error) = player.seek(target_secs, "absolute+exact")
            {
                if let Some(active) = &mut self.playback.active {
                    active.resume_seek_pending = true;
                }
                self.message = error;
            }
        } else if self.playback.seek_drag.is_none()
            && !self.enforce_ab_loop_seek(position, duration, is_live, now)
            && let Some(skip_target) =
                self.try_auto_skip_sponsor_segment(position, duration, is_live, now)
            && let Some(player) = &self.player
        {
            match player.seek(skip_target, "absolute+exact") {
                Ok(()) => self.record_manual_seek(skip_target, duration, now),
                Err(error) => self.message = error,
            }
        }
        if self
            .playback
            .active
            .as_ref()
            .is_some_and(|active| active.has_live_sample && active.url == video.url)
            && pending_seek.is_none()
        {
            self.sync_player_osd_from_telemetry(
                ctx,
                &video,
                &telemetry,
                position,
                duration,
                is_live,
                is_live_dvr,
            );
        }

        let next_fallback_video = if self.queue.data.is_empty() {
            if mode == WatchLayoutMode::Short {
                self.next_or_prev_short(true)
            } else {
                self.next_recommended_video()
            }
        } else {
            None
        };
        let has_queued = !self.queue.data.is_empty() || next_fallback_video.is_some();
        let palette = self.palette();
        let mut actions = FullscreenControlsActions::default();
        // Show external control bar only when falling back to a native HWND.
        let show_hwnd_fullscreen_bar = if video_area.is_none() && !settings_open && fullscreen {
            let stage_rect = ctx.input(|i| i.content_rect());
            self.update_player_overlay_pointer_activity(ctx, stage_rect);
            let idle_secs = self.playback.overlay_last_activity.elapsed().as_secs_f64();
            let bar_visible = overlay::overlay_visible_with_state(
                false,
                telemetry.paused,
                telemetry.ended,
                self.playback.player_context_menu.is_some(),
                self.playback.seek_drag.is_some(),
                idle_secs,
            );
            if !bar_visible
                && !self.autoscroll.is_active()
                && !self.ui.shortcuts_open
                && !self.ui.command_palette.open
            {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            }
            bar_visible
        } else {
            true
        };
        if video_area.is_none() && !settings_open && show_hwnd_fullscreen_bar {
            self.fullscreen_seek_ui(
                ui,
                width,
                position,
                duration,
                is_live,
                is_live_dvr,
                &video,
                now,
            );
            let bar = FullscreenBarContext {
                width,
                compact_controls,
                fullscreen,
                theater_mode,
                has_queued,
                position,
                duration,
                is_live,
                at_live_edge,
                live_behind_secs,
                palette,
            };
            self.fullscreen_controls_bar_ui(ui, ctx, &video, &bar, &telemetry, &mut actions);
        }

        if actions.library_changed {
            self.save_library();
        }

        if let Some((rect, _)) = video_area.as_ref()
            && !self.autoscroll.is_active()
        {
            self.update_player_overlay_pointer_activity(ctx, *rect);
        }
        let overlay_active = video_area.as_ref().is_some_and(|(rect, _)| {
            let pointer_pos =
                ui.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()));
            let pointer_keeps_visible =
                overlay::should_keep_overlay_visible_for_pointer(fullscreen, pointer_pos, *rect);
            let any_menu_open =
                self.playback.player_menu.is_some() || self.playback.player_context_menu.is_some();
            overlay::overlay_visible_with_state(
                pointer_keeps_visible,
                telemetry.paused,
                telemetry.ended,
                any_menu_open,
                self.playback.seek_drag.is_some(),
                self.playback.overlay_last_activity.elapsed().as_secs_f64(),
            )
        });
        let chapters = if overlay_active {
            self.collect_player_chapters()
        } else {
            Vec::new()
        };
        let (has_active_subtitle, subtitles) = if overlay_active {
            self.collect_overlay_subtitles()
        } else {
            (self.watch.extras.active_subtitle_url.is_some(), Vec::new())
        };
        let overlay_ctx = OverlayPlaybackContext {
            video: video.clone(),
            paused: telemetry.paused,
            ended: telemetry.ended,
            muted: telemetry.muted,
            position,
            duration,
            buffered_secs: telemetry.buffered_secs,
            raw_volume: telemetry.raw_volume,
            speed: telemetry.speed,
            current_height: telemetry.current_height,
            selected_quality: self.playback.quality,
            qualities: self.playback.qualities.clone(),
            qualities_loading: self.is_playback_metadata_loading(),
            has_queued,
            has_active_subtitle,
            subtitles,
            subtitles_loading: self.watch.extras.subtitles_loading.is_some()
                || self.watch.extras.is_subtitle_applying()
                || (!self.watch.extras.subtitles_loaded && self.is_playback_metadata_loading()),
            chapters,
            theater: theater_mode,
            fullscreen,
            mini_player: self.playback.mini_player,
            autoplay: self.playback.autoplay,
            loop_video: self.playback.loop_video,
            is_live,
            is_live_dvr,
            at_live_edge,
            live_behind_secs,
            raw_path: telemetry.raw_path.clone(),
            raw_position: telemetry.raw_position,
            now,
        };
        let osd_video_rect = video_area.as_ref().map(|(rect, _)| *rect);
        let overlay_action = self.player_overlay_ui(ui, video_area, &overlay_ctx, ctx);
        if let Some(action) = overlay_action {
            self.handle_player_overlay_action(action, &overlay_ctx, ctx);
        }
        if !settings_open && let Some(video_rect) = osd_video_rect {
            Self::paint_player_osd_pill(ctx, video_rect);
        }

        if actions.go_live_clicked {
            self.go_live_edge(&video);
        } else if let Some(seek_pos) = actions.manual_seek_recorded {
            self.record_manual_seek(seek_pos, duration, now);
        } else if actions.flush_on_pause {
            self.flush_playback_history_at(now);
        }

        if let Some(size) = telemetry.video_size {
            self.playback.last_video_size_url = Some(video.url.clone());
            self.playback.last_video_size = Some(size);
        }
        if show_metadata {
            let resolution_badge = telemetry
                .video_size
                .or_else(|| {
                    (self.playback.last_video_size_url.as_deref() == Some(video.url.as_str()))
                        .then_some(self.playback.last_video_size)
                        .flatten()
                })
                .map(|size| format!("Reproduzindo em {size}"));
            self.watch_metadata_card_ui(ui, &video, width, resolution_badge.as_deref());
        }

        if actions.settings_toggled {
            let opening = !self.ui.settings_open;
            if opening {
                if self.playback.fullscreen {
                    self.toggle_fullscreen(ctx);
                }
                if self.playback.mini_player {
                    self.toggle_mini_player(ctx);
                }
            }
            self.ui.settings_open = opening;
            self.sync_player_window_visibility();
        }
        if actions.theater_toggled {
            self.toggle_theater_mode();
        }
        if actions.fullscreen_toggled {
            self.toggle_fullscreen(ctx);
        }
        if actions.next_clicked {
            if !self.queue.data.is_empty() {
                self.request_queue_advance();
            } else if let Some(next_video) = next_fallback_video {
                self.playback.pending_open_video = Some(next_video);
            } else {
                self.request_queue_advance();
            }
        }
        if actions.close_clicked {
            self.close_video(ctx);
        }
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
            #[cfg(windows)]
            {
                let needs_audio_poll = self.playback.audio_tracks_video_url.as_deref()
                    != Some(playback.video.url.as_str())
                    || self
                        .playback
                        .audio_tracks_last_poll
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
                    || self.playback.player_menu == Some(overlay::OverlayMenu::AudioTracks);
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
                qualities: playback.qualities.clone(),
                qualities_loading: playback.qualities_loading,
                has_queued: playback.has_queued,
                has_active_subtitle: playback.has_active_subtitle,
                active_subtitle_text,
                subtitles: playback.subtitles.clone(),
                subtitles_loading: playback.subtitles_loading,
                audio_tracks: self.playback.audio_tracks.clone(),
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
            };
            let (action, mut over_bar) = overlay::show(ui, rect, &telemetry, &style, visible);
            let mut over_stats = false;
            if self.playback.stats_for_nerds_open {
                self.refresh_stats_for_nerds(false);
                let stats_data = self.build_stats_for_nerds_data(rect, playback, ctx);
                let (stats_action, pointer_over_stats) =
                    overlay::show_stats_for_nerds(ui, rect, &stats_data, &style);
                over_stats = pointer_over_stats;
                over_bar |= over_stats;
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

            if let Some(action) = action
                && !was_context_menu_open
                && !just_opened_context_menu
            {
                self.playback.overlay_last_activity = Instant::now();
                overlay_action = Some(action);
            } else if !was_context_menu_open
                && !just_opened_context_menu
                && self.playback.player_context_menu.is_none()
                && !over_bar
            {
                if response.double_clicked() {
                    self.playback.overlay_last_activity = Instant::now();
                    if !menu_open {
                        self.handle_player_overlay_action(
                            overlay::OverlayAction::TogglePlay,
                            playback,
                            ctx,
                        );
                    }
                    overlay_action = Some(overlay::OverlayAction::ToggleFullscreen);
                } else if playback.mini_player
                    && !menu_open
                    && self.playback.seek_drag.is_none()
                    && response.drag_started_by(egui::PointerButton::Primary)
                {
                    self.playback.overlay_last_activity = Instant::now();
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                } else if response.clicked() {
                    self.playback.overlay_last_activity = Instant::now();
                    overlay_action = if menu_open {
                        Some(overlay::OverlayAction::SetMenu(None))
                    } else {
                        Some(overlay::OverlayAction::TogglePlay)
                    };
                }
            }

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
                {
                    let result = self
                        .player
                        .as_ref()
                        .map(|player| player.set("volume", &format!("{volume:.0}")))
                        .unwrap_or(Ok(()));
                    match result {
                        Ok(()) => {
                            let vol_rounded = f64::from(volume).round() as i32;
                            Self::record_player_osd(
                                ctx,
                                if vol_rounded == 0 {
                                    "\u{1f507} Mudo".to_owned()
                                } else {
                                    format!("\u{1f50a} {vol_rounded}%")
                                },
                            );
                            Self::update_player_osd_snapshot_state(
                                ctx,
                                &playback.video.url,
                                |snap| {
                                    snap.volume_rounded = vol_rounded;
                                },
                            );
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
            OverlayAction::Next => {
                if !self.queue.data.is_empty() {
                    self.request_queue_advance();
                } else if self.is_watch_short_mode() {
                    if let Some(video) = self.next_or_prev_short(true) {
                        self.playback.pending_open_video = Some(video);
                    }
                } else if let Some(video) = self.next_recommended_video() {
                    self.playback.pending_open_video = Some(video);
                } else {
                    self.request_queue_advance();
                }
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
                #[cfg(windows)]
                if let Some(player) = &self.player {
                    if let Err(error) = player.set_audio_track(&id) {
                        self.message = error;
                    } else {
                        for track in &mut self.playback.audio_tracks {
                            track.selected = track.id == id;
                        }
                        self.playback.audio_tracks_last_poll = None;
                    }
                }
                #[cfg(not(windows))]
                let _ = id;
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
                self.playback.brightness = 0;
                self.playback.contrast = 0;
                self.playback.saturation = 0;
                #[cfg(windows)]
                if let Some(player) = &self.player
                    && let Err(error) = player.set_video_eq(0, 0, 0)
                {
                    self.message = error;
                }
            }
            OverlayAction::SetSpeed(speed) => {
                let clamped = speed.clamp(0.25, 3.0);
                self.set_player_speed(clamped);
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
                if menu == Some(overlay::OverlayMenu::Quality)
                    && self.playback.qualities.is_empty()
                    && self.playback.quality_rx.is_none()
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
            OverlayAction::Close => self.close_video(ctx),
        }
    }

    pub(crate) fn set_player_speed(&mut self, speed: f64) {
        let clamped = speed.clamp(0.25, 3.0);
        self.playback.speed = clamped;
        #[cfg(windows)]
        if let Some(player) = &self.player
            && let Err(error) = player.set("speed", &format!("{clamped}"))
        {
            self.message = error;
        }
    }

    pub(crate) fn toggle_audio_only_mode(&mut self) {
        let next = !self.playback.audio_only;
        self.playback.audio_only = next;
        #[cfg(windows)]
        if let Some(player) = &self.player
            && let Err(error) = player.set_audio_only(next)
        {
            self.message = error;
            return;
        }
        self.sync_player_window_visibility();
        self.message = if next {
            "Modo somente \u{00e1}udio ativado.".into()
        } else {
            "Modo somente \u{00e1}udio desativado.".into()
        };
    }

    pub(crate) fn current_playback_position_for_ab(&self, fallback: Option<f64>) -> f64 {
        #[cfg(windows)]
        if let Some(player) = &self.player
            && let Some(pos) = player
                .double_cstr(c"time-pos")
                .filter(|&p| p.is_finite() && p >= 0.0)
        {
            return pos;
        }
        if let Some(pos) = fallback.filter(|&p| p.is_finite() && p >= 0.0) {
            return pos;
        }
        self.playback
            .active
            .as_ref()
            .map_or(0.0, |active| active.position_ms as f64 / 1000.0)
    }

    pub(crate) fn set_ab_loop_point_a(&mut self, pos_secs: f64) {
        if self.active_video_is_live() {
            self.message = "Loop A-B indispon\u{00ed}vel em transmiss\u{00f5}es ao vivo.".into();
            return;
        }
        self.playback.set_loop_a(pos_secs);
        self.message = match self.playback.ab_loop_active() {
            Some((a, b)) => format!(
                "Loop A-B ativo: {} \u{2013} {}",
                overlay::format_time(a),
                overlay::format_time(b)
            ),
            None => format!("Ponto A definido em {}", overlay::format_time(pos_secs)),
        };
    }

    pub(crate) fn set_ab_loop_point_b(&mut self, pos_secs: f64) {
        if self.active_video_is_live() {
            self.message = "Loop A-B indispon\u{00ed}vel em transmiss\u{00f5}es ao vivo.".into();
            return;
        }
        self.playback.set_loop_b(pos_secs);
        self.message = match self.playback.ab_loop_active() {
            Some((a, b)) => format!(
                "Loop A-B ativo: {} \u{2013} {}",
                overlay::format_time(a),
                overlay::format_time(b)
            ),
            None => format!(
                "Ponto B definido em {} (defina A antes de B)",
                overlay::format_time(pos_secs)
            ),
        };
    }

    pub(crate) fn clear_ab_loop_points(&mut self) {
        self.playback.clear_ab_loop();
        self.message = "Loop A-B limpo.".into();
    }

    pub(crate) fn enforce_ab_loop_seek(
        &mut self,
        position: f64,
        duration: Option<f64>,
        is_live: bool,
        now: Instant,
    ) -> bool {
        if is_live || !position.is_finite() {
            return false;
        }
        let Some((a, b)) = self.playback.ab_loop_active() else {
            return false;
        };
        if position < b {
            return false;
        }
        if let Some(last) = self.playback.ab_loop_last_seek_at
            && now.duration_since(last) < std::time::Duration::from_millis(350)
        {
            return false;
        }
        self.playback.ab_loop_last_seek_at = Some(now);
        #[cfg(windows)]
        if let Some(player) = &self.player
            && let Err(error) = player.seek(a, "absolute+exact")
        {
            self.message = error;
            return false;
        }
        self.record_manual_seek(a, duration, now);
        true
    }

    pub(crate) fn capture_player_screenshot(&mut self, video: &Video, position_secs: f64) {
        if self.playback.audio_only {
            self.message =
                "Captura de quadro indispon\u{00ed}vel no modo somente \u{00e1}udio.".into();
            return;
        }
        let candidates = screenshot_dir_candidates(
            std::env::var("USERPROFILE").ok().as_deref(),
            std::env::var("LOCALAPPDATA").ok().as_deref(),
        );
        let mut target_dir = None;
        let mut last_dir_err: Option<std::io::Error> = None;
        for dir in candidates {
            match std::fs::create_dir_all(&dir) {
                Ok(()) => {
                    target_dir = Some(dir);
                    break;
                }
                Err(err) => last_dir_err = Some(err),
            }
        }
        let Some(dir) = target_dir else {
            self.message = format!(
                "N\u{00e3}o foi poss\u{00ed}vel criar pasta de capturas: {}",
                last_dir_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "diret\u{00f3}rio indispon\u{00ed}vel".into())
            );
            return;
        };
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let filename = build_screenshot_filename(video, position_secs, now_secs);
        let path = dir.join(&filename);
        #[cfg(windows)]
        {
            let Some(player) = &self.player else {
                self.message = format!("Player n\u{00e3}o inicializado para capturar {filename}.");
                return;
            };
            match player.screenshot_to_file(&path) {
                Ok(()) => self.message = format!("Captura salva: {filename}"),
                Err(error) => self.message = error,
            }
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            self.message = format!("Captura salva: {filename}");
        }
    }

    pub(crate) fn watch_primary_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        column_width: f32,
        available_height: f32,
        mode: WatchLayoutMode,
    ) {
        let show_metadata = !self.playback.fullscreen
            && !self.playback.mini_player
            && mode != WatchLayoutMode::Short;
        self.watch_primary_with_metadata_ui(
            ui,
            ctx,
            column_width,
            available_height,
            mode,
            show_metadata,
        );
    }

    pub(crate) fn watch_primary_with_metadata_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        column_width: f32,
        available_height: f32,
        mode: WatchLayoutMode,
        show_metadata: bool,
    ) {
        self.sync_theme_visuals(ctx);
        self.sync_player_window_visibility();
        #[cfg(windows)]
        if self.player.is_some() {
            self.player_ui_with_metadata(
                ui,
                ctx,
                column_width,
                available_height,
                mode,
                show_metadata,
            );
            return;
        }
        let Some(video) = self.selected.clone() else {
            return;
        };
        let player_size = if self.playback.mini_player {
            Self::compute_mini_player_size(column_width, available_height)
        } else {
            compute_player_size_with_theater(
                column_width,
                available_height,
                self.playback.fullscreen,
                self.playback.theater,
                mode,
            )
        };
        let (rect, _) = ui.allocate_exact_size(player_size, egui::Sense::hover());
        let corner_radius =
            if self.playback.fullscreen || self.playback.theater || self.playback.mini_player {
                0.0
            } else {
                12.0
            };
        ui.painter()
            .rect_filled(rect, corner_radius, Color32::BLACK);
        let compact_controls = watch_controls_are_compact(player_size.x);
        self.paint_hidden_video_notice(ui, rect);
        if !self.ui.settings_open {
            Self::paint_player_osd_pill(ctx, rect);
        }
        ui.add_space(6.0);
        let mut theater_toggled = false;
        let mut fullscreen_toggled = false;
        let mut settings_toggled = false;
        let mut close_clicked = false;
        let mut next_clicked = false;
        let next_fallback_video = if self.queue.data.is_empty() {
            if mode == WatchLayoutMode::Short {
                self.next_or_prev_short(true)
            } else {
                self.next_recommended_video()
            }
        } else {
            None
        };
        let has_queued = !self.queue.data.is_empty() || next_fallback_video.is_some();
        let fullscreen = self.playback.fullscreen;
        let theater_mode = self.playback.theater;
        let palette = self.palette();
        egui::Frame::new()
            .fill(palette.player_bar_bg)
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_max_width(player_size.x);
                ui.horizontal_wrapped(|ui| {
                    if styled_compact_button_enabled(ui, has_queued, "⏭ Próximo", false, &palette)
                        .on_hover_text("Avançar para o próximo vídeo")
                        .clicked()
                    {
                        next_clicked = true;
                    }
                    if styled_icon_chip_button(ui, true, "⚙", &palette)
                        .on_hover_text("Abrir ou fechar configurações do player")
                        .clicked()
                    {
                        settings_toggled = true;
                    }
                    let theater_button_label = if compact_controls {
                        if theater_mode { "Normal" } else { "Teatro" }
                    } else if theater_mode {
                        "▭ Normal"
                    } else {
                        "▭ Teatro"
                    };
                    let theater_button = egui::Button::new(
                        RichText::new(theater_button_label)
                            .size(12.0)
                            .strong()
                            .color(if theater_mode {
                                palette.active_button_fg
                            } else {
                                palette.theater_inactive_fg
                            }),
                    )
                    .fill(if theater_mode {
                        palette.active_button_bg
                    } else {
                        palette.theater_inactive_bg
                    })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if theater_mode {
                            palette.active_button_bg
                        } else {
                            palette.card_alt_stroke
                        },
                    ))
                    .corner_radius(13)
                    .min_size(egui::vec2(0.0, 25.0));
                    if ui
                        .add_enabled(!fullscreen, theater_button)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(if fullscreen {
                            "Modo teatro indisponível enquanto a tela cheia estiver ativa"
                        } else if theater_mode {
                            "Sair do modo teatro e restaurar visualização padrão (T)"
                        } else {
                            "Ativar modo teatro na janela (T)"
                        })
                        .clicked()
                    {
                        theater_toggled = true;
                    }
                    let screen_icon = if fullscreen {
                        Icon::Minimize
                    } else {
                        Icon::Maximize
                    };
                    if ui
                        .add(
                            egui::Button::image(screen_icon.image(palette.icon_fg, 16.0))
                                .fill(palette.header_button_bg)
                                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(8)
                                .min_size(egui::vec2(26.0, 25.0)),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Tela cheia (F ou Esc para sair)")
                        .clicked()
                    {
                        fullscreen_toggled = true;
                    }
                    if ui
                        .add(
                            egui::Button::image(Icon::Close.image(palette.icon_fg, 16.0))
                                .fill(palette.header_button_bg)
                                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(8)
                                .min_size(egui::vec2(26.0, 25.0)),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Fechar vídeo")
                        .clicked()
                    {
                        close_clicked = true;
                    }
                });
            });
        if show_metadata {
            self.watch_metadata_card_ui(ui, &video, player_size.x, None);
        }
        if settings_toggled {
            let opening = !self.ui.settings_open;
            if opening {
                if self.playback.fullscreen {
                    self.toggle_fullscreen(ctx);
                }
                if self.playback.mini_player {
                    self.toggle_mini_player(ctx);
                }
            }
            self.ui.settings_open = opening;
            self.sync_player_window_visibility();
        }
        if theater_toggled {
            self.toggle_theater_mode();
        }
        if fullscreen_toggled {
            self.toggle_fullscreen(ctx);
        }
        if next_clicked {
            if !self.queue.data.is_empty() {
                self.request_queue_advance();
            } else if let Some(next_video) = next_fallback_video {
                self.playback.pending_open_video = Some(next_video);
            } else {
                self.request_queue_advance();
            }
        }
        if close_clicked {
            self.close_video(ctx);
        }
    }

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
            {
                stats.push(relative_date);
            }
            return stats;
        }
        if let Some(views) = metadata.and_then(|meta| meta.view_count) {
            stats.push(format_view_count(views));
        }
        if let Some(relative_date) = metadata
            .and_then(|meta| meta.upload_date.as_deref())
            .and_then(|date| recommendations::format_youtube_relative_time(date, None))
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

        let channel_width = (display.content_width - 110.0).max(120.0);
        ui.vertical(|ui| {
            ui.set_max_width(channel_width);
            let clean_channel = truncate_text_chars(&channel_label, 48);
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
                .on_hover_text(if let Some(target) = display.channel_target {
                    format!("Abrir canal ({})", target.canonical_url())
                } else {
                    "Abrir detalhes do canal".to_owned()
                })
                .clicked()
            {
                open_channel_target = display.channel_target.cloned();
            }
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

            egui::Frame::new()
                .fill(palette.header_button_bg)
                .corner_radius(18)
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let like_button =
                            egui::Button::new(RichText::new(like_text).size(12.5).strong().color(
                                if like_active {
                                    palette.active_button_fg
                                } else {
                                    palette.header_button_fg
                                },
                            ))
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

                        let dislike_button =
                            egui::Button::new(RichText::new(dislike_text).size(13.5).color(
                                if dislike_active {
                                    palette.active_button_fg
                                } else {
                                    palette.header_button_fg
                                },
                            ))
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

    pub(crate) fn watch_metadata_description_with_stats_ui(
        &mut self,
        ui: &mut egui::Ui,
        video_url: &str,
        content_width: f32,
        stats: &[String],
        technical_resolution: Option<&str>,
        palette: &ThemePalette,
    ) {
        let chapters = self.watch.metadata_cache.description_chapters.clone();
        if !chapters.is_empty() {
            self.watch_description_chapters_ui(
                ui,
                ("watch_desc_chapters", video_url),
                &chapters,
                palette,
            );
        }
        let has_desc = self.watch.metadata_cache.cleaned_description.is_some();
        if !stats.is_empty() || has_desc {
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
                    if let Some(lines) = lines.as_deref()
                        && !lines.is_empty()
                    {
                        if !stats.is_empty() {
                            ui.add_space(6.0);
                        }
                        const COLLAPSED_LINES: usize = 3;
                        let needs_collapse = lines.len() > COLLAPSED_LINES;
                        let expanded =
                            self.watch.metadata_cache.description_expanded || !needs_collapse;
                        let visible_lines = if expanded {
                            lines
                        } else {
                            &lines[..COLLAPSED_LINES]
                        };
                        self.render_cached_timestamp_lines_ui(
                            ui,
                            visible_lines,
                            palette.description_fg,
                            13.0,
                        );
                        if expanded {
                            ui.add_space(12.0);
                            Self::paint_hairline_divider(ui, palette);
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(if pt { "Transcrição" } else { "Transcript" })
                                    .size(14.0)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(if pt {
                                    "Acompanhe usando a transcrição."
                                } else {
                                    "Follow along using the transcript."
                                })
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                            );
                            ui.add_space(6.0);
                            let transcript_active = self.watch.extras.sidebar_tab
                                == watch_extras::SidebarTab::Transcript;
                            if styled_pill_button(
                                ui,
                                if pt {
                                    "Mostrar transcrição"
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
                    }
                });
            if let Some(expanded) = next_expanded {
                self.watch.metadata_cache.description_expanded = expanded;
            }
            if open_transcript {
                self.channels.view_open = false;
                self.watch.extras.sidebar_tab = watch_extras::SidebarTab::Transcript;
                self.watch.extras.tab = watch_extras::Tab::Results;
                if !self.watch.extras.subtitles_loaded
                    && self.watch.extras.subtitles_loading.is_none()
                    && !self.is_playback_metadata_loading()
                {
                    let ctx = ui.ctx().clone();
                    self.watch.extras.start_subtitles(&ctx);
                }
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
        let sub_pending = self.auth.authenticated && self.auth.sub_toggle_rx.is_some();
        let palette = self.palette();
        let content_width = width.min(ui.available_width()).max(1.0);
        let save_menu_open = self.playback.save_menu_open;
        let video_id = history::youtube_video_id(&video.url);
        let current_rating = video_id
            .as_ref()
            .and_then(|id| self.auth.ratings.get(id).copied());
        let rating_pending = self.auth.rating_rx.is_some();
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
                && self.watch.extras.comments_error.is_none()
                && self
                    .auth
                    .session
                    .as_ref()
                    .is_some_and(oauth::TokenSession::has_comment_scope)
            {
                self.watch
                    .extras
                    .start_comments(self.auth.session.as_ref(), false, &ctx);
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
            self.channels.view_open = false;
            self.watch.extras.sidebar_tab = watch_extras::SidebarTab::Transcript;
            self.watch.extras.tab = watch_extras::Tab::Results;
            if !self.watch.extras.subtitles_loaded
                && self.watch.extras.subtitles_loading.is_none()
                && !self.is_playback_metadata_loading()
            {
                self.watch.extras.start_subtitles(&ctx);
            }
        }
        if action_flags.open_report_modal {
            self.feed.search_report_reason = None;
            self.feed.search_report_details.clear();
            self.feed.search_report_modal_video = Some(video.clone());
        }
        if let Some(format) = action_flags.download_format {
            self.start_download_video(video.clone(), format, &ctx);
        }
        if let Some(target) = open_channel_target {
            self.open_channel(target, &ctx);
        }
    }

    pub(crate) fn argb_u32_to_color32(argb: u32) -> Color32 {
        let r = ((argb >> 16) & 0xff) as u8;
        let g = ((argb >> 8) & 0xff) as u8;
        let b = (argb & 0xff) as u8;
        Color32::from_rgb(r, g, b)
    }

    pub(crate) fn watch_live_chat_panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        width: f32,
        max_height: f32,
    ) -> bool {
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
            let dark_theme = self.settings.theme.value == Theme::Dark;
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
                            RichText::new("Chat ao vivo indisponível no momento.")
                                .size(12.0)
                                .color(palette.neutral_status_fg),
                        )
                        .on_hover_text(err);
                    });
                return true;
            }
            return false;
        }

        if !self.watch.live.chat_visible {
            let show_btn = egui::Button::new(
                RichText::new("💬 Mostrar chat ao vivo")
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
                            RichText::new("Chat ao vivo")
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if styled_compact_button(ui, "Ocultar chat", false, &palette).clicked() {
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
                            .hint_text("Filtrar por usuário ou palavra..."),
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
                            .on_hover_text("Limpar filtro do chat")
                            .clicked()
                    {
                        self.watch.live.filter_query.clear();
                    }
                });
                ui.add_space(6.0);
                Self::paint_hairline_divider(ui, &palette);
                ui.add_space(6.0);

                let list_height = (max_height - 160.0).clamp(160.0, 420.0);
                let stick_bottom = self.watch.live.auto_scroll;
                let filter_query = self.watch.live.filter_query.clone();
                let has_filter = !filter_query.trim().is_empty();
                let messages = std::mem::take(&mut self.watch.live.messages);
                let chat_loading = self.watch.live.chat_loading;
                let chat_error = self.watch.live.chat_error.clone();

                let scroll_output = egui::ScrollArea::vertical()
                    .id_salt("watch_live_chat_scroll")
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .stick_to_bottom(stick_bottom)
                    .show(ui, |ui| {
                        ui.set_width((panel_width - 34.0).max(160.0));
                        if messages.is_empty() {
                            if chat_loading {
                                ui.add_space(6.0);
                                let dark_theme = self.settings.theme.value == Theme::Dark;
                                skeleton::live_chat_skeleton(
                                    ui,
                                    (panel_width - 34.0).max(160.0),
                                    dark_theme,
                                );
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
                                                    RichText::new(
                                                        "Aguardando mensagens do chat ao vivo...",
                                                    )
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                                );
                                            }
                                        });
                                    });
                            }
                            return;
                        }

                        let mut matched_count = 0usize;
                        for msg in &messages {
                            if !live_chat::message_matches_filter(msg, &filter_query) {
                                continue;
                            }
                            matched_count += 1;
                            match &msg.kind {
                                live_chat::LiveChatMessageKind::Engagement => {
                                    egui::Frame::new()
                                        .fill(palette.card_alt_bg)
                                        .corner_radius(8)
                                        .inner_margin(egui::Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(&msg.text)
                                                    .size(11.5)
                                                    .color(palette.neutral_status_fg),
                                            );
                                        });
                                    ui.add_space(4.0);
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
                                                        .inner_margin(egui::Margin::symmetric(5, 1))
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
                                            if !msg.text.is_empty() {
                                                ui.add_space(3.0);
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&msg.text)
                                                            .size(12.5)
                                                            .color(Color32::WHITE),
                                                    )
                                                    .wrap(),
                                                );
                                            }
                                        });
                                    ui.add_space(5.0);
                                }
                                live_chat::LiveChatMessageKind::Membership { header_subtext } => {
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
                                            if !msg.text.is_empty() {
                                                ui.add_space(3.0);
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&msg.text)
                                                            .size(12.5)
                                                            .color(Color32::WHITE),
                                                    )
                                                    .wrap(),
                                                );
                                            }
                                        });
                                    ui.add_space(5.0);
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
                                                let author_color = if msg
                                                    .badges
                                                    .contains(&live_chat::LiveChatBadgeKind::Owner)
                                                {
                                                    Color32::from_rgb(255, 214, 0)
                                                } else if msg.badges.contains(
                                                    &live_chat::LiveChatBadgeKind::Moderator,
                                                ) {
                                                    Color32::from_rgb(94, 158, 255)
                                                } else if msg
                                                    .badges
                                                    .contains(&live_chat::LiveChatBadgeKind::Member)
                                                {
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
                                                                "Proprietário",
                                                                Color32::from_rgb(255, 214, 0),
                                                                Color32::BLACK,
                                                            ),
                                                            live_chat::LiveChatBadgeKind::Moderator => (
                                                                "Mod",
                                                                Color32::from_rgb(47, 111, 216),
                                                                Color32::WHITE,
                                                            ),
                                                            live_chat::LiveChatBadgeKind::Member => (
                                                                "Membro",
                                                                Color32::from_rgb(43, 166, 64),
                                                                Color32::WHITE,
                                                            ),
                                                            live_chat::LiveChatBadgeKind::Verified => (
                                                                "✓",
                                                                palette.header_button_bg,
                                                                palette.channel_fg,
                                                            ),
                                                        };
                                                    egui::Frame::new()
                                                        .fill(badge_bg)
                                                        .corner_radius(4)
                                                        .inner_margin(egui::Margin::symmetric(4, 1))
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
                                            if !msg.text.is_empty() {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&msg.text)
                                                            .size(12.5)
                                                            .color(palette.channel_fg),
                                                    )
                                                    .wrap(),
                                                );
                                            }
                                        });
                                    });
                                    ui.add_space(5.0);
                                }
                            }
                        }
                        if has_filter && matched_count == 0 {
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
                                            RichText::new("Nenhuma mensagem corresponde ao filtro.")
                                                .size(12.0)
                                                .color(palette.neutral_status_fg),
                                        );
                                    });
                                });
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
                                RichText::new("Rolagem pausada — ↓ Ir para o fim")
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
                    .or_else(|| (!self.watch.live.messages.is_empty()).then_some(self.watch.live.chat_error.as_deref()).flatten())
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

                if self.watch.live.send_params.is_some() {
                    let sending = self.watch.live.sending;
                    ui.horizontal(|ui| {
                        let input_w = (ui.available_width() - 78.0).max(100.0);
                        let input_resp = ui.add_enabled(
                            !sending,
                            egui::TextEdit::singleline(&mut self.watch.live.composer_draft)
                                .desired_width(input_w)
                                .hint_text("Conversar..."),
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
                            RichText::new(if sending { "..." } else { "Enviar" })
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
                } else {
                    ui.label(
                        RichText::new("Entre no YouTube para enviar mensagens no chat.")
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

        let _ = ctx;
        let mut clicked_tab = None;
        egui::ScrollArea::horizontal()
            .id_salt("watch_sidebar_filter_chips")
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                    for (label, tab) in &tabs {
                        let is_selected =
                            self.watch.extras.sidebar_tab == *tab && !self.channels.view_open;
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
                        let resp = ui
                            .add_sized([0.0, 32.0], btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            clicked_tab = Some(*tab);
                        }
                    }
                });
            });

        if let Some(tab) = clicked_tab {
            self.channels.view_open = false;
            self.watch.extras.sidebar_tab = tab;
            self.watch.extras.tab = watch_extras::Tab::Results;
            if matches!(tab, watch_extras::SidebarTab::Transcript)
                && !self.watch.extras.subtitles_loaded
                && self.watch.extras.subtitles_loading.is_none()
            {
                self.watch.extras.start_subtitles(ctx);
            }
            if matches!(tab, watch_extras::SidebarTab::FromChannel)
                && let Some(video) = self.selected.clone()
                && !video.channel.trim().is_empty()
            {
                let ch_key = recommendations::normalize_channel_key(&video.channel);
                let has_channel_videos = self.feed.videos.iter().any(|v| {
                    v.url != video.url
                        && recommendations::normalize_channel_key(&v.channel) == ch_key
                });
                if !has_channel_videos && self.feed.search.rx.is_none() {
                    self.feed.query = video.channel.clone();
                    self.start_search(ctx);
                }
            }
        }
    }

    pub(crate) fn watch_sidebar_queue_section_ui(&mut self, ui: &mut egui::Ui, scroll_id: &str) {
        if !self.queue.data.is_empty() {
            let palette = self.palette();
            let summary = self.queue.data.format_duration_summary();
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    egui::CollapsingHeader::new(
                        RichText::new(format!("Fila de reprodu\u{00e7}\u{00e3}o ({summary})"))
                            .size(14.0)
                            .strong(),
                    )
                    .id_salt(("watch_queue", scroll_id))
                    .default_open(false)
                    .show(ui, |ui| {
                        self.queue_section_ui(
                            ui,
                            &format!("{scroll_id}_queue"),
                            true,
                            None,
                            Some(180.0),
                        );
                    });
                });
            ui.add_space(8.0);
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
            && self.watch.sidebar_cache.playlist_urls_len == self.feed.home.playlist_urls.len();
        if up_to_date {
            return;
        }

        let mut previous_heights: HashMap<String, (i32, f32)> = self
            .watch
            .sidebar_cache
            .cards
            .drain(..)
            .filter_map(|card| card.measured_height.map(|h| (card.video.url, h)))
            .collect();

        let extra_cap = self.channels.page.as_ref().map_or(0, |p| p.videos.len());
        let mut seen_urls = HashSet::with_capacity(self.feed.videos.len() + extra_cap);
        let mut candidate_videos: Vec<&Video> =
            Vec::with_capacity(self.feed.videos.len() + extra_cap);
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

        let mut cards = Vec::new();
        if !candidate_videos.is_empty() {
            let current_video = self.selected.as_ref();
            let current_meta = current_video.and_then(|v| self.metadata_for(&v.url));
            let profile = UserProfile::build(&self.history.data, &self.queue.data, current_video);
            let scored = RecommendationEngine::rank_sidebar_video_indices(
                &candidate_videos,
                &self.video_metadata,
                &profile,
                current_video,
                current_meta,
                self.watch.extras.sidebar_tab,
            );
            let ordered_indices: Vec<usize> = if scored.is_empty() {
                (0..candidate_videos.len()).collect()
            } else {
                scored
                    .into_iter()
                    .map(|(orig_idx, _, _)| orig_idx)
                    .collect()
            };
            let now_epoch = recommendations::current_unix_epoch();
            cards.reserve(ordered_indices.len());
            for orig_idx in ordered_indices {
                let video = candidate_videos[orig_idx];
                let metadata = self.metadata_for(&video.url);
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
                let channel_label = (!video.channel.is_empty()).then(|| {
                    let verified = metadata
                        .and_then(|m| m.view_count)
                        .is_some_and(|v| v >= 50_000);
                    if verified {
                        format!("{} \u{2714}", truncate_text_chars(&video.channel, 34))
                    } else {
                        truncate_text_chars(&video.channel, 38)
                    }
                });
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
            let loading_feed = self.feed.search.rx.is_some()
                || self.feed.home.batch_rx.is_some()
                || self.feed.home.rx.is_some();
            if loading_feed {
                let dark_theme = self.settings.theme.value == Theme::Dark;
                skeleton::watch_sidebar_skeleton(ui, ui.available_width(), dark_theme);
            } else {
                ui.label(RichText::new("Nenhuma recomendação no momento.").weak());
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

    pub(crate) fn watch_sidebar_results_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
    ) -> Option<Video> {
        self.sync_theme_visuals(ctx);
        self.ensure_history_synchronized();
        let palette = self.palette();
        if !self.channels.view_open
            && self.watch.extras.sidebar_tab == watch_extras::SidebarTab::Transcript
        {
            self.subtitles_ui(ui, ctx, scroll_id);
            return None;
        }
        self.watch_sidebar_tabs_ui(ui, ctx, &palette);
        ui.add_space(10.0);
        self.watch_sidebar_queue_section_ui(ui, scroll_id);
        if self.channels.view_open {
            return self.channel_section_ui(ui, ctx, &format!("{scroll_id}_channel"), true, None);
        }
        self.watch_sidebar_feed_ui(ui, ctx, scroll_id)
    }

    pub(crate) fn paint_avatar_or_initials(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        avatar_url: Option<&str>,
        fallback_name: &str,
        diameter: f32,
        clickable_cursor: bool,
    ) -> egui::Response {
        Self::paint_avatar_with_cache(
            &mut self.avatars,
            ui,
            ctx,
            avatar_url,
            fallback_name,
            diameter,
            clickable_cursor,
        )
    }

    pub(crate) fn paint_avatar_with_cache(
        avatars: &mut thumbnails::AvatarCache,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        avatar_url: Option<&str>,
        fallback_name: &str,
        diameter: f32,
        clickable_cursor: bool,
    ) -> egui::Response {
        let sense = if clickable_cursor {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(diameter, diameter), sense);
        let painted_image = avatar_url
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
            .is_some_and(|uri| avatars.try_paint_circle(ui, ctx, rect, uri));
        if !painted_image {
            let label = if fallback_name.trim().is_empty() {
                "YT"
            } else {
                fallback_name
            };
            let [r, g, b] = recommendations::channel_avatar_rgb(label);
            let initials = recommendations::channel_avatar_initials(label);
            ui.painter()
                .circle_filled(rect.center(), diameter * 0.5, Color32::from_rgb(r, g, b));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initials,
                egui::FontId::proportional((diameter * 0.39).clamp(10.0, 16.0)),
                Color32::WHITE,
            );
        }
        if clickable_cursor {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    }

    pub(crate) fn insert_emoji_into_draft(
        ctx: &egui::Context,
        input_id: egui::Id,
        draft: &mut String,
        emoji: &str,
    ) {
        let char_pos: usize = egui::TextEdit::load_state(ctx, input_id)
            .and_then(|state| state.cursor.char_range())
            .map(|range| usize::from(range.primary.index))
            .unwrap_or_else(|| draft.chars().count());
        let byte_idx = draft
            .char_indices()
            .nth(char_pos)
            .map(|(idx, _)| idx)
            .unwrap_or(draft.len());
        draft.insert_str(byte_idx, emoji);
        let new_char_pos = char_pos + emoji.chars().count();
        if let Some(mut state) = egui::TextEdit::load_state(ctx, input_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(new_char_pos),
                )));
            state.store(ctx, input_id);
        }
        ctx.memory_mut(|mem| mem.request_focus(input_id));
    }

    pub(crate) fn comment_emoji_picker_ui(
        ui: &mut egui::Ui,
        palette: &ThemePalette,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
    ) -> Option<&'static str> {
        const EMOJIS: &[&str] = &[
            "\u{1f600}",
            "\u{1f602}",
            "\u{1f923}",
            "\u{1f60a}",
            "\u{1f60d}",
            "\u{1f970}",
            "\u{1f914}",
            "\u{1f62e}",
            "\u{1f622}",
            "\u{1f62d}",
            "\u{1f621}",
            "\u{1f525}",
            "\u{2764}\u{fe0f}",
            "\u{1f44d}",
            "\u{1f44e}",
            "\u{1f44f}",
            "\u{1f64c}",
            "\u{1f389}",
            "\u{1f4af}",
            "\u{2728}",
            "\u{1f440}",
            "\u{1f64f}",
            "\u{1f680}",
            "\u{26a1}",
        ];
        let mut chosen = None;
        ui.push_id(id_salt, |ui| {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                        for &emoji in EMOJIS {
                            let btn = egui::Button::new(RichText::new(emoji).size(16.0))
                                .fill(Color32::TRANSPARENT)
                                .frame(false)
                                .min_size(egui::vec2(28.0, 28.0));
                            if ui
                                .add(btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                chosen = Some(emoji);
                            }
                        }
                    });
                });
        });
        chosen
    }

    pub(crate) fn hash_comment_row_state(
        extras: &watch_extras::WatchExtras,
        comment_id: &str,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(state) = extras.comment_replies.get(comment_id) {
            state.reply_open.hash(&mut hasher);
            state.loaded.hash(&mut hasher);
            state.comments.len().hash(&mut hasher);
            state.is_loading().hash(&mut hasher);
            state.is_posting_reply().hash(&mut hasher);
            state.error.is_some().hash(&mut hasher);
            state.reply_post_error.is_some().hash(&mut hasher);
            state.next.is_some().hash(&mut hasher);
            if state.loaded {
                for reply in &state.comments {
                    reply.id.len().hash(&mut hasher);
                    if let Some(tr) = extras.comment_translations.get(&reply.id) {
                        tr.showing_translated.hash(&mut hasher);
                        tr.loading.hash(&mut hasher);
                        tr.error.is_some().hash(&mut hasher);
                        tr.translated_lines.len().hash(&mut hasher);
                    }
                    extras
                        .comment_rating_errors
                        .contains_key(&reply.id)
                        .hash(&mut hasher);
                    extras.is_rating_comment(&reply.id).hash(&mut hasher);
                }
            }
        } else {
            0u8.hash(&mut hasher);
        }
        (extras.reply_emoji_picker_for.as_deref() == Some(comment_id)).hash(&mut hasher);
        if let Some(tr) = extras.comment_translations.get(comment_id) {
            tr.showing_translated.hash(&mut hasher);
            tr.loading.hash(&mut hasher);
            tr.error.is_some().hash(&mut hasher);
            tr.translated_lines.len().hash(&mut hasher);
        }
        extras
            .comment_rating_errors
            .contains_key(comment_id)
            .hash(&mut hasher);
        extras.is_rating_comment(comment_id).hash(&mut hasher);
        hasher.finish()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn comment_item_body_and_actions_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        comment: &oauth::Comment,
        cached_lines: &[watch_extras::CachedTimestampLine],
        parent_comment_id: Option<&str>,
        text_size: f32,
        can_reply: bool,
    ) {
        let palette = self.palette();
        let link_color = if self.settings.theme.value == Theme::Dark {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let (showing_translated, translation_loading, translation_error, clicked_secs) = {
            let translation = self.watch.extras.comment_translations.get(&comment.id);
            let showing_translated = translation.is_some_and(|t| t.showing_translated);
            let translation_loading = translation.is_some_and(|t| t.loading);
            let translation_error = translation.and_then(|t| t.error.clone());
            let lines_to_render = translation
                .filter(|t| t.showing_translated && !t.translated_lines.is_empty())
                .map(|t| t.translated_lines.as_slice())
                .unwrap_or(cached_lines);
            let clicked_secs = Self::render_timestamp_lines_ui(
                ui,
                lines_to_render,
                palette.channel_fg,
                text_size,
                link_color,
            );
            (
                showing_translated,
                translation_loading,
                translation_error,
                clicked_secs,
            )
        };
        if let Some(secs) = clicked_secs {
            self.seek_to_seconds(secs);
        }

        let translate_label = if translation_loading {
            "Traduzindo..."
        } else if showing_translated {
            "Ver original"
        } else {
            "Traduzir para o portugu\u{00ea}s"
        };
        let translate_btn = egui::Button::new(RichText::new(translate_label).size(12.0).color(
            if showing_translated {
                palette.info_accent_fg
            } else {
                palette.neutral_status_fg
            },
        ))
        .fill(Color32::TRANSPARENT)
        .frame(false);
        if ui
            .add_enabled(!translation_loading, translate_btn)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.watch
                .extras
                .toggle_comment_translation(&comment.id, &comment.text, ctx);
        }
        if let Some(error) = translation_error {
            ui.label(RichText::new(error).size(11.5).color(palette.warning_fg));
        }

        let rating_busy_any = self.watch.extras.is_rating_any_comment();
        let rating_busy_here = self.watch.extras.is_rating_comment(&comment.id);
        let rating_error = self
            .watch
            .extras
            .comment_rating_errors
            .get(&comment.id)
            .cloned();

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let rating = self.watch.extras.comment_ratings.get(&comment.id).copied();
            let liked = rating == Some(watch_extras::CommentRating::Like);
            let disliked = rating == Some(watch_extras::CommentRating::Dislike);
            let displayed_likes = self
                .watch
                .extras
                .comment_displayed_likes(&comment.id, comment.likes);
            let like_color = if liked {
                palette.info_accent_fg
            } else {
                palette.neutral_status_fg
            };
            let like_icon = Icon::ThumbsUp.image(like_color, 15.0);
            let like_btn = if displayed_likes > 0 {
                let compact = recommendations::format_youtube_compact_views(displayed_likes)
                    .trim_end_matches(" visualiza\u{00e7}\u{00f5}es")
                    .trim_end_matches(" visualiza\u{00e7}\u{00e3}o")
                    .to_owned();
                egui::Button::image_and_text(
                    like_icon,
                    RichText::new(compact).size(12.5).color(like_color),
                )
            } else {
                egui::Button::image(like_icon)
            }
            .fill(Color32::TRANSPARENT)
            .frame(false);
            if ui
                .add_enabled(!rating_busy_any, like_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Gostei")
                .clicked()
            {
                self.watch.extras.start_comment_rating(
                    &comment.id,
                    parent_comment_id,
                    watch_extras::CommentRating::Like,
                    self.auth.youtube_web_session.as_ref(),
                    ctx,
                );
            }

            let dislike_color = if disliked {
                palette.info_accent_fg
            } else {
                palette.neutral_status_fg
            };
            let dislike_btn = egui::Button::image(Icon::ThumbsDown.image(dislike_color, 15.0))
                .fill(Color32::TRANSPARENT)
                .frame(false);
            if ui
                .add_enabled(!rating_busy_any, dislike_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("N\u{00e3}o gostei")
                .clicked()
            {
                self.watch.extras.start_comment_rating(
                    &comment.id,
                    parent_comment_id,
                    watch_extras::CommentRating::Dislike,
                    self.auth.youtube_web_session.as_ref(),
                    ctx,
                );
            }

            if rating_busy_here {
                let _ = skeleton::skeleton_box(
                    ui,
                    egui::vec2(26.0, 14.0),
                    7.0,
                    self.settings.theme.value == Theme::Dark,
                );
            }

            if can_reply {
                let reply_btn = egui::Button::new(
                    RichText::new("Responder")
                        .size(12.0)
                        .strong()
                        .color(palette.header_button_fg),
                )
                .fill(Color32::TRANSPARENT)
                .frame(false);
                if ui
                    .add(reply_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    let entry = self
                        .watch
                        .extras
                        .comment_replies
                        .entry(comment.id.clone())
                        .or_default();
                    entry.reply_open = !entry.reply_open;
                    entry.reply_post_error = None;
                    if !entry.reply_open
                        && self.watch.extras.reply_emoji_picker_for.as_deref()
                            == Some(comment.id.as_str())
                    {
                        self.watch.extras.reply_emoji_picker_for = None;
                    }
                }
            }
        });
        if let Some(error) = rating_error {
            ui.label(RichText::new(error).size(11.5).color(palette.warning_fg));
        }
    }

    pub(crate) fn comments_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, scroll_id: &str) {
        let palette = self.palette();
        if self.auth.authenticated
            && self.auth.my_channel_avatar_url.is_none()
            && self.auth.my_channel_rx.is_none()
            && !self.auth.my_channel_fetch_attempted
        {
            self.start_fetch_my_channel(false, ctx);
        }
        if !cfg!(test)
            && self.watch.extras.comments_loaded
            && !self.watch.extras.comments.is_empty()
            && !self.watch.extras.comment_state_sync_started
            && (self.auth.youtube_web_session.is_some()
                || (!self.auth.youtube_web_session_checked && session::has_saved_profile()))
        {
            self.watch
                .extras
                .start_comment_state_sync(self.auth.youtube_web_session.as_ref(), ctx);
        }

        let needs_reauth = self
            .auth
            .session
            .as_ref()
            .is_some_and(|s| !s.has_comment_scope());
        let can_write_comments = self
            .auth
            .session
            .as_ref()
            .is_some_and(oauth::TokenSession::has_comment_scope);

        let displayed_total = self
            .watch
            .extras
            .comments_total
            .unwrap_or(self.watch.extras.comments.len() as u64)
            .max(self.watch.extras.comments.len() as u64);
        let count_text = if displayed_total == 1 {
            "1 coment\u{00e1}rio".to_owned()
        } else {
            format!("{displayed_total} coment\u{00e1}rios")
        };

        let mut new_order = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(24.0, 8.0);
            ui.label(
                RichText::new(count_text)
                    .size(20.0)
                    .strong()
                    .color(palette.header_button_fg),
            );

            if styled_compact_button_enabled(
                ui,
                self.watch.extras.comments_loading.is_none(),
                "\u{2261}  Ordenar por",
                self.watch.extras.comments_order_menu_open,
                &palette,
            )
            .on_hover_text("Escolher ordena\u{00e7}\u{00e3}o dos coment\u{00e1}rios")
            .clicked()
            {
                self.watch.extras.comments_order_menu_open =
                    !self.watch.extras.comments_order_menu_open;
            }

            if !self.watch.extras.comments_loaded
                && self.watch.extras.comments_loading.is_none()
                && !needs_reauth
            {
                if styled_compact_button(ui, "Carregar coment\u{00e1}rios", false, &palette)
                    .clicked()
                {
                    self.watch
                        .extras
                        .start_comments(self.auth.session.as_ref(), false, ctx);
                }
            } else if self.watch.extras.comments_loaded
                && styled_compact_button_enabled(
                    ui,
                    self.watch.extras.comments_loading.is_none(),
                    "\u{21bb}",
                    false,
                    &palette,
                )
                .on_hover_text("Atualizar coment\u{00e1}rios")
                .clicked()
            {
                self.watch
                    .extras
                    .start_comments(self.auth.session.as_ref(), false, ctx);
            }
        });

        if self.watch.extras.comments_order_menu_open {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(palette.card_bg)
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(6, 6))
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .show(ui, |ui| {
                    ui.set_max_width(340.0);
                    for (title, subtitle, order) in [
                        (
                            "Principais",
                            "Mostrar coment\u{00e1}rios em destaque",
                            oauth::CommentOrder::Relevance,
                        ),
                        (
                            "Mais recentes",
                            "Mostrar coment\u{00e1}rios recentes, incluindo spam em potencial",
                            oauth::CommentOrder::Time,
                        ),
                    ] {
                        let selected = self.watch.extras.comments_order == order;
                        let item_fill = if selected {
                            palette.header_button_bg
                        } else {
                            Color32::TRANSPARENT
                        };
                        let resp = egui::Frame::new()
                            .fill(item_fill)
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(12, 9))
                            .show(ui, |ui| {
                                ui.set_min_width(308.0);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.label(
                                        RichText::new(title)
                                            .size(14.0)
                                            .strong()
                                            .color(palette.header_button_fg),
                                    );
                                    ui.label(
                                        RichText::new(subtitle)
                                            .size(12.0)
                                            .color(palette.neutral_status_fg),
                                    );
                                });
                            })
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            new_order = Some(order);
                            self.watch.extras.comments_order_menu_open = false;
                        }
                    }
                });
        }

        if let Some(order) = new_order {
            self.watch
                .extras
                .set_comments_order(order, self.auth.session.as_ref(), ctx);
        }

        ui.add_space(12.0);

        if needs_reauth {
            if self.auth.reauth_rx.is_some() {
                ui.horizontal(|ui| {
                    skeleton::skeleton_circle(ui, 16.0, self.settings.theme.value == Theme::Dark);
                    ui.label("Aguardando autoriza\u{00e7}\u{00e3}o no navegador...");
                    if styled_ghost_button(ui, "Cancelar", &palette).clicked() {
                        self.cancel_reauth();
                    }
                });
            } else if styled_pill_button(ui, "Autorizar coment\u{00e1}rios", false, &palette)
                .clicked()
            {
                self.start_reauth(ctx);
            }
        } else if can_write_comments {
            let mut post_comment = false;
            let user_avatar_url = self.auth.my_channel_avatar_url.clone();
            let user_name = self
                .auth
                .my_channel_title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| match &self.auth.my_channel {
                    Some(ChannelTarget::Handle(h)) => h.trim_start_matches('@').to_owned(),
                    Some(ChannelTarget::ChannelId(id)) => id.clone(),
                    None => "Voc\u{00ea}".to_owned(),
                });
            let composer_id = ui.make_persistent_id((scroll_id, "comment_composer_input"));
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let composer_avatar_resp = self.paint_avatar_or_initials(
                    ui,
                    ctx,
                    user_avatar_url.as_deref(),
                    &user_name,
                    40.0,
                    false,
                );
                Self::paint_comment_avatar_ring(ui, composer_avatar_resp.rect);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let has_draft = !self.watch.extras.comment_draft.trim().is_empty();
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut self.watch.extras.comment_draft)
                            .id(composer_id)
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .frame(egui::Frame::NONE)
                            .hint_text(
                                RichText::new("Adicione um coment\u{00e1}rio...")
                                    .size(14.0)
                                    .color(palette.neutral_status_fg),
                            ),
                    );
                    if resp.gained_focus() || resp.clicked() || has_draft {
                        self.watch.extras.comment_composer_active = true;
                    }
                    let line_y = resp.rect.bottom() + 4.0;
                    let line_stroke = if resp.has_focus() {
                        egui::Stroke::new(2.0, palette.header_button_fg)
                    } else {
                        egui::Stroke::new(1.0, palette.neutral_status_fg.gamma_multiply(0.5))
                    };
                    ui.painter().line_segment(
                        [
                            egui::pos2(resp.rect.left(), line_y),
                            egui::pos2(ui.max_rect().right(), line_y),
                        ],
                        line_stroke,
                    );
                    ui.add_space(10.0);

                    let sending = self.watch.extras.is_posting_comment();
                    let ready = !sending && has_draft;
                    let ctrl_enter = resp.has_focus()
                        && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
                    if ctrl_enter && ready {
                        post_comment = true;
                    }

                    if self.watch.extras.comment_composer_active
                        || has_draft
                        || self.watch.extras.comment_emoji_picker_open
                    {
                        ui.horizontal(|ui| {
                            let emoji_btn =
                                egui::Button::image(Icon::Smile.image(palette.icon_fg, 19.0))
                                    .fill(if self.watch.extras.comment_emoji_picker_open {
                                        palette.card_alt_bg
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .frame(false)
                                    .corner_radius(16)
                                    .min_size(egui::vec2(32.0, 32.0));
                            if ui
                                .add(emoji_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("Inserir emoji")
                                .clicked()
                            {
                                self.watch.extras.comment_emoji_picker_open =
                                    !self.watch.extras.comment_emoji_picker_open;
                                if self.watch.extras.comment_emoji_picker_open {
                                    self.watch.extras.reply_emoji_picker_for = None;
                                }
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let comentar_btn = egui::Button::new(
                                        RichText::new("Comentar").size(13.0).strong().color(
                                            if ready {
                                                palette.active_button_fg
                                            } else {
                                                palette.neutral_status_fg
                                            },
                                        ),
                                    )
                                    .fill(if ready {
                                        palette.active_button_bg
                                    } else {
                                        palette.header_button_bg
                                    })
                                    .corner_radius(18)
                                    .min_size(egui::vec2(92.0, 34.0));
                                    if ui
                                        .add_enabled(ready, comentar_btn)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        post_comment = true;
                                    }

                                    let cancel_btn = egui::Button::new(
                                        RichText::new("Cancelar")
                                            .size(13.0)
                                            .strong()
                                            .color(palette.header_button_fg),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false)
                                    .corner_radius(18)
                                    .min_size(egui::vec2(74.0, 34.0));
                                    if ui
                                        .add(cancel_btn)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.watch.extras.comment_draft.clear();
                                        self.watch.extras.comment_composer_active = false;
                                        self.watch.extras.comment_emoji_picker_open = false;
                                        self.watch.extras.comment_post_error = None;
                                        resp.surrender_focus();
                                    }
                                },
                            );
                        });

                        if self.watch.extras.comment_emoji_picker_open {
                            ui.add_space(8.0);
                            if let Some(emoji) = Self::comment_emoji_picker_ui(
                                ui,
                                &palette,
                                (scroll_id, "comment_emoji_picker"),
                            ) {
                                Self::insert_emoji_into_draft(
                                    ctx,
                                    composer_id,
                                    &mut self.watch.extras.comment_draft,
                                    emoji,
                                );
                                self.watch.extras.comment_composer_active = true;
                            }
                        }
                    }
                });
            });
            if post_comment {
                self.watch
                    .extras
                    .start_post_comment(self.auth.session.as_ref(), ctx);
            }
            if let Some(error) = &self.watch.extras.comment_post_error {
                ui.add_space(4.0);
                ui.colored_label(palette.warning_fg, error);
            }
        } else if !self.auth.authenticated {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let avatar_resp = self.paint_avatar_or_initials(ui, ctx, None, "FT", 40.0, false);
                Self::paint_comment_avatar_ring(ui, avatar_resp.rect);
                ui.vertical(|ui| {
                    let row = ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Adicione um coment\u{00e1}rio...")
                                .size(13.5)
                                .color(palette.neutral_status_fg),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if styled_pill_button(ui, "Entrar", false, &palette).clicked() {
                                self.start_reauth(ctx);
                            }
                        });
                    });
                    let line_y = row.response.rect.bottom() + 2.0;
                    ui.painter().line_segment(
                        [
                            egui::pos2(row.response.rect.left(), line_y),
                            egui::pos2(row.response.rect.right(), line_y),
                        ],
                        egui::Stroke::new(1.0, palette.neutral_status_fg.gamma_multiply(0.5)),
                    );
                });
            });
        }

        ui.add_space(10.0);
        Self::paint_hairline_divider(ui, &palette);
        ui.add_space(10.0);

        if let Some(error) = &self.watch.extras.comments_error {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.colored_label(palette.warning_fg, error);
                });
        }
        if self.watch.extras.comments_loaded
            && self.watch.extras.comments.is_empty()
            && self.watch.extras.comments_error.is_none()
        {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new("Nenhum coment\u{00e1}rio dispon\u{00ed}vel.")
                            .color(palette.neutral_status_fg),
                    );
                });
        }

        let render_comments_body = |this: &mut Self, ui: &mut egui::Ui| {
            let comments_video_url = this.watch.extras.video_url.clone();
            let initial_comments_len = this.watch.extras.comments.len();
            watch_extras::sync_cached_comment_meta(
                &this.watch.extras.comments,
                &mut this.watch.extras.comment_render_cache,
            );
            let comments = std::mem::take(&mut this.watch.extras.comments);
            let mut comment_metas = std::mem::take(&mut this.watch.extras.comment_render_cache);
            let width_key = ui.available_width().round() as i32;
            let alloc_width = ui.available_width().max(80.0);
            let visible_rect = ui.clip_rect().expand2(egui::vec2(0.0, 350.0));
            for (comment, meta) in comments.iter().zip(comment_metas.iter_mut()) {
                let row_hash = Self::hash_comment_row_state(&this.watch.extras, &comment.id);
                ui.add_space(10.0);
                let cursor_top = ui.cursor().top();
                let offscreen_height = meta.measured_height.and_then(|(w_key, s_hash, height)| {
                    (w_key == width_key
                        && s_hash == row_hash
                        && height > 0.0
                        && (cursor_top + height < visible_rect.top()
                            || cursor_top > visible_rect.bottom()))
                    .then_some(height)
                });
                if let Some(height) = offscreen_height {
                    ui.allocate_exact_size(egui::vec2(alloc_width, height), egui::Sense::hover());
                } else {
                    let row_resp = ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let avatar_resp = this.paint_avatar_or_initials(
                            ui,
                            ctx,
                            comment.author_avatar.as_deref(),
                            &meta.avatar_name,
                            36.0,
                            false,
                        );
                        Self::paint_comment_avatar_ring(ui, avatar_resp.rect);

                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.set_max_width(ui.available_width().max(80.0));
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(
                                    RichText::new(&meta.handle)
                                        .size(13.0)
                                        .strong()
                                        .color(palette.header_button_fg),
                                );
                                if !meta.relative_time.is_empty() {
                                    ui.label(
                                        RichText::new(&meta.relative_time)
                                            .size(12.0)
                                            .color(palette.neutral_status_fg),
                                    );
                                }
                            });

                            this.comment_item_body_and_actions_ui(
                                ui,
                                ctx,
                                comment,
                                &meta.lines,
                                None,
                                14.0,
                                can_write_comments,
                            );

                            if this
                                .watch
                                .extras
                                .comment_replies
                                .get(&comment.id)
                                .is_some_and(|state| state.reply_open)
                            {
                                let mut post_reply = false;
                                let mut cancel_reply = false;
                                let mut toggle_reply_emoji = false;
                                let mut picked_reply_emoji: Option<&'static str> = None;
                                let reply_input_id =
                                    ui.make_persistent_id(("reply_composer_input", &comment.id));
                                let reply_emoji_open =
                                    this.watch.extras.reply_emoji_picker_for.as_deref()
                                        == Some(comment.id.as_str());
                                let user_avatar_url = this.auth.my_channel_avatar_url.clone();
                                let user_name = this
                                    .auth
                                    .my_channel_title
                                    .as_deref()
                                    .filter(|s| !s.trim().is_empty())
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| "Voc\u{00ea}".to_owned());

                                ui.add_space(4.0);
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    let _ = this.paint_avatar_or_initials(
                                        ui,
                                        ctx,
                                        user_avatar_url.as_deref(),
                                        &user_name,
                                        26.0,
                                        false,
                                    );
                                    if let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                    {
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 0.0;
                                            let resp = ui.add(
                                                egui::TextEdit::multiline(&mut state.reply_draft)
                                                    .id(reply_input_id)
                                                    .desired_rows(1)
                                                    .desired_width(f32::INFINITY)
                                                    .frame(egui::Frame::NONE)
                                                    .hint_text(
                                                        RichText::new("Adicione uma resposta...")
                                                            .size(13.0)
                                                            .color(palette.neutral_status_fg),
                                                    ),
                                            );
                                            let line_y = resp.rect.bottom() + 4.0;
                                            let line_stroke = if resp.has_focus() {
                                                egui::Stroke::new(2.0, palette.header_button_fg)
                                            } else {
                                                egui::Stroke::new(
                                                    1.0,
                                                    palette.neutral_status_fg.gamma_multiply(0.5),
                                                )
                                            };
                                            ui.painter().line_segment(
                                                [
                                                    egui::pos2(resp.rect.left(), line_y),
                                                    egui::pos2(ui.max_rect().right(), line_y),
                                                ],
                                                line_stroke,
                                            );
                                            ui.add_space(8.0);

                                            let sending = state.is_posting_reply();
                                            let ready =
                                                !sending && !state.reply_draft.trim().is_empty();
                                            let ctrl_enter = resp.has_focus()
                                                && ui.input(|i| {
                                                    i.modifiers.ctrl
                                                        && i.key_pressed(egui::Key::Enter)
                                                });
                                            if ctrl_enter && ready {
                                                post_reply = true;
                                            }

                                            ui.horizontal(|ui| {
                                                let emoji_btn = egui::Button::image(
                                                    Icon::Smile.image(palette.icon_fg, 17.0),
                                                )
                                                .fill(if reply_emoji_open {
                                                    palette.card_alt_bg
                                                } else {
                                                    Color32::TRANSPARENT
                                                })
                                                .frame(false)
                                                .corner_radius(14)
                                                .min_size(egui::vec2(28.0, 28.0));
                                                if ui
                                                    .add(emoji_btn)
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .on_hover_text("Inserir emoji")
                                                    .clicked()
                                                {
                                                    toggle_reply_emoji = true;
                                                }

                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        ui.spacing_mut().item_spacing.x = 8.0;
                                                        let responder_btn = egui::Button::new(
                                                            RichText::new("Responder")
                                                                .size(12.5)
                                                                .strong()
                                                                .color(if ready {
                                                                    palette.active_button_fg
                                                                } else {
                                                                    palette.neutral_status_fg
                                                                }),
                                                        )
                                                        .fill(if ready {
                                                            palette.active_button_bg
                                                        } else {
                                                            palette.header_button_bg
                                                        })
                                                        .corner_radius(16)
                                                        .min_size(egui::vec2(88.0, 30.0));
                                                        if ui
                                                            .add_enabled(ready, responder_btn)
                                                            .on_hover_cursor(
                                                                egui::CursorIcon::PointingHand,
                                                            )
                                                            .clicked()
                                                        {
                                                            post_reply = true;
                                                        }

                                                        let cancel_btn = egui::Button::new(
                                                            RichText::new("Cancelar")
                                                                .size(12.5)
                                                                .strong()
                                                                .color(palette.header_button_fg),
                                                        )
                                                        .fill(Color32::TRANSPARENT)
                                                        .frame(false)
                                                        .corner_radius(16)
                                                        .min_size(egui::vec2(68.0, 30.0));
                                                        if ui
                                                            .add(cancel_btn)
                                                            .on_hover_cursor(
                                                                egui::CursorIcon::PointingHand,
                                                            )
                                                            .clicked()
                                                        {
                                                            cancel_reply = true;
                                                            resp.surrender_focus();
                                                        }
                                                    },
                                                );
                                            });

                                            if reply_emoji_open {
                                                ui.add_space(6.0);
                                                picked_reply_emoji = Self::comment_emoji_picker_ui(
                                                    ui,
                                                    &palette,
                                                    ("reply_emoji_picker", &comment.id),
                                                );
                                            }
                                            if state.is_posting_reply() {
                                                ui.add_space(4.0);
                                                skeleton::comments_skeleton(
                                                    ui,
                                                    ui.available_width(),
                                                    1,
                                                    this.settings.theme.value == Theme::Dark,
                                                );
                                            }
                                            if let Some(err) = &state.reply_post_error {
                                                ui.colored_label(palette.warning_fg, err);
                                            }
                                        });
                                    }
                                });

                                if toggle_reply_emoji {
                                    if reply_emoji_open {
                                        this.watch.extras.reply_emoji_picker_for = None;
                                    } else {
                                        this.watch.extras.reply_emoji_picker_for =
                                            Some(comment.id.clone());
                                        this.watch.extras.comment_emoji_picker_open = false;
                                    }
                                }
                                if let Some(emoji) = picked_reply_emoji
                                    && let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                {
                                    Self::insert_emoji_into_draft(
                                        ctx,
                                        reply_input_id,
                                        &mut state.reply_draft,
                                        emoji,
                                    );
                                }
                                if cancel_reply
                                    && let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                {
                                    state.reply_open = false;
                                    state.reply_post_error = None;
                                    if this.watch.extras.reply_emoji_picker_for.as_deref()
                                        == Some(comment.id.as_str())
                                    {
                                        this.watch.extras.reply_emoji_picker_for = None;
                                    }
                                } else if post_reply {
                                    if this.watch.extras.reply_emoji_picker_for.as_deref()
                                        == Some(comment.id.as_str())
                                    {
                                        this.watch.extras.reply_emoji_picker_for = None;
                                    }
                                    this.watch.extras.start_post_reply(
                                        &comment.id,
                                        this.auth.session.as_ref(),
                                        ctx,
                                    );
                                }
                            }

                            let has_local_replies = this
                                .watch
                                .extras
                                .comment_replies
                                .get(&comment.id)
                                .is_some_and(|state| !state.comments.is_empty() || state.loaded);
                            if comment.replies > 0 || has_local_replies {
                                let state = this.watch.extras.comment_replies.get(&comment.id);
                                if comment.replies > 0 && state.is_none_or(|state| !state.loaded) {
                                    let loading = state.is_some_and(|state| state.is_loading());
                                    let reply_word = if comment.replies == 1 {
                                        "1 resposta \u{2304}".to_owned()
                                    } else {
                                        format!("{} respostas \u{2304}", comment.replies)
                                    };
                                    let btn = egui::Button::new(
                                        RichText::new(reply_word)
                                            .size(13.0)
                                            .strong()
                                            .color(Color32::from_rgb(62, 166, 255)),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false);
                                    if ui
                                        .add_enabled(!loading, btn)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        this.watch.extras.start_replies(
                                            &comment.id,
                                            false,
                                            this.auth.session.as_ref(),
                                            ctx,
                                        );
                                    }
                                }
                                let (loaded, loading, error, has_replies, has_next) = this
                                    .watch
                                    .extras
                                    .comment_replies
                                    .get(&comment.id)
                                    .map(|state| {
                                        (
                                            state.loaded,
                                            state.is_loading(),
                                            state.error.clone(),
                                            !state.comments.is_empty(),
                                            state.next.is_some(),
                                        )
                                    })
                                    .unwrap_or_default();
                                if loading {
                                    ui.add_space(4.0);
                                    skeleton::comments_skeleton(
                                        ui,
                                        ui.available_width(),
                                        2,
                                        this.settings.theme.value == Theme::Dark,
                                    );
                                }
                                if let Some(error) = error {
                                    ui.colored_label(palette.warning_fg, error);
                                    if styled_compact_button(
                                        ui,
                                        "Tentar novamente",
                                        false,
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        let retry_next = this
                                            .watch
                                            .extras
                                            .comment_replies
                                            .get(&comment.id)
                                            .is_some_and(
                                                watch_extras::CommentReplies::retry_uses_next_page,
                                            );
                                        this.watch.extras.start_replies(
                                            &comment.id,
                                            retry_next,
                                            this.auth.session.as_ref(),
                                            ctx,
                                        );
                                    }
                                }
                                if loaded && has_replies {
                                    let (replies, reply_metas) = this
                                        .watch
                                        .extras
                                        .comment_replies
                                        .get_mut(&comment.id)
                                        .map(|state| {
                                            watch_extras::sync_cached_comment_meta(
                                                &state.comments,
                                                &mut state.render_cache,
                                            );
                                            (
                                                std::mem::take(&mut state.comments),
                                                std::mem::take(&mut state.render_cache),
                                            )
                                        })
                                        .unwrap_or_default();
                                    for (reply, r_meta) in replies.iter().zip(reply_metas.iter()) {
                                        ui.add_space(6.0);
                                        ui.horizontal_top(|ui| {
                                            ui.spacing_mut().item_spacing.x = 10.0;
                                            let reply_avatar_resp = this.paint_avatar_or_initials(
                                                ui,
                                                ctx,
                                                reply.author_avatar.as_deref(),
                                                &r_meta.avatar_name,
                                                26.0,
                                                false,
                                            );
                                            Self::paint_comment_avatar_ring(
                                                ui,
                                                reply_avatar_resp.rect,
                                            );
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 3.0;
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(
                                                        RichText::new(&r_meta.handle)
                                                            .size(12.5)
                                                            .strong()
                                                            .color(palette.header_button_fg),
                                                    );
                                                    if !r_meta.relative_time.is_empty() {
                                                        ui.label(
                                                            RichText::new(&r_meta.relative_time)
                                                                .size(11.5)
                                                                .color(palette.neutral_status_fg),
                                                        );
                                                    }
                                                });
                                                this.comment_item_body_and_actions_ui(
                                                    ui,
                                                    ctx,
                                                    reply,
                                                    &r_meta.lines,
                                                    Some(&comment.id),
                                                    13.0,
                                                    false,
                                                );
                                            });
                                        });
                                    }
                                    if let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                        && state.comments.is_empty()
                                    {
                                        state.comments = replies;
                                        state.render_cache = reply_metas;
                                    }
                                }
                                if has_next
                                    && ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Mostrar mais respostas \u{2304}")
                                                    .size(12.5)
                                                    .strong()
                                                    .color(Color32::from_rgb(62, 166, 255)),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                {
                                    this.watch.extras.start_replies(
                                        &comment.id,
                                        true,
                                        this.auth.session.as_ref(),
                                        ctx,
                                    );
                                }
                            }
                        });
                    });
                    meta.measured_height =
                        Some((width_key, row_hash, row_resp.response.rect.height()));
                }
                ui.add_space(6.0);
            }
            if this.watch.extras.video_url == comments_video_url
                && this.watch.extras.comments.is_empty()
                && comments.len() == initial_comments_len
            {
                this.watch.extras.comments = comments;
                this.watch.extras.comment_render_cache = comment_metas;
            }

            if this.watch.extras.comments_loading.is_some() {
                ui.add_space(8.0);
                let dark_theme = this.settings.theme.value == Theme::Dark;
                skeleton::comments_skeleton(ui, ui.available_width(), 4, dark_theme);
            } else if this.watch.extras.is_posting_comment() {
                ui.add_space(6.0);
                skeleton::comments_skeleton(
                    ui,
                    ui.available_width(),
                    1,
                    this.settings.theme.value == Theme::Dark,
                );
            }

            if this.watch.extras.comments_next.is_some() {
                ui.add_space(8.0);
                let (sentinel_rect, _) =
                    ui.allocate_exact_size(egui::vec2(32.0, 8.0), egui::Sense::hover());
                let auto_trigger = ui.is_rect_visible(sentinel_rect)
                    && this.watch.extras.comments_loading.is_none()
                    && this.watch.extras.comments_error.is_none();
                let manual_click = ui
                    .add_enabled_ui(this.watch.extras.comments_loading.is_none(), |ui| {
                        styled_pill_button(
                            ui,
                            "Carregar mais 50 coment\u{00e1}rios",
                            false,
                            &palette,
                        )
                    })
                    .inner
                    .clicked();
                if auto_trigger || manual_click {
                    this.watch
                        .extras
                        .start_comments(this.auth.session.as_ref(), true, ctx);
                }
            }
        };

        if scroll_id == "shorts_comments" {
            egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_comments"))
                .show(ui, |ui| {
                    render_comments_body(self, ui);
                });
        } else {
            render_comments_body(self, ui);
        }
    }

    pub(crate) fn apply_ready_subtitle(&mut self) {
        #[cfg(windows)]
        if let Some(sub) = self.watch.extras.take_ready_subtitle() {
            self.playback.last_subtitle_id = Some(format!("yt:{}", sub.url));
            self.watch.extras.active_subtitle_url = Some(sub.url.clone());
            self.watch.extras.subtitles_error = None;
            if let Some(player) = &self.player {
                if !self.watch.extras.active_subtitle_cues.is_empty() && player.is_render_api() {
                    let _ = player.disable_subtitles();
                } else {
                    let path = sub.path.to_string_lossy().into_owned();
                    if let Err(error) = player.add_subtitle(&path, &sub.label, &sub.language) {
                        self.watch.extras.subtitles_error = Some(error);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        let _ = self.watch.extras.take_ready_subtitle();
    }

    pub(crate) fn subtitles_tracks_list_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
    ) {
        #[cfg(windows)]
        let has_active_player_subtitle = self
            .player
            .as_ref()
            .and_then(|player| player.property("sid"))
            .is_some_and(|sid| !sid.is_empty() && sid != "no")
            || self.watch.extras.active_subtitle_url.is_some();
        #[cfg(not(windows))]
        let has_active_player_subtitle = self.watch.extras.active_subtitle_url.is_some();

        let palette = self.palette();
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let mut chosen = None;
        let mut transcript_track = None;
        egui::Frame::new()
            .fill(palette.info_box_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.heading("Legendas");
                    if styled_compact_button_enabled(
                        ui,
                        self.watch.extras.subtitles_loading.is_none(),
                        "↻ Buscar novamente",
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        self.watch.extras.refresh_subtitles(ctx);
                    }
                    if has_active_player_subtitle
                        && styled_ghost_button(ui, "Desativar", &palette).clicked()
                    {
                        self.watch.extras.cancel_active_subtitle();
                        #[cfg(windows)]
                        if let Some(player) = &self.player
                            && let Err(error) = player.disable_subtitles()
                        {
                            self.watch.extras.subtitles_error = Some(error);
                        }
                    }
                });
                ui.label(
                    RichText::new(
                        "Faixas do YouTube obtidas pelo yt-dlp; disponibilidade pode variar.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                Self::paint_hairline_divider(ui, &palette);
                ui.add_space(6.0);
                if self.watch.extras.subtitles_loading.is_some()
                    || self.watch.extras.is_subtitle_applying()
                {
                    ui.add_space(4.0);
                    skeleton::transcript_skeleton(ui, ui.available_width(), dark_theme);
                    ui.add_space(4.0);
                }
                if let Some(error) = &self.watch.extras.subtitles_error {
                    egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.colored_label(palette.warning_fg, error);
                        });
                }
                if self.watch.extras.subtitles_loaded
                    && self.watch.extras.subtitles.is_empty()
                    && self.watch.extras.subtitles_error.is_none()
                {
                    egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new("Nenhuma legenda compatível encontrada.")
                                    .color(palette.neutral_status_fg),
                            );
                        });
                }
                let active_url = self.watch.extras.active_subtitle_url.as_deref();
                egui::ScrollArea::vertical()
                    .id_salt(format!("{scroll_id}_subtitles"))
                    .max_height(150.0)
                    .show(ui, |ui| {
                        for track in &self.watch.extras.subtitles {
                            let is_active = has_active_player_subtitle
                                && active_url == Some(track.url.as_str());
                            let label = Self::subtitle_track_label(track);
                            let button_text = if is_active {
                                format!("{label} ✓")
                            } else {
                                label.clone()
                            };
                            ui.horizontal(|ui| {
                                if styled_compact_button(ui, &button_text, is_active, &palette)
                                    .clicked()
                                {
                                    chosen = Some((track.clone(), label.clone()));
                                }
                                if styled_ghost_button(ui, "Transcrição", &palette)
                                    .on_hover_text(
                                        "Baixar, pesquisar e acompanhar o WebVTT desta faixa",
                                    )
                                    .clicked()
                                {
                                    transcript_track = Some(track.clone());
                                }
                            });
                            ui.add_space(3.0);
                        }
                    });
            });
        if let Some((track, label)) = chosen {
            #[cfg(windows)]
            if let Some(player) = &self.player {
                let _ = player.disable_subtitles();
            }
            self.playback.last_subtitle_id = Some(format!("yt:{}", track.url));
            self.watch.extras.start_apply_subtitle(&track, label, ctx);
            self.apply_ready_subtitle();
        }
        if let Some(track) = transcript_track {
            self.watch.extras.start_transcript(&track, ctx);
        }
    }

    pub(crate) fn transcript_active_cue_card_ui(
        &self,
        ui: &mut egui::Ui,
        active_index: usize,
        font_size: f32,
        high_contrast: bool,
    ) -> Option<u64> {
        let cue = &self.watch.extras.transcript[active_index];
        let (fill, stroke, text_color) = if high_contrast {
            if self.settings.theme.value == Theme::Dark {
                (
                    Color32::from_rgb(0, 0, 0),
                    Color32::from_rgb(255, 220, 80),
                    Color32::WHITE,
                )
            } else {
                (
                    Color32::from_rgb(255, 246, 184),
                    Color32::from_rgb(74, 57, 0),
                    Color32::from_rgb(20, 16, 0),
                )
            }
        } else {
            let palette = self.palette();
            (palette.notice_bg, palette.notice_stroke, palette.notice_fg)
        };
        let mut seek_to_ms = None;
        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Trecho atual")
                        .small()
                        .strong()
                        .color(text_color),
                );
                let text = format!(
                    "{}  {}",
                    format_time(cue.start_ms as f64 / 1000.0),
                    cue.text
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(text)
                                .size(font_size)
                                .color(text_color)
                                .strong(),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .wrap(),
                    )
                    .on_hover_text("Voltar para o início deste trecho")
                    .clicked()
                {
                    seek_to_ms = Some(cue.start_ms);
                }
            });
        seek_to_ms
    }

    pub(crate) fn transcript_matches_ui(
        &self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        query: &str,
        font_size: f32,
    ) -> Option<u64> {
        let matches = transcript::search(&self.watch.extras.transcript, query);
        if matches.is_empty() {
            let palette = self.palette();
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new("Nenhum trecho corresponde à busca.")
                            .color(palette.neutral_status_fg),
                    );
                });
            return None;
        }
        ui.label(
            RichText::new(format!(
                "{} ocorrência(s); exibindo até 200.",
                matches.len()
            ))
            .small()
            .weak(),
        );
        let mut seek_to_ms = None;
        egui::ScrollArea::vertical()
            .id_salt(format!("{scroll_id}_transcript_matches"))
            .max_height(360.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for cue in matches.into_iter().take(200) {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(format!(
                                    "{}  {}",
                                    format_time(cue.start_ms as f64 / 1000.0),
                                    cue.text
                                ))
                                .size(font_size),
                            )
                            .frame(false)
                            .wrap(),
                        )
                        .clicked()
                    {
                        seek_to_ms = Some(cue.start_ms);
                    }
                }
            });
        seek_to_ms
    }

    pub(crate) fn transcript_cues_list_ui(
        &self,
        ui: &mut egui::Ui,
        scroll_id: &str,
        active_index: Option<usize>,
        font_size: f32,
        high_contrast: bool,
    ) -> Option<u64> {
        let palette = self.palette();
        let row_height = (font_size * 2.4).max(36.0);
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(format!("{scroll_id}_transcript_cues"))
            .max_height(360.0)
            .auto_shrink([false, true]);
        if self.watch.extras.transcript_follow
            && let Some(index) = active_index
        {
            scroll = scroll
                .vertical_scroll_offset((index as f32 * row_height - row_height * 3.0).max(0.0));
        }
        let mut seek_to_ms = None;
        scroll.show_rows(
            ui,
            row_height,
            self.watch.extras.transcript.len(),
            |ui, visible| {
                for index in visible {
                    let cue = &self.watch.extras.transcript[index];
                    let is_active = active_index == Some(index);
                    let row_rect = ui
                        .allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::hover(),
                        )
                        .0;
                    let fill = if is_active {
                        if high_contrast && self.settings.theme.value == Theme::Dark {
                            Color32::from_rgb(55, 45, 0)
                        } else {
                            palette.queue_next_bg
                        }
                    } else if index % 2 == 0 {
                        Color32::TRANSPARENT
                    } else {
                        palette.message_box_bg
                    };
                    if fill != Color32::TRANSPARENT {
                        ui.painter().rect_filled(row_rect, 4.0, fill);
                    }
                    let text = format!(
                        "{}  {}",
                        format_time(cue.start_ms as f64 / 1000.0),
                        truncate_text_chars(&cue.text, 180)
                    );
                    let text = RichText::new(text).size(font_size);
                    let text = if is_active { text.strong() } else { text };
                    let button = egui::Button::new(text)
                        .frame(false)
                        .fill(Color32::TRANSPARENT);
                    if ui
                        .put(row_rect.shrink2(egui::vec2(8.0, 3.0)), button)
                        .on_hover_text(&cue.text)
                        .clicked()
                    {
                        seek_to_ms = Some(cue.start_ms);
                    }
                }
            },
        );
        seek_to_ms
    }

    pub(crate) fn subtitles_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, scroll_id: &str) {
        if let Some(video) = self.selected.clone() {
            self.ensure_watch_metadata_cache(&video);
        }
        let chapters = self.watch.metadata_cache.description_chapters.clone();
        if !chapters.is_empty() {
            let palette = self.palette();
            self.watch_description_chapters_ui(
                ui,
                ("subtitles_desc_chapters", scroll_id),
                &chapters,
                &palette,
            );
            ui.add_space(8.0);
        }
        self.subtitles_tracks_list_ui(ui, ctx, scroll_id);
        ui.add_space(8.0);
        let palette = self.palette();
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let has_transcript = !self.watch.extras.transcript.is_empty();
        let mut seek_to_ms = None;
        let mut close_transcript = false;
        egui::Frame::new()
            .fill(palette.info_box_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let pt = self.ui_pt();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if pt {
                            "Transcri\u{00e7}\u{00e3}o"
                        } else {
                            "Transcript"
                        })
                        .size(18.0)
                        .strong()
                        .color(palette.channel_fg),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if styled_icon_chip_button(ui, true, "\u{2715}", &palette)
                            .on_hover_text(if pt {
                                "Fechar transcri\u{00e7}\u{00e3}o"
                            } else {
                                "Close transcript"
                            })
                            .clicked()
                        {
                            close_transcript = true;
                        }
                    });
                });
                ui.label(
                    RichText::new(
                        "Use a busca para navegar ou acompanhe o trecho ativo sem tirar os olhos do vídeo.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if styled_compact_button_enabled(
                        ui,
                        has_transcript,
                        "Copiar transcrição",
                        false,
                        &palette,
                    )
                    .on_hover_text(
                        "Copiar todos os trechos com timestamps para a área de transferência",
                    )
                    .clicked()
                    {
                        let formatted = transcript::format_txt(&self.watch.extras.transcript);
                        let _ = copy_text_to_clipboard(ctx, &formatted);
                        self.watch.extras.transcript_export_status = Some((
                            true,
                            format!(
                                "{} trecho(s) copiados para a área de transferência.",
                                self.watch.extras.transcript.len()
                            ),
                        ));
                    }
                    if styled_compact_button_enabled(
                        ui,
                        has_transcript && !self.watch.extras.is_exporting_transcript(),
                        "Exportar (.txt / .md)",
                        false,
                        &palette,
                    )
                    .on_hover_text("Salvar transcrição em arquivo .txt ou Markdown (.md)")
                    .clicked()
                    {
                        let title = self
                            .selected
                            .as_ref()
                            .map(|v| v.title.as_str())
                            .unwrap_or("Transcricao")
                            .to_owned();
                        let cues = self.watch.extras.transcript.clone();
                        self.watch
                            .extras
                            .start_transcript_export(&title, &cues, ctx);
                    }
                    if self.watch.extras.is_exporting_transcript() {
                        let _ = skeleton::skeleton_box(
                            ui,
                            egui::vec2(48.0, 14.0),
                            7.0,
                            self.settings.theme.value == Theme::Dark,
                        );
                    }
                });
                if let Some((ok, status)) = &self.watch.extras.transcript_export_status {
                    let color = if *ok {
                        palette.notice_fg
                    } else {
                        palette.warning_fg
                    };
                    ui.colored_label(color, status);
                }
                ui.add_space(6.0);
                Self::paint_hairline_divider(ui, &palette);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Buscar:");
                    ui.text_edit_singleline(&mut self.watch.extras.transcript_query);
                    if !self.watch.extras.transcript_query.is_empty()
                        && styled_ghost_button(ui, "Limpar", &palette).clicked()
                    {
                        self.watch.extras.transcript_query.clear();
                    }
                });
                let (font_size, high_contrast) = self.transcript_settings_ui(ui);
                if self.watch.extras.transcript_loading {
                    ui.add_space(6.0);
                    skeleton::transcript_skeleton(ui, ui.available_width(), dark_theme);
                    ui.add_space(6.0);
                }
                if let Some(error) = &self.watch.extras.transcript_error {
                    egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.colored_label(palette.warning_fg, error);
                        });
                }
                if !self.watch.extras.transcript_loading
                    && self.watch.extras.transcript.is_empty()
                    && self.watch.extras.transcript_error.is_none()
                {
                    egui::Frame::new()
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new(
                                    "Escolha Transcrição em uma faixa acima para carregar o texto.",
                                )
                                .color(palette.neutral_status_fg),
                            );
                        });
                }
                let position_ms = self.current_player_live_pos_ms();
                let active_index = position_ms.and_then(|position| {
                    transcript::active_cue_index(&self.watch.extras.transcript, position)
                });
                seek_to_ms = active_index.and_then(|index| {
                    self.transcript_active_cue_card_ui(ui, index, font_size, high_contrast)
                });
                let query = self.watch.extras.transcript_query.trim().to_owned();
                if !query.is_empty() {
                    seek_to_ms = seek_to_ms
                        .or_else(|| self.transcript_matches_ui(ui, scroll_id, &query, font_size));
                } else if !self.watch.extras.transcript.is_empty() {
                    seek_to_ms = seek_to_ms.or_else(|| {
                        self.transcript_cues_list_ui(
                            ui,
                            scroll_id,
                            active_index,
                            font_size,
                            high_contrast,
                        )
                    });
                }
            });
        if close_transcript {
            self.watch.extras.sidebar_tab = watch_extras::SidebarTab::All;
        }
        if let Some(start_ms) = seek_to_ms {
            self.seek_to_seconds(start_ms as f64 / 1000.0);
        }
    }

    pub(crate) fn transcript_settings_ui(&mut self, ui: &mut egui::Ui) -> (f32, bool) {
        let mut follow = self.watch.extras.transcript_follow;
        let mut font_size = self.watch.extras.transcript_font_size.clamp(13.0, 24.0);
        if self.watch.extras.transcript_font_size <= 0.0 {
            font_size = 16.0;
        }
        let mut high_contrast = self.watch.extras.transcript_high_contrast;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut follow, "Acompanhar reprodução");
            ui.checkbox(&mut high_contrast, "Alto contraste");
            ui.add(
                egui::Slider::new(&mut font_size, 13.0..=24.0)
                    .integer()
                    .text("Tamanho"),
            );
        });
        self.watch.extras.transcript_follow = follow;
        self.watch.extras.transcript_font_size = font_size;
        self.watch.extras.transcript_high_contrast = high_contrast;
        (font_size, high_contrast)
    }

    pub(crate) fn shorts_watch_channel_ui(
        &self,
        ui: &mut egui::Ui,
        channel_label: &str,
        channel_target: Option<&ChannelTarget>,
        subscribed: Option<bool>,
        sub_pending: bool,
        palette: &ThemePalette,
    ) -> (Option<ChannelTarget>, bool) {
        let mut open_channel_target = None;
        let mut toggle_subscribe = false;
        ui.horizontal_wrapped(|ui| {
            if paint_channel_avatar(
                ui,
                if channel_label.is_empty() {
                    "Canal"
                } else {
                    channel_label
                },
                38.0,
            )
            .clicked()
            {
                open_channel_target = channel_target.cloned();
            }
            ui.vertical(|ui| {
                let label = if channel_label.is_empty() {
                    channel_target
                        .map(channel_target_display_label)
                        .unwrap_or("Canal")
                } else {
                    channel_label
                };
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(label)
                                .size(14.0)
                                .strong()
                                .color(palette.header_button_fg),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    open_channel_target = channel_target.cloned();
                }
                ui.label(
                    RichText::new("Ver canal")
                        .size(12.0)
                        .color(palette.neutral_status_fg),
                );
            });
            if channel_target.is_some() {
                let is_sub = subscribed == Some(true);
                let sub_label = if is_sub {
                    "✓ Inscrito"
                } else {
                    "Inscrever-se"
                };
                let sub_btn =
                    egui::Button::new(RichText::new(sub_label).size(12.5).strong().color(
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
                if ui
                    .add_enabled(!sub_pending, sub_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    toggle_subscribe = true;
                }
            }
        });
        (open_channel_target, toggle_subscribe)
    }

    pub(crate) fn shorts_watch_actions_ui(
        &self,
        ui: &mut egui::Ui,
        details: &ShortsWatchDetails<'_>,
        palette: &ThemePalette,
        actions: &mut ShortsWatchActions,
    ) {
        ui.horizontal_wrapped(|ui| {
            let like_active = details.current_rating == Some(oauth::VideoRating::Like);
            let like_btn = egui::Button::new(RichText::new("👍 Gostei").size(12.5).strong().color(
                if like_active {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                },
            ))
            .fill(if like_active {
                palette.active_button_bg
            } else {
                palette.header_button_bg
            })
            .corner_radius(18);
            if ui
                .add_enabled_ui(
                    !details.rating_pending && details.video_id.is_some(),
                    |ui| ui.add_sized([88.0, 36.0], like_btn),
                )
                .inner
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                actions.rating_to_apply = Some(if like_active {
                    None
                } else {
                    Some(oauth::VideoRating::Like)
                });
            }

            let dislike_active = details.current_rating == Some(oauth::VideoRating::Dislike);
            let dislike_btn =
                egui::Button::new(RichText::new("👎").size(14.5).color(if dislike_active {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                }))
                .fill(if dislike_active {
                    palette.active_button_bg
                } else {
                    palette.header_button_bg
                })
                .corner_radius(18);
            if ui
                .add_enabled_ui(
                    !details.rating_pending && details.video_id.is_some(),
                    |ui| ui.add_sized([42.0, 36.0], dislike_btn),
                )
                .inner
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                actions.rating_to_apply = Some(if dislike_active {
                    None
                } else {
                    Some(oauth::VideoRating::Dislike)
                });
            }

            let comments_btn =
                egui::Button::new(RichText::new("💬 Comentários").size(12.5).strong().color(
                    if details.comments_open {
                        palette.active_button_fg
                    } else {
                        palette.header_button_fg
                    },
                ))
                .fill(if details.comments_open {
                    palette.active_button_bg
                } else {
                    palette.header_button_bg
                })
                .corner_radius(18);
            if ui
                .add_sized([122.0, 36.0], comments_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                actions.toggle_comments = true;
            }

            let queue = egui::Button::new(
                RichText::new("Fila")
                    .size(13.0)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(18);
            if ui.add_sized([64.0, 36.0], queue).clicked() {
                actions.enqueue_video = true;
            }
            let share_label = if details.share_recently_copied {
                "✓ Copiado"
            } else {
                "Compartilhar"
            };
            let share = egui::Button::new(RichText::new(share_label).size(13.0).strong().color(
                if details.share_recently_copied {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                },
            ))
            .fill(if details.share_recently_copied {
                palette.active_button_bg
            } else {
                palette.header_button_bg
            })
            .corner_radius(18);
            if ui.add_sized([114.0, 36.0], share).clicked() {
                actions.share_video = true;
            }
            let save_label = if details.saved_in_library {
                "✓ Salvo"
            } else {
                "Salvar"
            };
            let save = egui::Button::new(RichText::new(save_label).size(13.0).strong().color(
                if details.save_menu_open {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                },
            ))
            .fill(if details.save_menu_open {
                palette.active_button_bg
            } else {
                palette.header_button_bg
            })
            .corner_radius(18);
            if ui.add_sized([82.0, 36.0], save).clicked() {
                actions.toggle_save_menu = true;
            }
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let prev_btn = egui::Button::new(
                RichText::new("↑ Anterior")
                    .size(12.5)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(16);
            if ui
                .add_enabled(details.has_prev_short, prev_btn)
                .on_hover_text("Short anterior (↑ ou K)")
                .clicked()
            {
                actions.prev_short = true;
            }
            if let Some((index, total)) = details.short_index_and_total {
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(14)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{index} / {total}"))
                                .size(12.5)
                                .strong()
                                .color(palette.header_button_fg),
                        );
                    });
            }
            let next_btn = egui::Button::new(
                RichText::new("↓ Próximo")
                    .size(12.5)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(16);
            if ui
                .add_enabled(details.has_next_short, next_btn)
                .on_hover_text("Próximo Short (↓ ou J)")
                .clicked()
            {
                actions.next_short = true;
            }
            let loop_btn = egui::Button::new(RichText::new("🔁 Loop").size(12.5).strong().color(
                if details.loop_video {
                    palette.active_button_fg
                } else {
                    palette.header_button_fg
                },
            ))
            .fill(if details.loop_video {
                palette.active_button_bg
            } else {
                palette.header_button_bg
            })
            .corner_radius(16);
            if ui
                .add(loop_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if details.loop_video {
                    "Desativar repetição automática do Short"
                } else {
                    "Repetir este Short automaticamente"
                })
                .clicked()
            {
                actions.toggle_loop = true;
            }
        });
    }

    pub(crate) fn shorts_watch_details_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        details: &ShortsWatchDetails<'_>,
        palette: &ThemePalette,
        actions: &mut ShortsWatchActions,
    ) {
        let video = details.video;
        let channel_label = video.channel.trim();

        ui.label(
            RichText::new("Shorts")
                .size(12.5)
                .strong()
                .color(Color32::from_rgb(255, 0, 51)),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(&video.title)
                .size(19.0)
                .strong()
                .color(palette.header_button_fg),
        );
        if let Some(line) = &details.meta_line {
            ui.add_space(4.0);
            ui.label(
                RichText::new(line)
                    .size(13.0)
                    .color(palette.neutral_status_fg),
            );
        }
        ui.add_space(12.0);
        let (target, sub_clicked) = self.shorts_watch_channel_ui(
            ui,
            channel_label,
            details.channel_target,
            details.subscribed,
            details.sub_pending,
            palette,
        );
        if let Some(target) = target {
            actions.open_channel_target = Some(target);
        }
        if sub_clicked {
            actions.toggle_subscribe = true;
        }
        ui.add_space(14.0);
        self.shorts_watch_actions_ui(ui, details, palette, actions);
        if details.save_menu_open {
            ui.add_space(10.0);
            self.save_menu_ui(ui, video, ctx);
        }
        if let Some(text) = &details.description {
            ui.add_space(14.0);
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    self.render_text_with_timestamps_ui(ui, text, palette.description_fg, 13.0);
                });
        }
        if details.comments_open {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    self.comments_ui(ui, ctx, "shorts_comments");
                });
        }
        if is_actionable_inline_message(&self.message) {
            ui.add_space(8.0);
            if inline_error_banner_ui(ui, &self.message, palette) {
                self.message.clear();
            }
        }
    }

    pub(crate) fn shorts_watch_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        let video = self.selected.clone()?;
        let palette = self.palette();
        let (meta_line, description, channel_target, channel_id_hint) = {
            let metadata = self.metadata_for(&video.url);
            (
                format_youtube_card_subline(&video.duration, metadata),
                description_snippet(metadata.and_then(|meta| meta.description.as_deref()), 220),
                metadata.and_then(search::channel_target_for_metadata),
                metadata.and_then(|meta| meta.channel_id.clone()),
            )
        };
        let available_width = ui.available_width();
        let available_height = ui.available_height();
        let player_size = compute_player_size_with_theater(
            available_width.min(430.0),
            available_height,
            false,
            false,
            WatchLayoutMode::Short,
        );
        let wide = available_width >= 760.0;
        let saved_in_library = self.video_saved_in_library(&video);
        let save_menu_open = self.playback.save_menu_open;
        let comments_open = self.watch.extras.tab == watch_extras::Tab::Comments;
        let subscribed = self
            .subscription_status_for(channel_target.as_ref(), channel_id_hint.as_deref())
            .map(|s| s.subscription_id.is_some());
        let sub_pending = self.auth.sub_toggle_rx.is_some();
        let video_id = history::youtube_video_id(&video.url);
        let current_rating = video_id
            .as_ref()
            .and_then(|id| self.auth.ratings.get(id).copied());
        let rating_pending = self.auth.rating_rx.is_some();
        let has_prev_short = self.next_or_prev_short(false).is_some();
        let has_next_short = self.next_or_prev_short(true).is_some();
        let short_index_and_total = self.short_index_and_total();
        let details = ShortsWatchDetails {
            video: &video,
            meta_line,
            description,
            channel_target: channel_target.as_ref(),
            video_id: video_id.as_deref(),
            current_rating,
            rating_pending,
            subscribed,
            sub_pending,
            saved_in_library,
            save_menu_open,
            comments_open,
            has_prev_short,
            has_next_short,
            short_index_and_total,
            loop_video: self.playback.loop_video,
            share_recently_copied: self
                .watch
                .share_copied_at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(2)),
        };
        let mut actions = ShortsWatchActions::default();

        egui::ScrollArea::vertical()
            .id_salt("shorts_watch_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                if wide {
                    let details_width = 350.0;
                    let content_width = player_size.x + 24.0 + details_width;
                    let left = ((available_width - content_width) * 0.5).max(0.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(left);
                        ui.vertical(|ui| {
                            ui.set_width(player_size.x);
                            ui.set_max_width(player_size.x);
                            self.watch_primary_ui(
                                ui,
                                ctx,
                                player_size.x,
                                available_height,
                                WatchLayoutMode::Short,
                            );
                        });
                        ui.add_space(24.0);
                        ui.vertical(|ui| {
                            ui.set_width(details_width);
                            ui.set_max_width(details_width);
                            self.shorts_watch_details_ui(ui, ctx, &details, &palette, &mut actions);
                        });
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        self.watch_primary_ui(
                            ui,
                            ctx,
                            player_size.x,
                            available_height,
                            WatchLayoutMode::Short,
                        );
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        self.shorts_watch_details_ui(ui, ctx, &details, &palette, &mut actions);
                    });
                }
            });

        if !self.in_app_ui_pass && !ctx.egui_wants_keyboard_input() {
            let (key_prev, key_next) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowUp) || input.key_pressed(egui::Key::K),
                    input.key_pressed(egui::Key::ArrowDown) || input.key_pressed(egui::Key::J),
                )
            });
            if key_prev {
                actions.prev_short = true;
            } else if key_next {
                actions.next_short = true;
            }
        }

        if let (Some(id), Some(rating)) = (video_id, actions.rating_to_apply) {
            self.start_video_rating(id, rating, ctx);
        }
        if actions.toggle_subscribe
            && let Some(target) = channel_target
        {
            self.start_toggle_subscription(target, channel_id_hint, ctx);
        }
        if actions.toggle_comments {
            if self.watch.extras.tab == watch_extras::Tab::Comments {
                self.watch.extras.tab = watch_extras::Tab::Results;
            } else {
                self.watch.extras.tab = watch_extras::Tab::Comments;
                if !self.watch.extras.comments_loaded {
                    self.watch
                        .extras
                        .start_comments(self.auth.session.as_ref(), false, ctx);
                }
            }
        }
        if actions.enqueue_video {
            let _ = self.enqueue_video(video.clone());
        }
        if actions.share_video {
            self.open_share_modal(&video);
        }
        if actions.toggle_save_menu {
            self.playback.save_menu_open = !self.playback.save_menu_open;
            if self.playback.save_menu_open
                && self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
            {
                self.start_fetch_remote_playlists(ctx);
            }
        }
        if actions.toggle_loop {
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
                self.message = error.clone();
                self.toasts.error(error);
            }
            self.toasts.info(if self.playback.loop_video {
                "Repetição de Short ativada."
            } else {
                "Repetição de Short desativada."
            });
        }
        if let Some(target) = actions.open_channel_target {
            self.open_channel(target, ctx);
        }
        if actions.prev_short {
            return self.next_or_prev_short(false);
        }
        if actions.next_short {
            return self.next_or_prev_short(true);
        }
        None
    }

    pub(crate) fn is_watch_short_mode(&self) -> bool {
        if matches!(self.feed.filter, recommendations::FeedFilter::Shorts)
            || self.ui.browse_section == BrowseSection::Shorts
        {
            self.selected.as_ref().is_some_and(|video| {
                recommendations::is_short_video(video, self.metadata_for(&video.url))
            })
        } else {
            false
        }
    }

    pub(crate) fn watch_page_side_by_side_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        let side_margin = watch_page_side_margin(ui.available_width(), columns);
        egui::ScrollArea::vertical()
            .id_salt("watch_primary_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(side_margin);
                    ui.vertical(|ui| {
                        ui.set_width(columns.primary_width);
                        ui.set_max_width(columns.primary_width);
                        self.watch_primary_ui(
                            ui,
                            ctx,
                            columns.primary_width,
                            available_height,
                            WatchLayoutMode::SideBySide,
                        );
                        if is_actionable_inline_message(&self.message) {
                            ui.add_space(6.0);
                            let palette = self.palette();
                            if inline_error_banner_ui(ui, &self.message, &palette) {
                                self.message.clear();
                            }
                        }
                    });
                    ui.add_space(columns.gap);
                    ui.vertical(|ui| {
                        ui.set_width(columns.secondary_width);
                        ui.set_max_width(columns.secondary_width);
                        if self.watch_live_chat_panel_ui(
                            ui,
                            ctx,
                            columns.secondary_width,
                            (available_height - 24.0).max(320.0),
                        ) {
                            ui.add_space(10.0);
                        }
                        chosen =
                            self.watch_sidebar_results_ui(ui, ctx, "watch_sidebar_results_scroll");
                    });
                });
            });
        chosen
    }

    pub(crate) fn watch_page_short_column_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("watch_short_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.watch_primary_ui(
                    ui,
                    ctx,
                    columns.primary_width,
                    available_height,
                    WatchLayoutMode::Short,
                );
                if is_actionable_inline_message(&self.message) {
                    ui.add_space(6.0);
                    let palette = self.palette();
                    if inline_error_banner_ui(ui, &self.message, &palette) {
                        self.message.clear();
                    }
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                chosen = self.watch_sidebar_results_ui(ui, ctx, "watch_short_results_scroll");
            });
        chosen
    }

    pub(crate) fn watch_page_stacked_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        columns: &WatchColumns,
        available_height: f32,
    ) -> Option<Video> {
        let mut chosen = None;
        if self.playback.theater {
            egui::ScrollArea::vertical()
                .id_salt("watch_theater_main_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let theater_width = ui.available_width().max(columns.primary_width);
                    egui::Frame::NONE.fill(Color32::BLACK).show(ui, |ui| {
                        ui.set_width(theater_width);
                        ui.vertical_centered(|ui| {
                            self.watch_primary_with_metadata_ui(
                                ui,
                                ctx,
                                theater_width,
                                available_height,
                                WatchLayoutMode::Stacked,
                                false,
                            );
                        });
                    });
                    ui.add_space(12.0);
                    let margin = if theater_width >= 680.0 {
                        watch_page_margin()
                    } else {
                        12.0
                    };
                    let inner_width = (theater_width - margin * 2.0).max(260.0);
                    let below_columns = compute_watch_columns(inner_width);
                    let side_margin = watch_page_side_margin(theater_width, &below_columns);
                    let selected_video = self.selected.clone();
                    #[cfg(windows)]
                    let resolution_badge = selected_video
                        .as_ref()
                        .and_then(|video| {
                            (self.playback.last_video_size_url.as_deref()
                                == Some(video.url.as_str()))
                            .then_some(self.playback.last_video_size)
                            .flatten()
                        })
                        .map(|size| format!("Reproduzindo em {size}"));
                    #[cfg(not(windows))]
                    let resolution_badge: Option<String> = None;
                    if below_columns.mode == WatchLayoutMode::SideBySide {
                        ui.horizontal_top(|ui| {
                            ui.add_space(side_margin);
                            ui.vertical(|ui| {
                                ui.set_width(below_columns.primary_width);
                                ui.set_max_width(below_columns.primary_width);
                                if let Some(video) = &selected_video {
                                    self.watch_metadata_card_ui(
                                        ui,
                                        video,
                                        below_columns.primary_width,
                                        resolution_badge.as_deref(),
                                    );
                                }
                                if is_actionable_inline_message(&self.message) {
                                    ui.add_space(6.0);
                                    let palette = self.palette();
                                    if inline_error_banner_ui(ui, &self.message, &palette) {
                                        self.message.clear();
                                    }
                                }
                            });
                            ui.add_space(below_columns.gap);
                            ui.vertical(|ui| {
                                ui.set_width(below_columns.secondary_width);
                                ui.set_max_width(below_columns.secondary_width);
                                if self.watch_live_chat_panel_ui(
                                    ui,
                                    ctx,
                                    below_columns.secondary_width,
                                    (available_height - 24.0).max(320.0),
                                ) {
                                    ui.add_space(10.0);
                                }
                                chosen = self.watch_sidebar_results_ui(
                                    ui,
                                    ctx,
                                    "watch_theater_results_scroll",
                                );
                            });
                        });
                    } else {
                        ui.horizontal_top(|ui| {
                            ui.add_space(side_margin);
                            ui.vertical(|ui| {
                                ui.set_width(below_columns.primary_width);
                                ui.set_max_width(below_columns.primary_width);
                                if let Some(video) = &selected_video {
                                    self.watch_metadata_card_ui(
                                        ui,
                                        video,
                                        below_columns.primary_width,
                                        resolution_badge.as_deref(),
                                    );
                                }
                                if is_actionable_inline_message(&self.message) {
                                    ui.add_space(6.0);
                                    let palette = self.palette();
                                    if inline_error_banner_ui(ui, &self.message, &palette) {
                                        self.message.clear();
                                    }
                                }
                                if self.watch_live_chat_panel_ui(
                                    ui,
                                    ctx,
                                    below_columns.primary_width,
                                    420.0,
                                ) {
                                    ui.add_space(8.0);
                                }
                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(6.0);
                                chosen = self.watch_sidebar_results_ui(
                                    ui,
                                    ctx,
                                    "watch_theater_results_scroll",
                                );
                            });
                        });
                    }
                });
            return chosen;
        }
        let side_margin = watch_page_side_margin(ui.available_width(), columns);
        egui::ScrollArea::vertical()
            .id_salt("watch_stacked_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(side_margin);
                    ui.vertical(|ui| {
                        ui.set_width(columns.primary_width);
                        ui.set_max_width(columns.primary_width);
                        self.watch_primary_ui(
                            ui,
                            ctx,
                            columns.primary_width,
                            available_height,
                            WatchLayoutMode::Stacked,
                        );
                        if is_actionable_inline_message(&self.message) {
                            ui.add_space(6.0);
                            let palette = self.palette();
                            if inline_error_banner_ui(ui, &self.message, &palette) {
                                self.message.clear();
                            }
                        }
                        if self.watch_live_chat_panel_ui(ui, ctx, columns.primary_width, 420.0) {
                            ui.add_space(8.0);
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(6.0);
                        chosen =
                            self.watch_sidebar_results_ui(ui, ctx, "watch_stacked_results_scroll");
                    });
                });
            });
        chosen
    }

    pub(crate) fn watch_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if self.is_watch_short_mode() && !self.playback.theater && !self.playback.fullscreen {
            return self.shorts_watch_page_ui(ui, ctx);
        }
        let columns = compute_watch_page_columns(
            ui.available_width(),
            ui.available_height(),
            self.playback.theater,
        );
        let available_height = ui.available_height();
        match columns.mode {
            WatchLayoutMode::SideBySide => {
                self.watch_page_side_by_side_ui(ui, ctx, &columns, available_height)
            }
            WatchLayoutMode::Short => {
                self.watch_page_short_column_ui(ui, ctx, &columns, available_height)
            }
            WatchLayoutMode::Stacked => {
                self.watch_page_stacked_ui(ui, ctx, &columns, available_height)
            }
        }
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

    pub(crate) fn in_app_mini_player_ui(&mut self, ctx: &egui::Context) {
        const MINI_PLAYER_FOOTER_HEIGHT: f32 = 82.0;
        let Some(video) = self.selected.clone() else {
            return;
        };
        self.sync_player_window_visibility();
        let palette = self.palette();
        let pt = self.ui_pt();
        let content = ctx.input(|i| i.content_rect());
        let card_w = (content.width() * 0.27)
            .clamp(300.0, 384.0)
            .min((content.width() - 24.0).max(240.0));
        let video_h = (card_w * 9.0 / 16.0).round();

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
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
            .default_size(egui::vec2(
                card_w + 2.0,
                video_h + MINI_PLAYER_FOOTER_HEIGHT + 2.0,
            ))
            .movable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12.0)
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 20,
                        spread: 2,
                        color: Color32::from_black_alpha(110),
                    })
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_width(card_w);
                        ui.set_max_width(card_w);
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                        let mut video_rect_opt: Option<egui::Rect> = None;
                        let mut surface_clicked = false;
                        let mut can_overlay_hover = true;

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
                                            && telemetry.raw_path.as_deref().is_some_and(|path| {
                                                path == video.url
                                                    || (history::youtube_video_id(path).is_some()
                                                        && history::youtube_video_id(path)
                                                            == history::youtube_video_id(
                                                                &video.url,
                                                            ))
                                            })
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

                        if let Some(vrect) = video_rect_opt
                            && can_overlay_hover
                        {
                            let mut consumed_click = false;
                            let mut seek_hot = false;
                            let mut overlay_ui =
                                ui.new_child(egui::UiBuilder::new().max_rect(vrect));
                            overlay_ui.set_clip_rect(vrect.intersect(ui.clip_rect()));

                            if !is_live && let Some(dur) = duration.filter(|&d| d > 0.0) {
                                let seek_hit_rect = egui::Rect::from_min_max(
                                    egui::pos2(
                                        vrect.left(),
                                        (vrect.bottom() - 8.0).max(vrect.top()),
                                    ),
                                    vrect.max,
                                );
                                let mut seek_ui = overlay_ui
                                    .new_child(egui::UiBuilder::new().max_rect(seek_hit_rect));
                                seek_ui.set_clip_rect(seek_hit_rect.intersect(ui.clip_rect()));
                                let (_, seek_resp) = seek_ui.allocate_exact_size(
                                    seek_hit_rect.size(),
                                    egui::Sense::click_and_drag(),
                                );
                                let seek_resp =
                                    seek_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                                seek_hot = seek_resp.hovered() || seek_resp.dragged();
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

                            let hovered_video = ui.rect_contains_pointer(vrect);
                            if hovered_video && !seek_hot {
                                let painter = overlay_ui.painter();
                                painter.rect_filled(vrect, 0.0, Color32::from_black_alpha(64));
                                let center = vrect.center();
                                painter.circle_filled(center, 22.0, Color32::from_black_alpha(165));
                                painter.circle_stroke(
                                    center,
                                    22.0,
                                    egui::Stroke::new(
                                        1.0,
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 45),
                                    ),
                                );
                                if paused {
                                    let tri = vec![
                                        egui::pos2(center.x - 5.5, center.y - 8.0),
                                        egui::pos2(center.x - 5.5, center.y + 8.0),
                                        egui::pos2(center.x + 8.0, center.y),
                                    ];
                                    painter.add(egui::Shape::convex_polygon(
                                        tri,
                                        Color32::WHITE,
                                        egui::Stroke::NONE,
                                    ));
                                } else {
                                    let left_bar = egui::Rect::from_min_max(
                                        egui::pos2(center.x - 6.0, center.y - 7.5),
                                        egui::pos2(center.x - 1.8, center.y + 7.5),
                                    );
                                    let right_bar = egui::Rect::from_min_max(
                                        egui::pos2(center.x + 1.8, center.y - 7.5),
                                        egui::pos2(center.x + 6.0, center.y + 7.5),
                                    );
                                    painter.rect_filled(left_bar, 1.0, Color32::WHITE);
                                    painter.rect_filled(right_bar, 1.0, Color32::WHITE);
                                }
                            }

                            let bar_h = if seek_hot { 5.0 } else { 3.0 };
                            let bar_rect = egui::Rect::from_min_max(
                                egui::pos2(vrect.left(), vrect.bottom() - bar_h),
                                vrect.max,
                            );
                            overlay_ui.painter().rect_filled(
                                bar_rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(255, 255, 255, 65),
                            );
                            let frac = if is_live {
                                1.0
                            } else if let Some(dur) = duration.filter(|&d| d > 0.0) {
                                (position / dur).clamp(0.0, 1.0) as f32
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
                                    overlay_ui.painter().circle_filled(
                                        egui::pos2(fill_rect.right(), bar_rect.center().y),
                                        5.0,
                                        Color32::from_rgb(255, 0, 51),
                                    );
                                }
                            }
                            if surface_clicked && !consumed_click {
                                toggle_pause_clicked = true;
                            }
                        }

                        let can_prev = can_prev_video || (!is_live && position > 1.0);
                        let inner_w = (card_w - 20.0).max(200.0);
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.set_width(inner_w);
                                ui.set_max_width(inner_w);
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.allocate_ui_with_layout(
                                    egui::vec2(inner_w, 34.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_max_width(inner_w);
                                        ui.set_min_height(34.0);
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&video.title)
                                                    .size(13.0)
                                                    .strong()
                                                    .color(palette.channel_fg),
                                            )
                                            .truncate(),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&video.channel)
                                                    .size(11.5)
                                                    .color(palette.neutral_status_fg),
                                            )
                                            .truncate(),
                                        );
                                    },
                                );
                                ui.add_space(6.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(inner_w, 26.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_width(inner_w);
                                        ui.set_min_height(26.0);
                                        ui.spacing_mut().item_spacing.x = 4.0;
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
                                        if mini_player_icon_chip_button(
                                            ui,
                                            can_prev,
                                            MiniPlayerIcon::Previous,
                                            &palette,
                                        )
                                        .on_hover_text(prev_tip)
                                        .clicked()
                                        {
                                            prev_clicked = true;
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
                                        if mini_player_icon_chip_button(
                                            ui,
                                            can_next,
                                            MiniPlayerIcon::Next,
                                            &palette,
                                        )
                                        .on_hover_text(next_tip)
                                        .clicked()
                                        {
                                            next_clicked = true;
                                        }

                                        ui.add_space(4.0);

                                        let mute_tip = if muted || volume_percent <= 0.5 {
                                            if pt { "Ativar som (M)" } else { "Unmute (M)" }
                                        } else if pt {
                                            "Silenciar (M)"
                                        } else {
                                            "Mute (M)"
                                        };
                                        let mute_resp = mini_player_icon_chip_button(
                                            ui,
                                            true,
                                            MiniPlayerIcon::Volume {
                                                muted,
                                                volume_percent,
                                            },
                                            &palette,
                                        )
                                        .on_hover_text(mute_tip);
                                        if mute_resp.clicked() {
                                            toggle_mute_clicked = true;
                                        }

                                        let slider_w = (inner_w - 196.0).clamp(36.0, 96.0);
                                        let (new_vol, vol_resp) = mini_player_volume_slider(
                                            ui,
                                            slider_w,
                                            volume_percent,
                                            muted,
                                            &palette,
                                        );
                                        if let Some(vol) = new_vol {
                                            set_volume_target = Some(vol);
                                        } else if mute_resp.hovered() || vol_resp.hovered() {
                                            let wheel_y = ctx.input(|i| i.smooth_scroll_delta.y);
                                            if wheel_y.abs() >= 1.0 {
                                                let base = if muted { 0.0 } else { volume_percent };
                                                let step = if wheel_y > 0.0 { 5.0 } else { -5.0 };
                                                set_volume_target =
                                                    Some((base + step).clamp(0.0, 100.0));
                                            }
                                        }

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.spacing_mut().item_spacing.x = 4.0;
                                                if styled_icon_chip_button(
                                                    ui, true, "\u{2715}", &palette,
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
                                                if styled_icon_chip_button(
                                                    ui, true, "\u{29c9}", &palette,
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
                                                if styled_icon_chip_button(
                                                    ui, true, "\u{2922}", &palette,
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
                                            },
                                        );
                                    },
                                );
                            });
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

#[derive(Clone, Copy)]
enum MiniPlayerIcon {
    Previous,
    Next,
    Volume { muted: bool, volume_percent: f32 },
}

fn mini_player_icon_chip_button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: MiniPlayerIcon,
    palette: &ThemePalette,
) -> egui::Response {
    let size = egui::vec2(26.0, 24.0);
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
        let bg = if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
            palette.card_alt_bg
        } else {
            palette.header_button_bg
        };
        let stroke_color = if enabled && response.hovered() {
            palette.header_button_fg.gamma_multiply(0.45)
        } else {
            palette.card_alt_stroke
        };
        painter.rect_filled(rect, 8.0, bg);
        painter.rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );
        let fg = if enabled {
            palette.header_button_fg
        } else {
            palette.neutral_status_fg.gamma_multiply(0.5)
        };
        let c = rect.center();
        match icon {
            MiniPlayerIcon::Previous => {
                let bar = egui::Rect::from_min_max(
                    egui::pos2(c.x - 5.5, c.y - 4.5),
                    egui::pos2(c.x - 3.5, c.y + 4.5),
                );
                painter.rect_filled(bar, 0.5, fg);
                let tri = vec![
                    egui::pos2(c.x + 5.0, c.y - 4.5),
                    egui::pos2(c.x + 5.0, c.y + 4.5),
                    egui::pos2(c.x - 2.0, c.y),
                ];
                painter.add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
            }
            MiniPlayerIcon::Next => {
                let tri = vec![
                    egui::pos2(c.x - 5.0, c.y - 4.5),
                    egui::pos2(c.x - 5.0, c.y + 4.5),
                    egui::pos2(c.x + 2.0, c.y),
                ];
                painter.add(egui::Shape::convex_polygon(tri, fg, egui::Stroke::NONE));
                let bar = egui::Rect::from_min_max(
                    egui::pos2(c.x + 3.5, c.y - 4.5),
                    egui::pos2(c.x + 5.5, c.y + 4.5),
                );
                painter.rect_filled(bar, 0.5, fg);
            }
            MiniPlayerIcon::Volume {
                muted,
                volume_percent,
            } => {
                let speaker = vec![
                    egui::pos2(c.x - 6.0, c.y - 2.0),
                    egui::pos2(c.x - 3.2, c.y - 2.0),
                    egui::pos2(c.x + 0.5, c.y - 5.0),
                    egui::pos2(c.x + 0.5, c.y + 5.0),
                    egui::pos2(c.x - 3.2, c.y + 2.0),
                    egui::pos2(c.x - 6.0, c.y + 2.0),
                ];
                painter.add(egui::Shape::convex_polygon(speaker, fg, egui::Stroke::NONE));
                if muted || volume_percent <= 0.5 {
                    painter.line_segment(
                        [
                            egui::pos2(c.x + 2.5, c.y - 2.8),
                            egui::pos2(c.x + 6.5, c.y + 2.8),
                        ],
                        egui::Stroke::new(1.5, fg),
                    );
                    painter.line_segment(
                        [
                            egui::pos2(c.x + 6.5, c.y - 2.8),
                            egui::pos2(c.x + 2.5, c.y + 2.8),
                        ],
                        egui::Stroke::new(1.5, fg),
                    );
                } else {
                    let arc1 = vec![
                        egui::pos2(c.x + 2.4, c.y - 2.4),
                        egui::pos2(c.x + 3.6, c.y),
                        egui::pos2(c.x + 2.4, c.y + 2.4),
                    ];
                    painter.add(egui::Shape::line(arc1, egui::Stroke::new(1.4, fg)));
                    if volume_percent > 45.0 {
                        let arc2 = vec![
                            egui::pos2(c.x + 4.5, c.y - 4.2),
                            egui::pos2(c.x + 6.5, c.y),
                            egui::pos2(c.x + 4.5, c.y + 4.2),
                        ];
                        painter.add(egui::Shape::line(arc2, egui::Stroke::new(1.4, fg)));
                    }
                }
            }
        }
    }
    response
}

fn mini_player_volume_slider(
    ui: &mut egui::Ui,
    width: f32,
    volume_percent: f32,
    muted: bool,
    palette: &ThemePalette,
) -> (Option<f32>, egui::Response) {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let effective = if muted {
        0.0
    } else {
        volume_percent.clamp(0.0, 100.0)
    };
    let ratio = (effective / 100.0).clamp(0.0, 1.0);
    let mut new_volume = None;
    if (response.dragged() || response.clicked())
        && rect.width() > 10.0
        && let Some(pos) = response.interact_pointer_pos()
    {
        let track_left = rect.left() + 5.0;
        let track_right = (rect.right() - 5.0).max(track_left + 1.0);
        let frac = ((pos.x - track_left) / (track_right - track_left)).clamp(0.0, 1.0);
        new_volume = Some((frac * 100.0).round());
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let track_left = rect.left() + 5.0;
        let track_right = (rect.right() - 5.0).max(track_left + 1.0);
        let cy = rect.center().y;
        let track_rect = egui::Rect::from_min_max(
            egui::pos2(track_left, cy - 1.5),
            egui::pos2(track_right, cy + 1.5),
        );
        painter.rect_filled(
            track_rect,
            1.5,
            palette.neutral_status_fg.gamma_multiply(0.35),
        );
        let shown_ratio = new_volume.map(|v| v / 100.0).unwrap_or(ratio);
        let fill_x = track_left + (track_right - track_left) * shown_ratio;
        if fill_x > track_left {
            let fill_rect = egui::Rect::from_min_max(
                egui::pos2(track_left, cy - 1.5),
                egui::pos2(fill_x, cy + 1.5),
            );
            painter.rect_filled(fill_rect, 1.5, palette.header_button_fg);
        }
        let thumb_radius = if response.hovered() || response.dragged() {
            5.5
        } else {
            4.5
        };
        painter.circle_filled(
            egui::pos2(fill_x, cy),
            thumb_radius,
            palette.header_button_fg,
        );
    }
    let resp = response.on_hover_text(format!("Volume: {:.0}%", effective));
    (new_volume, resp)
}
