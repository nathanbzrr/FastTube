use super::*;
use eframe::egui::RichText;
use std::sync::Arc;

type PendingAmbientColorSlot = Arc<std::sync::Mutex<Option<(String, Color32)>>>;

#[derive(Clone)]
struct AmbientColorCacheEntry {
    video_url: String,
    color: Color32,
    pending: Option<PendingAmbientColorSlot>,
}

fn paint_ambient_mode_glow(
    painter: &egui::Painter,
    rect: egui::Rect,
    accent: Color32,
    dark_theme: bool,
) {
    if rect.width() < 80.0 || rect.height() < 60.0 {
        return;
    }
    let scale = if dark_theme { 1.0_f32 } else { 0.45_f32 };
    let rings: [(f32, f32, u8); 5] = [
        (46.0, 38.0, 12),
        (34.0, 28.0, 20),
        (24.0, 20.0, 30),
        (14.0, 12.0, 40),
        (6.0, 6.0, 48),
    ];
    for (expand_x, expand_y, alpha) in rings {
        let a = ((alpha as f32) * scale).round().clamp(1.0, 255.0) as u8;
        let ring_rect = rect.expand2(egui::vec2(expand_x, expand_y));
        painter.rect_filled(
            ring_rect,
            (18.0 + expand_y * 0.5).min(48.0),
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), a),
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
pub(super) struct PlayerOsdSnapshot {
    pub(super) video_url: String,
    pub(super) paused: bool,
    pub(super) muted: bool,
    pub(super) volume_rounded: i32,
    pub(super) speed_hundredths: i32,
}

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
    pub(super) fn update_player_osd_snapshot_state(
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
        let current_pass = ctx.cumulative_pass_nr();
        let already_polled = self.playback.frame_events_polled_pass == Some(current_pass);
        let frame_snapshot = if already_polled {
            self.playback.frame_snapshot.clone()
        } else {
            None
        };
        let dark_theme = self.is_dark_theme();
        let player = self.player.as_mut()?;
        let (events, poll_error, snap) = if let Some(snap) = frame_snapshot {
            (Vec::new(), None, snap)
        } else {
            let (events, poll_error) = player.poll_events();
            (events, poll_error, player.snapshot())
        };

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
        if self.settings.prefs.value.ambient_mode
            && !fullscreen
            && !mini_player
            && !self.playback.in_app_mini_player
            && !settings_open
            && !audio_only
            && let Some(video) = selected
        {
            let ambient_color = Self::resolve_ambient_color_for_video(ctx, video);
            let glow_painter = ui.painter().with_clip_rect(ui.clip_rect().expand(52.0));
            paint_ambient_mode_glow(&glow_painter, rect, ambient_color, dark_theme);
        }
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
        let raw_path = if snap.observed {
            snap.path.clone()
        } else if refresh_raw_path {
            player.property_cstr(c"path")
        } else {
            cached_raw_path
        };
        let width_num = if snap.observed {
            snap.video_width.map(|w| w as f64)
        } else {
            player.double_cstr(c"width")
        };
        let height_num = if snap.observed {
            snap.video_height.map(|h| h as f64)
        } else {
            player.double_cstr(c"height")
        };
        let video_size = VideoSize::from_numbers(width_num, height_num);
        let raw_position = if snap.observed {
            snap.time_pos.filter(|&pos| pos >= 0.0)
        } else {
            player.double_cstr(c"time-pos").filter(|&pos| pos >= 0.0)
        };
        let raw_duration = if snap.observed {
            snap.duration.filter(|&dur| dur > 0.0)
        } else {
            player.double_cstr(c"duration").filter(|&dur| dur > 0.0)
        };
        let buffered_secs = if snap.observed {
            snap.demuxer_cache_duration
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(|cache_dur| {
                    let pos = raw_position.unwrap_or(0.0);
                    let end = pos + cache_dur;
                    raw_duration.map_or(end, |dur| end.min(dur))
                })
        } else {
            player.buffered_duration_secs()
        };
        let telemetry = PlayerTelemetry {
            events,
            poll_error,
            paused: if snap.observed {
                snap.pause.unwrap_or(false)
            } else {
                player.flag_cstr(c"pause").unwrap_or(false)
            },
            muted: if snap.observed {
                snap.mute.unwrap_or(false)
            } else {
                player.flag_cstr(c"mute").unwrap_or(false)
            },
            ended: if snap.observed {
                snap.eof_reached.unwrap_or(false)
            } else {
                player.flag_cstr(c"eof-reached").unwrap_or(false)
            },
            seekable: if snap.observed {
                snap.seekable
            } else {
                player.flag_cstr(c"seekable")
            },
            raw_path,
            raw_position,
            raw_duration,
            buffered_secs,
            raw_volume: if snap.observed {
                snap.volume.unwrap_or(100.0).clamp(0.0, 100.0)
            } else {
                player
                    .double_cstr(c"volume")
                    .unwrap_or(100.0)
                    .clamp(0.0, 100.0)
            },
            speed: if snap.observed {
                snap.speed.unwrap_or(1.0)
            } else {
                player.double_cstr(c"speed").unwrap_or(1.0)
            },
            current_height: height_num.map(|height| height as u32),
            video_size,
        };
        if refresh_raw_path || snap.observed {
            self.playback.cached_raw_path = telemetry.raw_path.clone();
            self.playback.cached_raw_path_poll = Some(Instant::now());
        }
        Some((video_area, telemetry))
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
        if let Some(ref manifest) = self.playback.current_manifest
            && !manifest.is_expired()
            && !manifest.is_live
            && history::youtube_video_id(&video.url).as_deref() == Some(manifest.video_id.as_str())
            && let Some(pair) = crate::stream_manifest::select_stream_pair(
                manifest,
                self.settings.playback.value.max_height,
                quality,
                self.settings.prefs.value.video_codec_preference,
                self.playback.preferred_audio_lang.as_deref(),
                self.playback.audio_only,
            )
            && player
                .switch_direct_stream(
                    &pair.to_direct_stream_options(None, self.playback.audio_only),
                )
                .is_ok()
        {
            self.playback.quality = quality;
            self.playback.active_direct_stream_url = Some(pair.video_url);
            self.playback.cached_raw_path_poll = None;
            self.message.clear();
            return;
        }
        let path_matches_video = raw_path.is_some_and(|path| {
            path == video.url
                || (self
                    .playback
                    .active
                    .as_ref()
                    .is_some_and(|active| active.url == video.url)
                    && (path.starts_with("https://") || path.starts_with("http://")))
        });
        let ready = self
            .playback
            .active
            .as_ref()
            .is_some_and(|active| active.has_live_sample)
            && path_matches_video
            && raw_position.is_some();
        let preserve_position = self
            .playback
            .active
            .as_ref()
            .is_none_or(|active| !active.is_live || !active.at_live_edge);
        let fallback_format = player::ytdl_format_for_codec_pref(
            self.settings.playback.value.max_height,
            self.settings.prefs.value.video_codec_preference,
        );
        let result = if ready {
            player.set("options/start", "none").and_then(|()| {
                player.quality(&video.url, quality, &fallback_format, preserve_position)
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
        let fallback_chapters = |url: &str| {
            let desc = self.description_overlay_chapters(url);
            if !desc.is_empty() {
                return desc;
            }
            self.watch
                .live
                .info
                .as_ref()
                .filter(|_| self.watch.live.video_url.as_deref() == Some(url))
                .map(|info| info.chapters.clone())
                .unwrap_or_default()
        };
        let Some(player) = &self.player else {
            return video_url
                .as_deref()
                .map(fallback_chapters)
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
                chapters.extend(fallback_chapters(url));
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
                    && self.raw_path_matches_video(telemetry.raw_path.as_deref(), &video.url)
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
        {
            if let Some(skip_target) =
                self.try_auto_skip_sponsor_segment(position, duration, is_live, now)
                && let Some(player) = &self.player
            {
                match player.seek(skip_target, "absolute+exact") {
                    Ok(()) => self.record_manual_seek(skip_target, duration, now),
                    Err(error) => self.message = error,
                }
            } else if !is_live
                && let Some(chapter_target) = self.try_auto_skip_keyword_chapter(position, duration)
            {
                self.record_manual_seek(chapter_target, duration, now);
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

        let has_queued = self.has_next_video_target();
        let has_previous = self.has_previous_video_target();
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
        let manifest_qualities = self
            .playback
            .current_manifest
            .as_ref()
            .filter(|m| {
                !m.is_expired()
                    && !m.is_live
                    && history::youtube_video_id(&video.url).as_deref() == Some(m.video_id.as_str())
            })
            .map(|m| m.available_heights())
            .filter(|h| !h.is_empty());
        let overlay_qualities =
            manifest_qualities.unwrap_or_else(|| self.playback.qualities.clone());
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
            qualities: overlay_qualities,
            qualities_loading: self.is_playback_metadata_loading(),
            has_previous,
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
            self.tick_sleep_timer(ctx, telemetry.ended, now);
            if fullscreen && overlay_active {
                let scroll_y = ctx
                    .data(|d| d.get_temp::<f32>(egui::Id::new("watch_fullscreen_scroll_y")))
                    .unwrap_or(0.0);
                if scroll_y < 16.0 {
                    Self::paint_fullscreen_scroll_hint(ctx, video_rect, self.ui_pt());
                }
            }
        } else {
            self.tick_sleep_timer(ctx, telemetry.ended, now);
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
            self.trigger_next_video_action(false);
        }
        if actions.close_clicked {
            self.close_video(ctx);
        }
    }

    pub(super) fn current_sleep_timer_overlay_state(
        ctx: &egui::Context,
        now: Instant,
    ) -> (Option<overlay::SleepTimerOption>, Option<u64>) {
        let target = ctx
            .data(|d| d.get_temp::<SleepTimerTarget>(egui::Id::new("fasttube_sleep_timer_target")))
            .unwrap_or(SleepTimerTarget::Off);
        match target {
            SleepTimerTarget::Off => (None, None),
            SleepTimerTarget::EndOfVideo => (Some(overlay::SleepTimerOption::EndOfVideo), None),
            SleepTimerTarget::Until(deadline) => {
                let rem = deadline.saturating_duration_since(now).as_secs();
                let mins = rem.div_ceil(60).max(1) as u32;
                (Some(overlay::SleepTimerOption::Minutes(mins)), Some(rem))
            }
        }
    }

    pub(super) fn active_seek_ripple_overlay_state(ctx: &egui::Context) -> Option<(bool, f32)> {
        let id = egui::Id::new("fasttube_player_seek_ripple");
        let ripple = ctx.data(|d| d.get_temp::<SeekRippleFeedback>(id))?;
        let elapsed_ms = ripple.started_at.elapsed().as_secs_f32() * 1000.0;
        if elapsed_ms >= SEEK_RIPPLE_DURATION_MS {
            ctx.data_mut(|d| d.remove::<SeekRippleFeedback>(id));
            return None;
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        let progress = (elapsed_ms / SEEK_RIPPLE_DURATION_MS).clamp(0.0, 1.0);
        let forward = matches!(ripple.zone, PlayerClickZone::Right);
        Some((forward, progress))
    }

    pub(super) fn resolve_up_next_overlay_info(
        &mut self,
        ctx: &egui::Context,
        playback: &OverlayPlaybackContext,
    ) -> Option<overlay::UpNextOverlayInfo> {
        let id = egui::Id::new("fasttube_up_next_countdown");
        if playback.is_live || !playback.autoplay || playback.loop_video || playback.mini_player {
            ctx.data_mut(|d| d.remove::<UpNextCountdownState>(id));
            return None;
        }
        let near_eof = playback.ended
            || playback.duration.is_some_and(|dur| {
                dur.is_finite()
                    && dur >= 15.0
                    && playback.position.is_finite()
                    && (dur - playback.position) <= f64::from(UP_NEXT_COUNTDOWN_SECS)
            });
        let existing = ctx.data(|d| d.get_temp::<UpNextCountdownState>(id));
        if !near_eof {
            if existing
                .as_ref()
                .is_some_and(|st| st.source_video_url != playback.video.url)
                || playback
                    .duration
                    .is_some_and(|dur| (dur - playback.position) > 8.0)
            {
                ctx.data_mut(|d| d.remove::<UpNextCountdownState>(id));
            }
            return None;
        }
        let mut state = match existing {
            Some(st) if st.source_video_url == playback.video.url => st,
            _ => {
                let next_video = self.peek_up_next_video()?;
                UpNextCountdownState {
                    source_video_url: playback.video.url.clone(),
                    next_video,
                    started_at: playback.now,
                    cancelled: false,
                    triggered: false,
                }
            }
        };
        if state.cancelled || state.triggered {
            ctx.data_mut(|d| d.insert_temp(id, state));
            return None;
        }
        match evaluate_up_next_countdown(state.started_at, state.cancelled, playback.now) {
            UpNextCountdownTick::Inactive => None,
            UpNextCountdownTick::TriggerNext => {
                state.triggered = true;
                ctx.data_mut(|d| d.insert_temp(id, state));
                self.playback.autoplay_eof_handled = true;
                self.trigger_next_video_action(false);
                None
            }
            UpNextCountdownTick::Counting { remaining_secs, .. } => {
                ctx.request_repaint_after(Duration::from_millis(50));
                let info = overlay::UpNextOverlayInfo {
                    title: state.next_video.title.clone(),
                    channel: state.next_video.channel.clone(),
                    thumbnail_url: (!state.next_video.thumbnail.trim().is_empty())
                        .then(|| state.next_video.thumbnail.clone()),
                    remaining_secs,
                    total_secs: UP_NEXT_COUNTDOWN_SECS,
                };
                ctx.data_mut(|d| d.insert_temp(id, state));
                Some(info)
            }
        }
    }

    pub(crate) fn toggle_ambient_mode(&mut self) {
        let mut next = self.settings.prefs.value.clone();
        next.ambient_mode = !next.ambient_mode;
        let enabled = next.ambient_mode;
        self.settings.apply_app_preferences(next);
        self.toasts.info(if enabled {
            self.pick("Modo ambiente ativado.", "Ambient mode enabled.")
        } else {
            self.pick("Modo ambiente desativado.", "Ambient mode disabled.")
        });
    }

    pub(crate) fn toggle_stable_volume(&mut self) {
        let mut next = self.settings.prefs.value.clone();
        next.audio_normalization = !next.audio_normalization;
        let enabled = next.audio_normalization;
        self.settings.apply_app_preferences(next);
        #[cfg(windows)]
        if let Some(player) = &self.player
            && let Err(error) = player.apply_audio_filters(enabled, self.playback.audio_eq)
        {
            self.message = error;
        }
        self.toasts.info(if enabled {
            self.pick("Volume est\u{00e1}vel ativado.", "Stable volume enabled.")
        } else {
            self.pick(
                "Volume est\u{00e1}vel desativado.",
                "Stable volume disabled.",
            )
        });
    }

    pub(crate) fn set_sleep_timer_minutes(
        &mut self,
        ctx: &egui::Context,
        minutes: Option<u32>,
        end_of_video: bool,
    ) {
        let target = if end_of_video {
            SleepTimerTarget::EndOfVideo
        } else if let Some(mins) = minutes.filter(|&m| m > 0) {
            SleepTimerTarget::Until(Instant::now() + Duration::from_secs(u64::from(mins) * 60))
        } else {
            SleepTimerTarget::Off
        };
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("fasttube_sleep_timer_target"), target));
        let pt = self.ui_pt();
        let msg = match target {
            SleepTimerTarget::Off => {
                if pt {
                    "Temporizador de sono desativado.".to_owned()
                } else {
                    "Sleep timer turned off.".to_owned()
                }
            }
            SleepTimerTarget::EndOfVideo => {
                if pt {
                    "Temporizador de sono definido para o fim do v\u{00ed}deo.".to_owned()
                } else {
                    "Sleep timer set for end of video.".to_owned()
                }
            }
            SleepTimerTarget::Until(_) => {
                let mins = minutes.unwrap_or(0);
                if pt {
                    format!("Temporizador de sono definido para {mins} min.")
                } else {
                    format!("Sleep timer set for {mins} min.")
                }
            }
        };
        self.toasts.info(msg);
    }

    pub(crate) fn apply_ultrawide_zoom(&mut self, panscan: f32, video_zoom: f32) {
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let _ = player.set("panscan", &format!("{:.2}", panscan.clamp(0.0, 1.0)));
            let _ = player.set("video-zoom", &format!("{:.2}", video_zoom.clamp(-0.5, 1.5)));
        }
        #[cfg(not(windows))]
        let _ = (panscan, video_zoom);
    }

    pub(crate) fn peek_up_next_video(&mut self) -> Option<Video> {
        if let Some(next_q) = self.queue.data.videos().first() {
            return Some(next_q.clone());
        }
        if let Some((_, next_pl)) = self
            .watch
            .active_playlist
            .as_ref()
            .and_then(|pl| pl.peek_next())
        {
            return Some(next_pl);
        }
        if let Some(next_fwd) = self.playback.video_forward_stack.last() {
            return Some(next_fwd.clone());
        }
        if self.is_watch_short_mode() {
            return self.next_or_prev_short(true);
        }
        self.next_recommended_video()
    }

    pub(crate) fn cancel_up_next_countdown(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new("fasttube_up_next_countdown");
        ctx.data_mut(|d| {
            if let Some(mut state) = d.get_temp::<UpNextCountdownState>(id) {
                state.cancelled = true;
                d.insert_temp(id, state);
            }
        });
        self.playback.autoplay_eof_handled = true;
    }

    pub(crate) fn play_up_next_now(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| {
            d.remove::<UpNextCountdownState>(egui::Id::new("fasttube_up_next_countdown"))
        });
        self.playback.autoplay_eof_handled = true;
        self.trigger_next_video_action(false);
    }

    fn resolve_ambient_color_for_video(ctx: &egui::Context, video: &Video) -> Color32 {
        let cache_id = egui::Id::new("fasttube_ambient_color_cache");
        let existing = ctx.data(|d| d.get_temp::<AmbientColorCacheEntry>(cache_id));
        if let Some(mut entry) = existing
            && entry.video_url == video.url
        {
            let resolved = entry
                .pending
                .as_ref()
                .and_then(|slot| slot.try_lock().ok().and_then(|mut guard| guard.take()));
            if let Some((resolved_url, resolved_color)) = resolved
                && resolved_url == video.url
            {
                entry.color = resolved_color;
                entry.pending = None;
                ctx.data_mut(|d| d.insert_temp(cache_id, entry.clone()));
            }
            return entry.color;
        }
        let fallback = fallback_ambient_color_for_video(video);
        let pending = if cfg!(test) {
            None
        } else {
            let thumb_url = if !video.thumbnail.trim().is_empty() {
                video.thumbnail.clone()
            } else if let Some(vid) = history::youtube_video_id(&video.url) {
                format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg")
            } else {
                String::new()
            };
            if thumb_url.is_empty() {
                None
            } else {
                let slot = Arc::new(std::sync::Mutex::new(None));
                let slot_bg = Arc::clone(&slot);
                let video_url = video.url.clone();
                let ctx_bg = ctx.clone();
                std::thread::spawn(move || {
                    let bytes = thumbnails::DiskImageCache::default_production()
                        .and_then(|dc| dc.read_bytes(&thumb_url).ok().flatten())
                        .or_else(|| {
                            crate::http_client::no_redirect()
                                .get(&thumb_url)
                                .timeout(Duration::from_secs(6))
                                .send()
                                .ok()
                                .filter(|r| r.status().is_success())
                                .and_then(|r| r.bytes().ok())
                                .map(|b| b.to_vec())
                        });
                    if let Some(raw) = bytes
                        && let Some(accent) = compute_dominant_accent_from_image_bytes(&raw)
                        && let Ok(mut guard) = slot_bg.lock()
                    {
                        *guard = Some((video_url, accent));
                        ctx_bg.request_repaint();
                    }
                });
                Some(slot)
            }
        };
        let entry = AmbientColorCacheEntry {
            video_url: video.url.clone(),
            color: fallback,
            pending,
        };
        ctx.data_mut(|d| d.insert_temp(cache_id, entry));
        fallback
    }

    pub(super) fn record_seek_ripple(ctx: &egui::Context, zone: PlayerClickZone) {
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new("fasttube_player_seek_ripple"),
                SeekRippleFeedback {
                    zone,
                    started_at: Instant::now(),
                },
            );
        });
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    pub(super) fn is_hold_speed_boost_active(ctx: &egui::Context) -> bool {
        ctx.data(|d| d.get_temp::<HoldSpeedBoostState>(egui::Id::new("fasttube_hold_speed_boost")))
            .is_some_and(|s| s.active)
    }

    pub(super) fn update_hold_speed_boost(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        response: &egui::Response,
        playback: &OverlayPlaybackContext,
        over_bar: bool,
        menu_open: bool,
    ) -> bool {
        let id = egui::Id::new("fasttube_hold_speed_boost");
        let mut state = ctx
            .data(|d| d.get_temp::<HoldSpeedBoostState>(id))
            .unwrap_or_default();
        let mouse_held = !over_bar
            && !menu_open
            && !playback.mini_player
            && self.playback.seek_drag.is_none()
            && response.is_pointer_button_down_on()
            && ctx.input(|i| i.pointer.primary_down());
        let space_held = !ctx.egui_wants_keyboard_input()
            && !menu_open
            && !self.ui.settings_open
            && ctx.input(|i| {
                i.key_down(egui::Key::Space)
                    && !i.modifiers.ctrl
                    && !i.modifiers.alt
                    && !i.modifiers.shift
            });
        let held = (mouse_held || space_held) && rect.width() > 40.0;
        let transition = state.update(held, playback.speed, playback.now);
        match transition {
            HoldSpeedTransition::Activate2x => {
                self.set_player_speed(HOLD_SPEED_MULTIPLIER);
            }
            HoldSpeedTransition::Restore(prev) => {
                self.set_player_speed(prev);
            }
            HoldSpeedTransition::None => {}
        }
        if state.pressed_at.is_some() && !state.active {
            ctx.request_repaint_after(Duration::from_millis(32));
        }
        let consumed = if !held {
            state.take_suppressed_click()
        } else {
            state.active
        };
        ctx.data_mut(|d| d.insert_temp(id, state));
        consumed
    }

    fn tick_sleep_timer(&mut self, ctx: &egui::Context, ended: bool, now: Instant) {
        let id = egui::Id::new("fasttube_sleep_timer_target");
        let target = ctx
            .data(|d| d.get_temp::<SleepTimerTarget>(id))
            .unwrap_or(SleepTimerTarget::Off);
        match evaluate_sleep_timer_tick(target, ended, now) {
            SleepTimerTickOutcome::Idle => {}
            SleepTimerTickOutcome::Active { remaining_secs } => {
                if remaining_secs.is_some() {
                    ctx.request_repaint_after(Duration::from_secs(1));
                }
            }
            SleepTimerTickOutcome::ExpiredPause => {
                ctx.data_mut(|d| d.insert_temp(id, SleepTimerTarget::Off));
                self.playback.autoplay_eof_handled = true;
                #[cfg(windows)]
                if let Some(player) = &self.player {
                    let _ = player.set("pause", "yes");
                }
                let msg = self.pick(
                    "Temporizador de sono encerrado \u{2014} reprodu\u{00e7}\u{00e3}o pausada.",
                    "Sleep timer ended \u{2014} playback paused.",
                );
                let osd = if self.ui_pt() {
                    "\u{1f319} Pausado"
                } else {
                    "\u{1f319} Paused"
                };
                Self::record_player_osd(ctx, osd);
                self.toasts.info(msg);
            }
        }
    }

    fn paint_fullscreen_scroll_hint(ctx: &egui::Context, video_rect: egui::Rect, pt: bool) {
        if video_rect.height() < 240.0 {
            return;
        }
        let layer_id = egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("fasttube_fullscreen_scroll_hint_layer"),
        );
        let painter = ctx.layer_painter(layer_id).with_clip_rect(video_rect);
        let label = if pt {
            "\u{2193} Role para detalhes"
        } else {
            "\u{2193} Scroll for details"
        };
        let galley = painter.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(12.0),
            Color32::from_rgba_unmultiplied(245, 245, 250, 225),
        );
        let pill_size = egui::vec2(galley.size().x + 20.0, galley.size().y + 10.0);
        let pill_rect = egui::Rect::from_center_size(
            egui::pos2(
                video_rect.center().x,
                (video_rect.bottom() - overlay::OVERLAY_HEIGHT - 22.0).max(video_rect.top() + 40.0),
            ),
            pill_size,
        );
        painter.rect_filled(pill_rect, pill_size.y * 0.5, Color32::from_black_alpha(165));
        painter.rect_stroke(
            pill_rect,
            pill_size.y * 0.5,
            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36)),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            egui::pos2(
                pill_rect.center().x - galley.size().x * 0.5,
                pill_rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            Color32::from_rgba_unmultiplied(245, 245, 250, 225),
        );
    }

    pub(crate) fn set_player_speed(&mut self, speed: f64) {
        let clamped = speed.clamp(0.25, 4.0);
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
        if self.playback.fullscreen && !self.playback.mini_player {
            let scroll_out = egui::ScrollArea::vertical()
                .id_salt("watch_fullscreen_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.watch_primary_with_metadata_ui(
                        ui,
                        ctx,
                        column_width,
                        available_height,
                        mode,
                        false,
                    );
                    if let Some(video) = self.selected.clone() {
                        let palette = self.palette();
                        let pt = self.ui_pt();
                        let details_w = (column_width - 48.0).clamp(320.0, 1080.0);
                        let side_pad = ((column_width - details_w) * 0.5).max(16.0);
                        #[cfg(windows)]
                        let resolution_badge = (self.playback.last_video_size_url.as_deref()
                            == Some(video.url.as_str()))
                        .then_some(self.playback.last_video_size)
                        .flatten()
                        .map(|size| {
                            if pt {
                                format!("Reproduzindo em {size}")
                            } else {
                                format!("Playing at {size}")
                            }
                        });
                        #[cfg(not(windows))]
                        let resolution_badge: Option<String> = None;

                        egui::Frame::new()
                            .fill(palette.card_bg)
                            .inner_margin(egui::Margin::symmetric(0, 16))
                            .show(ui, |ui| {
                                ui.set_min_width(column_width);
                                ui.horizontal_top(|ui| {
                                    ui.add_space(side_pad);
                                    ui.vertical(|ui| {
                                        ui.set_width(details_w);
                                        ui.set_max_width(details_w);
                                        ui.horizontal(|ui| {
                                            if styled_compact_button(
                                                ui,
                                                if pt {
                                                    "\u{2191} Voltar ao v\u{00ed}deo"
                                                } else {
                                                    "\u{2191} Back to video"
                                                },
                                                false,
                                                &palette,
                                            )
                                            .clicked()
                                            {
                                                ui.scroll_to_rect(
                                                    egui::Rect::from_min_size(
                                                        egui::pos2(0.0, 0.0),
                                                        egui::vec2(1.0, 1.0),
                                                    ),
                                                    Some(egui::Align::TOP),
                                                );
                                            }
                                        });
                                        self.watch_metadata_card_ui(
                                            ui,
                                            &video,
                                            details_w,
                                            resolution_badge.as_deref(),
                                        );
                                    });
                                });
                            });
                    }
                });
            ctx.data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("watch_fullscreen_scroll_y"),
                    scroll_out.state.offset.y,
                );
            });
            self.watch_report_modal_ui(ctx);
            return;
        }
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
        if self.settings.prefs.value.ambient_mode
            && !self.playback.fullscreen
            && !self.playback.mini_player
            && !self.playback.in_app_mini_player
            && !self.ui.settings_open
            && !self.playback.audio_only
        {
            let ambient_color = Self::resolve_ambient_color_for_video(ctx, &video);
            let dark_theme = self.is_dark_theme();
            let glow_painter = ui.painter().with_clip_rect(ui.clip_rect().expand(52.0));
            paint_ambient_mode_glow(&glow_painter, rect, ambient_color, dark_theme);
        }
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
        let has_queued = self.has_next_video_target();
        let fullscreen = self.playback.fullscreen;
        let theater_mode = self.playback.theater;
        let palette = self.palette();
        if mode != WatchLayoutMode::Short {
            egui::Frame::new()
                .fill(palette.player_bar_bg)
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_max_width(player_size.x);
                    ui.horizontal_wrapped(|ui| {
                        if styled_compact_button_enabled(
                            ui,
                            has_queued,
                            "⏭ Próximo",
                            false,
                            &palette,
                        )
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
        }
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
            self.trigger_next_video_action(false);
        }
        if close_clicked {
            self.close_video(ctx);
        }
    }

    pub(crate) fn reset_video_and_audio_eq(&mut self) {
        self.playback.brightness = 0;
        self.playback.contrast = 0;
        self.playback.saturation = 0;
        self.playback.audio_eq = overlay::AudioEqPreset::Flat;
        #[cfg(windows)]
        if let Some(player) = &self.player {
            if let Err(error) = player.set_video_eq(0, 0, 0) {
                self.message = error;
            }
            if let Err(error) = player.apply_audio_filters(
                self.settings.prefs.value.audio_normalization,
                overlay::AudioEqPreset::Flat,
            ) {
                self.message = error;
            }
        }
    }
}
