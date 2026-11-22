use crate::*;

impl FastTube {
    #[allow(dead_code)]
    pub(crate) fn audio_normalization_af(enabled: bool) -> &'static str {
        if enabled {
            "lavfi=[loudnorm=I=-16:TP=-1.5:LRA=11]"
        } else {
            ""
        }
    }

    pub(crate) fn is_video_at_eof(&self) -> bool {
        #[cfg(windows)]
        {
            self.player
                .as_ref()
                .is_some_and(|player| player.flag_cstr(c"eof-reached") == Some(true))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    #[inline]
    pub(crate) fn is_watch_page_active(&self) -> bool {
        self.selected.is_some() && !self.playback.in_app_mini_player
    }

    #[inline]
    pub(crate) fn is_in_app_mini_player_active(&self) -> bool {
        self.selected.is_some() && self.playback.in_app_mini_player
    }

    pub(crate) fn has_modal_covering_mini_player(&self) -> bool {
        self.ui.settings_open
            || self.ui.shortcuts_open
            || self.ui.command_palette.open
            || self.feed.search_filters_open
            || self.feed.search_filters_modal_open
            || self.feed.search_save_modal_video.is_some()
            || self.feed.search_report_modal_video.is_some()
            || self.feed.share_modal_video.is_some()
            || self.channels.about_modal_open
    }

    pub(crate) fn enter_in_app_mini_player(&mut self, ctx: &egui::Context) -> bool {
        if self.selected.is_none() {
            return false;
        }
        if self.playback.fullscreen {
            self.toggle_fullscreen(ctx);
        }
        if self.playback.mini_player {
            self.toggle_mini_player(ctx);
        }
        self.playback.in_app_mini_player = true;
        self.playback.player_menu = None;
        self.playback.player_context_menu = None;
        self.playback.seek_drag = None;
        self.sync_player_window_visibility();
        ctx.request_repaint();
        true
    }

    pub(crate) fn exit_in_app_mini_player(&mut self, ctx: &egui::Context) {
        self.playback.in_app_mini_player = false;
        self.ui.settings_open = false;
        self.ui.navigation_open = false;
        self.close_channel_view();
        self.sync_player_window_visibility();
        ctx.request_repaint();
    }

    pub(crate) fn toggle_in_app_mini_player_playback(&mut self) {
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let ended = player.flag_cstr(c"eof-reached") == Some(true);
            let paused = player.flag_cstr(c"pause") == Some(true);
            let result = if ended {
                player
                    .seek(0.0, "absolute+exact")
                    .and_then(|()| player.set("pause", "no"))
            } else {
                player.set("pause", if paused { "no" } else { "yes" })
            };
            match result {
                Ok(()) => {
                    self.playback.mini_player_paused = if ended { false } else { !paused };
                }
                Err(error) => {
                    self.message = error;
                }
            }
            return;
        }
        self.playback.mini_player_paused = !self.playback.mini_player_paused;
    }

    pub(crate) fn toggle_in_app_mini_player_mute(&mut self) {
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let muted = player
                .flag_cstr(c"mute")
                .unwrap_or(self.playback.mini_player_muted);
            let vol = player
                .double_cstr(c"volume")
                .map(|v| v.clamp(0.0, 100.0) as f32)
                .unwrap_or(self.playback.mini_player_volume);
            let (next_muted, next_vol) = if vol <= 0.5 {
                (false, 50.0)
            } else {
                (!muted, vol)
            };
            let mut result = player.set("mute", if next_muted { "yes" } else { "no" });
            if result.is_ok() && (next_vol - vol).abs() > 0.1 {
                result = player.set("volume", &format!("{next_vol:.0}"));
            }
            match result {
                Ok(()) => {
                    self.playback.mini_player_muted = next_muted;
                    self.playback.mini_player_volume = next_vol;
                }
                Err(error) => {
                    self.message = error;
                }
            }
            return;
        }
        let muted = self.playback.mini_player_muted;
        let vol = self.playback.mini_player_volume;
        if vol <= 0.5 {
            self.playback.mini_player_muted = false;
            self.playback.mini_player_volume = 50.0;
        } else {
            self.playback.mini_player_muted = !muted;
        }
    }

    pub(crate) fn set_in_app_mini_player_volume(&mut self, volume: f32) {
        let clamped = if volume.is_finite() {
            volume.clamp(0.0, 100.0).round()
        } else {
            self.playback.mini_player_volume.clamp(0.0, 100.0)
        };
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let mut result = player.set("volume", &format!("{clamped:.0}"));
            if result.is_ok() && clamped > 0.0 && player.flag_cstr(c"mute") == Some(true) {
                result = player.set("mute", "no");
            }
            match result {
                Ok(()) => {
                    self.playback.mini_player_volume = clamped;
                    if clamped > 0.0 {
                        self.playback.mini_player_muted = false;
                    }
                }
                Err(error) => {
                    self.message = error;
                }
            }
            return;
        }
        self.playback.mini_player_volume = clamped;
        if clamped > 0.0 && self.playback.mini_player_muted {
            self.playback.mini_player_muted = false;
        }
    }

    pub(crate) fn record_video_back_stack(&mut self, previous: Video, incoming_url: &str) {
        if Self::same_video_target(&previous.url, incoming_url) {
            return;
        }
        if self
            .playback
            .video_back_stack
            .last()
            .is_none_or(|top| !Self::same_video_target(&top.url, &previous.url))
        {
            self.playback.video_back_stack.push(previous);
            if self.playback.video_back_stack.len() > 50 {
                self.playback.video_back_stack.remove(0);
            }
        }
        self.playback.video_forward_stack.clear();
    }

    pub(crate) fn previous_history_video_candidate(&self) -> Option<Video> {
        let selected_id = self
            .selected
            .as_ref()
            .and_then(|v| history::youtube_video_id(&v.url));
        for entry in self.history.data.entries() {
            if selected_id.as_deref() == Some(entry.video_id.as_str()) {
                continue;
            }
            let Some(url) = history::youtube_watch_url(&entry.video_id) else {
                continue;
            };
            if self
                .playback
                .video_forward_stack
                .iter()
                .any(|v| Self::same_video_target(&v.url, &url))
            {
                continue;
            }
            if let Some(known) = self
                .feed
                .videos
                .iter()
                .find(|v| Self::same_video_target(&v.url, &url))
            {
                return Some(known.clone());
            }
            return Some(Video {
                title: entry.title.clone(),
                channel: entry.channel.clone(),
                duration: String::new(),
                url,
                thumbnail: format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", entry.video_id),
            });
        }
        None
    }

    pub(crate) fn has_previous_video_target(&self) -> bool {
        if let Some(pl) = &self.watch.active_playlist {
            return pl.has_previous()
                || !self.playback.video_back_stack.is_empty()
                || (self.is_watch_short_mode() && self.next_or_prev_short(false).is_some());
        }
        !self.playback.video_back_stack.is_empty()
            || (self.is_watch_short_mode() && self.next_or_prev_short(false).is_some())
            || self.previous_history_video_candidate().is_some()
    }

    pub(crate) fn has_next_video_target(&mut self) -> bool {
        !self.queue.data.is_empty()
            || self
                .watch
                .active_playlist
                .as_ref()
                .is_some_and(app_state::ActivePlaylistPlayback::has_next)
            || !self.playback.video_forward_stack.is_empty()
            || (self.is_watch_short_mode() && self.next_or_prev_short(true).is_some())
            || self.next_recommended_video().is_some()
    }

    pub(crate) fn start_playlist_playback(&mut self, playlist: app_state::ActivePlaylistPlayback) {
        let Some(video) = playlist.current_video().cloned() else {
            return;
        };
        if !playlist.is_mix
            && let Some(cancel) = self.watch.mix_cancel.take()
        {
            cancel.cancel();
            self.watch.mix_rx = None;
            self.watch.mix_active_request_id = None;
        }
        self.watch.active_playlist = Some(playlist);
        self.playback.pending_open_keep_playlist = true;
        self.playback.pending_open_suppress_back_stack = false;
        self.playback.pending_open_video = Some(video);
        self.maybe_prefetch_next_video();
    }

    pub(crate) fn trigger_previous_video_action(
        &mut self,
        is_live: bool,
        position_secs: f64,
        keep_mini_player: bool,
    ) {
        if let Some((prev_pos, prev_video)) = self
            .watch
            .active_playlist
            .as_ref()
            .and_then(|pl| pl.peek_previous())
        {
            if let Some(pl) = &mut self.watch.active_playlist {
                pl.select_pos(prev_pos);
            }
            self.playback.pending_open_keep_mini_player = keep_mini_player;
            self.playback.pending_open_keep_playlist = true;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(prev_video);
            return;
        }
        let prev = self
            .playback
            .video_back_stack
            .pop()
            .or_else(|| {
                if self.is_watch_short_mode() {
                    self.next_or_prev_short(false)
                } else {
                    None
                }
            })
            .or_else(|| {
                if self.watch.active_playlist.is_none() {
                    self.previous_history_video_candidate()
                } else {
                    None
                }
            });
        if let Some(prev_video) = prev {
            if let Some(current) = self.selected.clone()
                && !Self::same_video_target(&current.url, &prev_video.url)
                && self
                    .playback
                    .video_forward_stack
                    .last()
                    .is_none_or(|top| !Self::same_video_target(&top.url, &current.url))
            {
                self.playback.video_forward_stack.push(current);
                if self.playback.video_forward_stack.len() > 50 {
                    self.playback.video_forward_stack.remove(0);
                }
            }
            self.playback.pending_open_keep_mini_player = keep_mini_player;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(prev_video);
        } else if !is_live && position_secs > 1.0 {
            self.seek_to_seconds(0.0);
        }
    }

    pub(crate) fn trigger_next_video_action(&mut self, keep_mini_player: bool) {
        if !self.queue.data.is_empty() {
            self.playback.pending_open_suppress_back_stack = false;
            if keep_mini_player {
                self.playback.pending_open_keep_mini_player = true;
                self.queue.pending_advance = true;
            } else {
                self.request_queue_advance();
            }
        } else if let Some((next_pos, next_video)) = self
            .watch
            .active_playlist
            .as_ref()
            .and_then(|pl| pl.peek_next())
        {
            if let Some(pl) = &mut self.watch.active_playlist {
                pl.select_pos(next_pos);
            }
            self.playback.pending_open_keep_mini_player = keep_mini_player;
            self.playback.pending_open_keep_playlist = true;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(next_video);
        } else if let Some(next_video) = self.playback.video_forward_stack.pop() {
            if let Some(current) = self.selected.clone()
                && !Self::same_video_target(&current.url, &next_video.url)
                && self
                    .playback
                    .video_back_stack
                    .last()
                    .is_none_or(|top| !Self::same_video_target(&top.url, &current.url))
            {
                self.playback.video_back_stack.push(current);
                if self.playback.video_back_stack.len() > 50 {
                    self.playback.video_back_stack.remove(0);
                }
            }
            self.playback.pending_open_keep_mini_player = keep_mini_player;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(next_video);
        } else if self.is_watch_short_mode()
            && let Some(next_video) = self.next_or_prev_short(true)
        {
            self.playback.pending_open_keep_mini_player = keep_mini_player;
            self.playback.pending_open_suppress_back_stack = false;
            self.playback.pending_open_video = Some(next_video);
        } else if let Some(next_video) = self.next_recommended_video() {
            self.playback.pending_open_keep_mini_player = keep_mini_player;
            self.playback.pending_open_suppress_back_stack = false;
            self.playback.pending_open_video = Some(next_video);
        }
    }

    pub(crate) fn trigger_in_app_mini_player_previous(
        &mut self,
        is_live: bool,
        position_secs: f64,
    ) {
        self.trigger_previous_video_action(is_live, position_secs, true);
    }

    pub(crate) fn trigger_in_app_mini_player_next(&mut self) {
        self.trigger_next_video_action(true);
    }

    pub(crate) fn maybe_enter_auto_mini_player(&mut self, ctx: &egui::Context) -> bool {
        if self.is_in_app_mini_player_active() {
            return true;
        }
        if self.selected.is_some()
            && self.settings.prefs.value.auto_mini_player
            && !self.playback.mini_player
            && !self.is_video_at_eof()
        {
            return self.enter_in_app_mini_player(ctx);
        }
        false
    }

    pub(crate) fn remember_current_player_volume(&mut self) {
        if !self.settings.prefs.value.remember_volume {
            return;
        }
        #[cfg(windows)]
        {
            let Some(player) = &self.player else {
                return;
            };
            if let Some(vol) = player.number("volume") {
                let clamped = vol.round().clamp(0.0, 200.0) as u8;
                if clamped != self.settings.prefs.value.default_volume_percent {
                    let mut prefs = self.settings.prefs.value.clone();
                    prefs.default_volume_percent = clamped;
                    self.apply_app_preferences(prefs);
                }
            }
        }
    }

    pub(crate) fn maybe_auto_activate_subtitle(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        {
            if !self.settings.prefs.value.auto_subtitles
                || self.playback.auto_subtitle_attempted
                || self.selected.is_none()
                || !self.watch.extras.subtitles_loaded
            {
                return;
            }
            self.playback.auto_subtitle_attempted = true;
            let (has_active, _) = self.collect_overlay_subtitles();
            if has_active {
                return;
            }
            let pref = self
                .settings
                .prefs
                .value
                .preferred_subtitle_lang
                .trim()
                .to_ascii_lowercase()
                .replace('_', "-");
            let pref = if pref.is_empty() { "pt" } else { pref.as_str() };
            let matches_exact_or_region = |lang: &str| {
                let lower = lang.trim().to_ascii_lowercase().replace('_', "-");
                lower == pref
                    || lower
                        .strip_prefix(pref)
                        .is_some_and(|rest| rest.starts_with('-'))
            };
            let base_pref = pref.split_once('-').map(|(base, _)| base);
            let matches_base = |lang: &str| {
                let Some(base) = base_pref else {
                    return false;
                };
                lang.trim().to_ascii_lowercase().replace('_', "-") == base
            };
            let subs = &self.watch.extras.subtitles;
            let chosen = subs
                .iter()
                .filter(|t| !t.automatic && matches_exact_or_region(&t.language))
                .chain(
                    subs.iter()
                        .filter(|t| !t.automatic && matches_base(&t.language)),
                )
                .chain(
                    subs.iter()
                        .filter(|t| t.automatic && matches_exact_or_region(&t.language)),
                )
                .chain(
                    subs.iter()
                        .filter(|t| t.automatic && matches_base(&t.language)),
                )
                .next()
                .cloned();
            if let Some(track) = chosen {
                self.select_overlay_subtitle(Some(&format!("yt:{}", track.url)), ctx);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = ctx;
        }
    }

    pub(crate) fn seek_step_secs(&self) -> u32 {
        match self.settings.prefs.value.seek_step_secs {
            5 | 10 | 15 | 30 => self.settings.prefs.value.seek_step_secs,
            _ => 10,
        }
    }

    pub(crate) fn has_pending_hwdec_restart(&self) -> bool {
        self.playback
            .active_hwdec
            .is_some_and(|active| active != self.settings.prefs.value.hwdec_mode)
    }

    pub(crate) fn is_video_known_live(&self, video: &Video) -> bool {
        is_live_duration_label(&video.duration)
            || self
                .metadata_for(&video.url)
                .is_some_and(|meta| meta.is_live)
            || self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.url == video.url && active.is_live)
            || self.watch.live.info.as_ref().is_some_and(|info| {
                info.is_live && self.watch.live.video_url.as_deref() == Some(video.url.as_str())
            })
    }

    pub(crate) fn active_video_is_live(&self) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|video| self.is_video_known_live(video))
            || self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.is_live)
    }

    pub(crate) fn planned_resume_ms(&mut self, video: &Video) -> Option<u64> {
        if !self.settings.prefs.value.resume_enabled || self.is_video_known_live(video) {
            return None;
        }
        let (resume, invalidated) = self.history.planned_resume_ms(video, &self.persistence);
        if invalidated {
            self.invalidate_feed_cache();
        }
        resume
    }

    pub(crate) fn record_active_playback_in_memory(&mut self) -> bool {
        if self.settings.prefs.value.pause_watch_history || self.ui.incognito_mode {
            return false;
        }
        let Some(active) = &mut self.playback.active else {
            return false;
        };
        if !active.has_live_sample {
            return false;
        }
        let Some(video_id) = active.video_id.clone() else {
            return false;
        };
        if !active.is_live
            && let Some(duration_ms) = active.duration_ms
        {
            if duration_ms == 0 {
                active.duration_ms = None;
            } else if active.position_ms > duration_ms {
                active.position_ms = duration_ms;
            }
        }
        let (record_position_ms, record_duration_ms) = if active.is_live {
            (0, None)
        } else {
            (active.position_ms, active.duration_ms)
        };
        let sanitized_title = sanitize_history_title(&active.title, &video_id);
        let sanitized_channel = sanitize_history_channel(&active.channel);
        let watched_at_ms = self
            .history
            .data
            .entries()
            .first()
            .filter(|existing| {
                existing.video_id == video_id
                    && existing.title == sanitized_title
                    && existing.channel == sanitized_channel
                    && existing.position_ms == record_position_ms
                    && existing.duration_ms == record_duration_ms
            })
            .and_then(|existing| existing.watched_at_ms)
            .or_else(|| Some(history::current_unix_epoch_ms()));
        let is_short_hint = active.url.contains("/shorts/").then_some(true).or_else(|| {
            self.video_metadata
                .get(&active.url)
                .is_some_and(|m| m.kind == search::SearchResultKind::Short)
                .then_some(true)
        });
        let published_time_text = self.video_metadata.get(&active.url).and_then(|meta| {
            meta.published_time_text
                .as_deref()
                .and_then(search::clean_published_time_text)
                .or_else(|| {
                    meta.upload_date
                        .as_deref()
                        .and_then(|d| recommendations::format_youtube_relative_time(d, None))
                })
        });
        let entry = HistoryEntry {
            video_id: video_id.clone(),
            title: sanitized_title,
            channel: sanitized_channel,
            position_ms: record_position_ms,
            duration_ms: record_duration_ms,
            watched_at_ms,
            is_short: is_short_hint,
            published_time_text,
            ..Default::default()
        };
        match self.history.data.record_active_tick(entry) {
            Ok(outcome) => {
                if outcome.changed {
                    self.history.dirty = true;
                }
                if outcome.ranking_changed {
                    self.invalidate_feed_cache();
                }
                outcome.changed
            }
            Err(_) => false,
        }
    }

    pub(crate) fn activate_video_playback(
        &mut self,
        video: &Video,
        resume_target_ms: Option<u64>,
        previous_player_pos_ms: Option<u64>,
        now: Instant,
    ) {
        self.ensure_history_synchronized();
        let video_id = history::youtube_video_id(&video.url);
        let is_live = self.is_video_known_live(video);
        let live_dvr = self
            .watch
            .live
            .info
            .as_ref()
            .filter(|info| {
                info.is_live && self.watch.live.video_url.as_deref() == Some(video.url.as_str())
            })
            .is_some_and(|info| info.is_live_dvr);
        let duration_ms = if is_live {
            None
        } else {
            let fallback_duration_ms = parse_duration_ms(&video.duration);
            let existing_duration_ms = video_id
                .as_deref()
                .and_then(|id| self.history.data.get(id))
                .and_then(|entry| entry.duration_ms);
            existing_duration_ms.or(fallback_duration_ms)
        };
        let effective_resume_ms = if is_live { None } else { resume_target_ms };
        let initial_position_ms = effective_resume_ms.unwrap_or(0);
        self.playback.active = Some(ActivePlayback {
            video_id: video_id.clone(),
            url: video.url.clone(),
            title: video.title.clone(),
            channel: video.channel.clone(),
            position_ms: initial_position_ms,
            duration_ms,
            resume_target_ms: effective_resume_ms,
            resume_seek_pending: effective_resume_ms.is_some(),
            explicit_seek_pending: false,
            start_option_cleared: false,
            has_live_sample: false,
            stale_guard_pos_ms: previous_player_pos_ms,
            is_live,
            live_dvr,
            at_live_edge: is_live,
            live_paused_since: None,
            live_paused_accumulated_secs: 0.0,
            live_behind_secs: 0.0,
        });
        self.history.save.last_position_ms = None;
        self.history.save.last_duration_ms = None;
        if video_id.is_some() {
            self.history.notice = None;
        }
        let _ = now;
    }

    pub(crate) fn is_playback_metadata_loading(&self) -> bool {
        self.playback.quality_task.is_running()
            || (self.watch.live.watch_info_loading && self.playback.qualities.is_empty())
    }

    pub(crate) fn start_quality_metadata_fallback(&mut self, url: String, ctx: &egui::Context) {
        self.playback
            .quality_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                search::playback_metadata_with_cancel(&url, &cancel)
            });
    }

    pub(crate) fn maybe_start_deferred_quality_fallback(&mut self, ctx: &egui::Context) {
        if self.playback.quality_task.is_running() || !self.playback.current_file_loaded {
            return;
        }
        let Some(url) = self.playback.quality_fallback_url.take() else {
            return;
        };
        if self.selected.as_ref().is_some_and(|v| v.url == url) {
            self.start_quality_metadata_fallback(url, ctx);
        }
    }

    #[cfg(windows)]
    pub(crate) fn sample_player_before_transition(&mut self, now: Instant) {
        let Some(active_video) = self.selected.clone() else {
            return;
        };
        let Some((raw_path, raw_pos, raw_dur, ended)) = self.player.as_ref().map(|player| {
            (
                player.property("path"),
                player.number("time-pos").filter(|&pos| pos >= 0.0),
                player.number("duration").filter(|&dur| dur > 0.0),
                player.property("eof-reached").as_deref() == Some("yes"),
            )
        }) else {
            return;
        };
        if raw_pos.is_some() {
            let _ = self.process_player_telemetry(
                &active_video,
                raw_path.as_deref(),
                raw_pos,
                raw_dur,
                ended,
                now,
            );
        }
    }

    pub(crate) fn process_player_telemetry(
        &mut self,
        video: &Video,
        raw_path: Option<&str>,
        raw_position: Option<f64>,
        raw_duration: Option<f64>,
        ended: bool,
        now: Instant,
    ) -> (f64, Option<f64>, Option<f64>, bool) {
        self.ensure_history_synchronized();
        if self
            .playback
            .active
            .as_ref()
            .is_none_or(|active| active.url != video.url)
        {
            let prev_pos = self
                .playback
                .active
                .as_ref()
                .filter(|active| active.has_live_sample)
                .map(|active| active.position_ms);
            self.flush_playback_history_at(now);
            let resume_ms = self.planned_resume_ms(video);
            self.activate_video_playback(video, resume_ms, prev_pos, now);
        }

        let path_matches = self.raw_path_matches_video(raw_path, &video.url);

        let Some(active) = &mut self.playback.active else {
            return (0.0, None, None, false);
        };

        if !path_matches {
            return (
                active.position_ms as f64 / 1000.0,
                active.duration_ms.map(|dur| dur as f64 / 1000.0),
                None,
                false,
            );
        }

        let Some(pos_secs) = raw_position.filter(|pos| pos.is_finite() && *pos >= 0.0) else {
            return (
                active.position_ms as f64 / 1000.0,
                active.duration_ms.map(|dur| dur as f64 / 1000.0),
                None,
                false,
            );
        };

        let mut sample_pos_ms = (pos_secs * 1000.0).round() as u64;
        let sample_dur_ms = raw_duration
            .filter(|dur| dur.is_finite() && *dur > 0.0)
            .map(|dur| (dur * 1000.0).round() as u64)
            .filter(|&dur| dur > 0);

        if !active.has_live_sample
            && let Some(stale_ms) = active.stale_guard_pos_ms
        {
            let expected_ms = active.resume_target_ms.unwrap_or(0);
            if stale_ms.abs_diff(expected_ms) > 2_000 && sample_pos_ms.abs_diff(stale_ms) <= 1_500 {
                return (
                    active.position_ms as f64 / 1000.0,
                    active.duration_ms.map(|dur| dur as f64 / 1000.0),
                    None,
                    false,
                );
            }
        }

        active.stale_guard_pos_ms = None;
        active.has_live_sample = true;
        let mut clear_start_option = false;
        if !active.start_option_cleared {
            active.start_option_cleared = true;
            clear_start_option = true;
        }
        if let Some(dur_ms) = sample_dur_ms {
            active.duration_ms = Some(dur_ms);
        }

        let mut pending_seek_secs = None;
        if active.is_live {
            active.resume_target_ms = None;
            active.resume_seek_pending = false;
            active.explicit_seek_pending = false;
        } else if active.resume_seek_pending {
            active.resume_seek_pending = false;
            if let Some(target_ms) = active.resume_target_ms {
                if !active.explicit_seek_pending
                    && history::should_resume(target_ms, active.duration_ms).is_none()
                {
                    active.resume_target_ms = None;
                    sample_pos_ms = 0;
                    if pos_secs > 1.0 {
                        pending_seek_secs = Some(0.0);
                    }
                } else if active.explicit_seek_pending {
                    active.explicit_seek_pending = false;
                    if target_ms == 0 {
                        if pos_secs > 1.0 {
                            pending_seek_secs = Some(0.0);
                        }
                    } else if sample_pos_ms.abs_diff(target_ms) > 1_000 {
                        pending_seek_secs = Some(target_ms as f64 / 1000.0);
                    }
                } else if sample_pos_ms + 1_000 < target_ms {
                    pending_seek_secs = Some(target_ms as f64 / 1000.0);
                }
            } else {
                active.explicit_seek_pending = false;
            }
        }

        if let Some(target_ms) = active.resume_target_ms {
            if sample_pos_ms.saturating_add(1_000) < target_ms {
                return (
                    active.position_ms as f64 / 1000.0,
                    active.duration_ms.map(|dur| dur as f64 / 1000.0),
                    pending_seek_secs,
                    clear_start_option,
                );
            }
            active.resume_target_ms = None;
        }

        if ended
            && !active.is_live
            && let Some(dur_ms) = active.duration_ms
        {
            sample_pos_ms = sample_pos_ms.max(dur_ms);
        }
        if let Some(dur_ms) = active.duration_ms {
            sample_pos_ms = sample_pos_ms.min(dur_ms);
        }
        active.position_ms = sample_pos_ms;
        let effective_pos = active.position_ms as f64 / 1000.0;
        let effective_dur = active.duration_ms.map(|dur| dur as f64 / 1000.0);
        let approaching_eof = !active.is_live
            && !ended
            && active
                .duration_ms
                .is_some_and(|dur| dur > 0 && dur.saturating_sub(active.position_ms) <= 20_000);

        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, ended) {
            self.save_history_to_disk(now);
        }
        self.maybe_send_remote_watchtime_heartbeat(effective_pos, false, ended, ended);
        if approaching_eof {
            self.maybe_prefetch_next_video();
        }

        (
            effective_pos,
            effective_dur,
            pending_seek_secs,
            clear_start_option,
        )
    }

    pub(crate) fn record_manual_seek(
        &mut self,
        position_secs: f64,
        duration_secs: Option<f64>,
        now: Instant,
    ) {
        if !position_secs.is_finite() {
            return;
        }
        let Some(active) = &mut self.playback.active else {
            return;
        };
        if let Some(dur_secs) = duration_secs.filter(|dur| dur.is_finite() && *dur > 0.0) {
            let dur_ms = (dur_secs * 1000.0).round() as u64;
            if dur_ms > 0 {
                active.duration_ms = Some(dur_ms);
            }
        }
        let mut pos_ms = (position_secs.max(0.0) * 1000.0).round() as u64;
        if let Some(dur_ms) = active.duration_ms {
            pos_ms = pos_ms.min(dur_ms);
        }
        active.position_ms = pos_ms;
        active.has_live_sample = true;
        active.resume_target_ms = None;
        active.resume_seek_pending = false;
        active.explicit_seek_pending = false;
        if active.is_live
            && let Some(dur_ms) = active.duration_ms.filter(|&d| d > 0)
        {
            let ratio = pos_ms as f64 / dur_ms as f64;
            if ratio >= 0.985 {
                active.at_live_edge = true;
                active.live_behind_secs = 0.0;
            } else {
                active.at_live_edge = false;
                active.live_behind_secs = (dur_ms.saturating_sub(pos_ms) as f64) / 1000.0;
            }
            active.live_paused_accumulated_secs = 0.0;
        }
        self.desktop.notify_seeked(pos_ms as f64 / 1000.0);
        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, true) {
            self.save_history_to_disk(now);
        }
        self.maybe_send_remote_watchtime_heartbeat(pos_ms as f64 / 1000.0, false, true, false);
    }

    pub(crate) fn apply_buffer_profile(&mut self, profile: BufferProfile) {
        let pending_restart = self
            .playback
            .active_buffer
            .is_some_and(|active| active.values() != profile.values());
        if self.settings.apply_buffer_profile(profile, pending_restart) {
            self.invalidate_feed_cache();
        }
    }

    pub(crate) fn has_pending_buffer_restart(&self) -> bool {
        self.playback
            .active_buffer
            .is_some_and(|active| active.values() != self.settings.buffer.profile.values())
    }

    pub(crate) fn reset_decoding_diagnostics(&mut self) {
        self.playback.diagnostics =
            DecodingDiagnostics::unknown(Some(self.settings.prefs.value.hwdec_mode.as_mpv_value()));
        self.playback.last_diagnostics_poll_at = None;
    }

    #[cfg(test)]
    pub(crate) fn refresh_decoding_diagnostics_with<F>(
        &mut self,
        now: Instant,
        has_active_video: bool,
        read_property: F,
    ) -> bool
    where
        F: FnMut(&str) -> Option<String>,
    {
        update_decoding_diagnostics(
            &mut self.playback.diagnostics,
            &mut self.playback.last_diagnostics_poll_at,
            self.ui.settings_open,
            has_active_video,
            now,
            read_property,
        )
    }

    pub(crate) fn refresh_player_diagnostics(&mut self, now: Instant) -> bool {
        if self.selected.is_none() {
            self.reset_decoding_diagnostics();
            return false;
        }
        #[cfg(windows)]
        if let Some(player) = &self.player {
            return update_decoding_diagnostics(
                &mut self.playback.diagnostics,
                &mut self.playback.last_diagnostics_poll_at,
                self.ui.settings_open,
                true,
                now,
                |name| player.property(name),
            );
        }
        let _ = now;
        false
    }

    pub(crate) fn should_show_video_window(&self) -> bool {
        if self.desktop.hidden_to_tray {
            return false;
        }
        #[cfg(windows)]
        if self
            .player
            .as_ref()
            .is_some_and(player::Player::is_render_api)
        {
            return self.selected.is_some();
        }
        if self.is_in_app_mini_player_active()
            && (self.has_modal_covering_mini_player() || self.ui.navigation_open)
        {
            return false;
        }
        !self.playback.audio_only
            && should_show_video_window(self.selected.is_some(), self.ui.settings_open)
    }

    pub(crate) fn sync_player_window_visibility(&mut self) {
        let visible = self.should_show_video_window();
        self.playback.window_visible = visible;
        #[cfg(windows)]
        if let Some(player) = &mut self.player {
            player.set_visible(visible);
        }
    }

    pub(crate) fn seek_to_seconds(&mut self, secs: f64) {
        let is_live = self.active_video_is_live();
        let live_dvr = self
            .playback
            .active
            .as_ref()
            .is_some_and(|active| active.live_dvr);
        if is_live && !live_dvr {
            return;
        }
        let target = secs.max(0.0);
        #[cfg(windows)]
        if let Some(player) = self.player.as_ref() {
            let duration = player.number("duration").filter(|&dur| dur > 0.0);
            if is_live
                && let Some(dur) = duration
                && (dur <= 1.5 || target >= (dur - 1.5).max(0.0))
                && let Some(video) = self.selected.clone()
            {
                self.go_live_edge(&video);
                return;
            }
            let clamped = if is_live {
                duration.map_or(target, |dur| target.min((dur - 1.5).max(0.0)))
            } else {
                target
            };
            let _ = player.seek(clamped, "absolute+exact");
            self.record_manual_seek(clamped, duration, Instant::now());
        } else {
            self.record_manual_seek(target, None, Instant::now());
        }
        #[cfg(not(windows))]
        {
            self.record_manual_seek(target, None, Instant::now());
        }
    }

    pub(crate) fn next_recommended_video(&mut self) -> Option<Video> {
        self.ensure_feed_cache();
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str());
        for ranked in &self.feed.cache.ranked {
            let v = &ranked.video;
            if Some(v.url.as_str()) != selected_url
                && !v.url.starts_with("fasttube-google-playlist:")
                && !self.feed.home.playlist_urls.contains(&v.url)
            {
                return Some(v.clone());
            }
        }
        for v in &self.feed.videos {
            if Some(v.url.as_str()) != selected_url
                && !v.url.starts_with("fasttube-google-playlist:")
                && !self.feed.home.playlist_urls.contains(&v.url)
            {
                return Some(v.clone());
            }
        }
        None
    }

    pub(crate) fn shorts_feed_candidates(&self) -> Vec<&Video> {
        self.feed
            .videos
            .iter()
            .filter(|v| {
                recommendations::is_short_video(v, self.metadata_for(&v.url))
                    || v.url.contains("/shorts/")
            })
            .collect()
    }

    pub(crate) fn short_index_and_total(&self) -> Option<(usize, usize)> {
        let shorts = self.shorts_feed_candidates();
        if shorts.is_empty() {
            return None;
        }
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str())?;
        let idx = shorts.iter().position(|v| v.url.as_str() == selected_url)?;
        Some((idx + 1, shorts.len()))
    }

    pub(crate) fn next_or_prev_short(&self, forward: bool) -> Option<Video> {
        let shorts = self.shorts_feed_candidates();
        if shorts.is_empty() {
            return None;
        }
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str());
        if let Some(idx) = shorts
            .iter()
            .position(|v| Some(v.url.as_str()) == selected_url)
        {
            let next_idx = if forward {
                idx.checked_add(1)?
            } else {
                idx.checked_sub(1)?
            };
            shorts.get(next_idx).map(|v| (*v).clone())
        } else if forward {
            shorts.first().map(|v| (*v).clone())
        } else {
            None
        }
    }

    pub(crate) fn enqueue_video(&mut self, video: Video) -> Result<(), Video> {
        let title = video.title.clone();
        match self.queue.data.add(video) {
            Ok(()) => {
                self.set_pending_queue_warning();
                self.queue.last_rejected_video = None;
                self.queue.notice = Some(format!(
                    "Adicionado à fila ({}/{}): {title}",
                    self.queue.data.len(),
                    self.queue.data.limit()
                ));
                self.toasts.success(format!("Adicionado à fila: {title}"));
                self.persist_queue();
                self.maybe_prefetch_next_video();
                Ok(())
            }
            Err(rejected) => {
                self.queue.notice = None;
                self.queue.error = Some(format!(
                    "Fila cheia ({}/{}): '{}' foi rejeitado sem perda.",
                    self.queue.data.len(),
                    self.queue.data.limit(),
                    rejected.title
                ));
                self.toasts.warning(format!(
                    "Fila cheia ({}/{})",
                    self.queue.data.len(),
                    self.queue.data.limit()
                ));
                self.queue.last_rejected_video = Some(rejected.clone());
                Err(rejected)
            }
        }
    }

    pub(crate) fn remove_from_queue(&mut self, index: usize) -> Option<Video> {
        let removed_entry_id = self.queue.data.entry_id(index);
        match self.queue.data.remove(index) {
            Some(removed) => {
                self.invalidate_feed_cache();
                let pending_id_matches = removed_entry_id
                    .zip(self.queue.pending_entry_id)
                    .is_some_and(|(removed, pending)| removed == pending);
                let legacy_video_matches = self.queue.pending_entry_id.is_none()
                    && self.queue.pending_video.as_ref() == Some(&removed);
                if pending_id_matches || legacy_video_matches {
                    self.cancel_pending_queue_video();
                }
                self.set_pending_queue_warning();
                self.queue.notice = Some(format!(
                    "Removido da fila ({}/{}): {}",
                    self.queue.data.len(),
                    self.queue.data.limit(),
                    removed.title
                ));
                self.persist_queue();
                Some(removed)
            }
            None => {
                self.queue.error = Some(format!("Índice inválido para remoção na fila: {index}."));
                None
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn reorder_queue(&mut self, from: usize, to: usize) -> bool {
        if self.queue.data.reorder(from, to) {
            self.set_pending_queue_warning();
            self.queue.notice = Some(format!("Fila reordenada ({} → {}).", from + 1, to + 1));
            self.persist_queue();
            true
        } else {
            self.queue.error = Some(format!(
                "Não foi possível reordenar a fila ({from} → {to})."
            ));
            false
        }
    }

    pub(crate) fn advance_queue_at(&mut self, now: Instant) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error =
                Some("Aguarde a confirma\u{00e7}\u{00e3}o do item atual antes de avan\u{00e7}ar novamente.".into());
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
            }
        }
        let Some((next_video, next_entry_id)) =
            self.queue.data.first_entry_id().and_then(|entry_id| {
                self.queue
                    .data
                    .videos()
                    .first()
                    .cloned()
                    .map(|video| (video, entry_id))
            })
        else {
            self.queue.error = Some("A fila está vazia; nenhum próximo vídeo para avançar.".into());
            return None;
        };
        let previous_player_pos_ms = self.current_player_live_pos_ms();
        #[cfg(windows)]
        self.sample_player_before_transition(now);
        self.flush_remote_watch_tracking();
        self.flush_playback_history_at(now);
        let resume_target_ms = self.planned_resume_ms(&next_video);
        self.playback.quality = None;
        self.playback.qualities.clear();
        self.playback.quality_task.cancel();
        self.playback.quality_error = None;
        self.playback.quality_fallback_url = None;
        self.playback.current_file_loaded = false;
        self.playback.cached_raw_path = None;
        self.playback.cached_raw_path_poll = None;
        self.playback.seek_drag = None;
        self.reset_decoding_diagnostics();
        self.selected_metadata = self.metadata_for(&next_video.url).cloned();
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.watch.overlay_cache = app_state::OverlayCache::default();
        self.playback.last_video_size_url = None;
        self.playback.last_video_size = None;
        self.activate_video_playback(&next_video, resume_target_ms, previous_player_pos_ms, now);
        self.watch.live.reset_for_video(
            Some(&next_video.url),
            history::youtube_video_id(&next_video.url),
        );
        if let Some(prev) = self.selected.clone() {
            self.record_video_back_stack(prev, &next_video.url);
        }
        self.playback.mini_player_paused = false;
        self.selected = Some(next_video.clone());
        self.invalidate_feed_cache();
        self.queue.pending_video = Some(next_video.clone());
        self.queue.pending_entry_id = Some(next_entry_id);
        self.queue.correlation.begin();
        self.sync_player_window_visibility();
        self.set_pending_queue_warning();
        Some(next_video)
    }

    pub(crate) fn advance_queue_with_frame(
        &mut self,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error =
                Some("Aguarde a confirma\u{00e7}\u{00e3}o do item atual antes de avan\u{00e7}ar novamente.".into());
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
                self.report_pending_queue_load_error();
            }
        }
        let Some((candidate, entry_id)) = self.queue.data.first_entry_id().and_then(|entry_id| {
            self.queue
                .data
                .videos()
                .first()
                .cloned()
                .map(|video| (video, entry_id))
        }) else {
            self.queue.error = Some("A fila está vazia; nenhum próximo vídeo para avançar.".into());
            return None;
        };
        self.playback.pending_open_queue_entry_id = Some(entry_id);
        if !self.open_video(candidate.clone(), frame, ctx) {
            self.playback.pending_open_queue_entry_id = None;
            self.queue.error = Some(
                "Não foi possível iniciar o próximo vídeo; o item foi mantido na fila.".into(),
            );
            return None;
        }
        self.queue.pending_video = Some(candidate.clone());
        self.queue.pending_entry_id = Some(entry_id);
        self.queue.correlation.begin();
        self.set_pending_queue_warning();
        self.queue.notice = Some(format!(
            "Abrindo da fila ({} itens; aguardando o início): {}",
            self.queue.data.len(),
            candidate.title
        ));
        Some(candidate)
    }

    pub(crate) fn request_play_queue_entry(&mut self, entry_id: u64) {
        if self.in_app_ui_pass {
            self.queue.pending_play_entry_id = Some(entry_id);
        } else {
            let _ = self.play_queue_entry_at(entry_id, Instant::now());
        }
    }

    pub(crate) fn play_queue_entry_at(&mut self, entry_id: u64, now: Instant) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error = Some(
                "Aguarde a confirmação do item atual antes de abrir outro item da fila.".into(),
            );
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
            }
        }
        let Some(index) = self.queue.data.index_of_entry(entry_id) else {
            self.queue.error = Some("Este item não está mais na fila.".into());
            return None;
        };
        if index > 0 {
            let _ = self.queue.data.move_to_top(index);
            self.persist_queue();
        }
        let next_video = self.queue.data.videos().first().cloned()?;
        let previous_player_pos_ms = self.current_player_live_pos_ms();
        #[cfg(windows)]
        self.sample_player_before_transition(now);
        self.flush_remote_watch_tracking();
        self.flush_playback_history_at(now);
        let resume_target_ms = self.planned_resume_ms(&next_video);
        self.playback.quality = None;
        self.playback.qualities.clear();
        self.playback.quality_task.cancel();
        self.playback.quality_error = None;
        self.playback.quality_fallback_url = None;
        self.playback.current_file_loaded = false;
        self.playback.cached_raw_path = None;
        self.playback.cached_raw_path_poll = None;
        self.playback.seek_drag = None;
        self.reset_decoding_diagnostics();
        self.selected_metadata = self.metadata_for(&next_video.url).cloned();
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.watch.overlay_cache = app_state::OverlayCache::default();
        self.playback.last_video_size_url = None;
        self.playback.last_video_size = None;
        self.activate_video_playback(&next_video, resume_target_ms, previous_player_pos_ms, now);
        self.watch.live.reset_for_video(
            Some(&next_video.url),
            history::youtube_video_id(&next_video.url),
        );
        if let Some(prev) = self.selected.clone() {
            self.record_video_back_stack(prev, &next_video.url);
        }
        self.playback.mini_player_paused = false;
        self.selected = Some(next_video.clone());
        self.invalidate_feed_cache();
        self.queue.pending_video = Some(next_video.clone());
        self.queue.pending_entry_id = Some(entry_id);
        self.queue.correlation.begin();
        self.sync_player_window_visibility();
        self.set_pending_queue_warning();
        Some(next_video)
    }

    pub(crate) fn play_queue_entry_with_frame(
        &mut self,
        entry_id: u64,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if self.queue.pending_video.is_some() {
            self.queue.error = Some(
                "Aguarde a confirmação do item atual antes de abrir outro item da fila.".into(),
            );
            return None;
        }
        if let Some(player) = &self.player {
            let (events, error) = player.poll_events();
            for event in events {
                self.handle_queue_player_event(event);
            }
            if let Some(error) = error {
                self.message = error;
                self.report_pending_queue_load_error();
            }
        }
        let Some(index) = self.queue.data.index_of_entry(entry_id) else {
            self.queue.error = Some("Este item não está mais na fila.".into());
            return None;
        };
        if index > 0 {
            let _ = self.queue.data.move_to_top(index);
            self.persist_queue();
        }
        let candidate = self.queue.data.videos().first().cloned()?;
        self.playback.pending_open_queue_entry_id = Some(entry_id);
        if !self.open_video(candidate.clone(), frame, ctx) {
            self.playback.pending_open_queue_entry_id = None;
            self.queue.error = Some(
                "Não foi possível iniciar o vídeo selecionado; o item foi mantido na fila.".into(),
            );
            return None;
        }
        self.queue.pending_video = Some(candidate.clone());
        self.queue.pending_entry_id = Some(entry_id);
        self.queue.correlation.begin();
        self.set_pending_queue_warning();
        self.queue.notice = Some(format!(
            "Abrindo da fila ({} itens; aguardando o início): {}",
            self.queue.data.len(),
            candidate.title
        ));
        Some(candidate)
    }

    pub(crate) fn move_queue_item_to_top(&mut self, index: usize) -> bool {
        if self.queue.data.move_to_top(index) {
            self.set_pending_queue_warning();
            self.queue.notice = Some("Item movido para tocar em seguida.".into());
            self.persist_queue();
            self.maybe_prefetch_next_video();
            true
        } else {
            false
        }
    }

    pub(crate) fn shuffle_queue(&mut self) {
        if self.queue.data.len() <= 1 {
            return;
        }
        self.queue.data.shuffle();
        self.set_pending_queue_warning();
        self.queue.notice = Some(self.pick("Fila embaralhada.", "Queue shuffled.").into());
        self.persist_queue();
        self.maybe_prefetch_next_video();
    }

    pub(crate) fn cycle_repeat_mode(&mut self) {
        self.queue.repeat_mode = self.queue.repeat_mode.cycle();
        let label = if self.ui_pt() {
            self.queue.repeat_mode.label_pt()
        } else {
            self.queue.repeat_mode.label_en()
        };
        let msg = format!("{}: {label}", self.pick("Modo de repetição", "Repeat mode"));
        self.queue.notice = Some(msg.clone());
        self.toasts.info(msg);
    }

    pub(crate) fn deduplicate_queue(&mut self) -> usize {
        let removed = self.queue.data.deduplicate();
        if let Some(pending_id) = self.queue.pending_entry_id
            && self.queue.data.index_of_entry(pending_id).is_none()
        {
            self.cancel_pending_queue_video();
        }
        self.set_pending_queue_warning();
        if removed > 0 {
            self.invalidate_feed_cache();
            self.queue.notice = Some(format!("{removed} duplicado(s) removido(s) da fila."));
            self.persist_queue();
        } else {
            self.queue.notice = Some("Nenhum item duplicado encontrado na fila.".into());
        }
        removed
    }

    pub(crate) fn clear_queue(&mut self) {
        self.cancel_pending_queue_video();
        self.queue.pending_advance = false;
        self.queue.pending_play_entry_id = None;
        if self.queue.data.is_empty() {
            self.set_pending_queue_warning();
            self.queue.error = None;
            self.queue.notice = Some("A fila j\u{00e1} est\u{00e1} vazia.".into());
            return;
        }
        self.queue.data.clear();
        self.invalidate_feed_cache();
        self.set_pending_queue_warning();
        self.queue.error = None;
        self.queue.last_rejected_video = None;
        self.queue.notice = Some("Fila de reprodução limpa.".into());
        self.persist_queue();
    }

    pub(crate) fn save_queue_as_collection(
        &mut self,
        requested_name: &str,
    ) -> Result<String, &'static str> {
        let resolved_name = if requested_name.trim().is_empty() {
            self.library
                .data
                .next_available_collection_name("Fila salva")
        } else {
            requested_name.trim().to_owned()
        };
        let (col_idx, added) = match self
            .library
            .data
            .create_collection_with_videos(&resolved_name, self.queue.data.videos())
        {
            Ok(res) => res,
            Err(err) => {
                self.queue.error = Some(err.to_owned());
                return Err(err);
            }
        };
        let created = self.library.data.collections[col_idx].name.clone();
        self.queue.save_collection_name.clear();
        self.save_library();
        self.queue.error = None;
        self.queue.notice = Some(format!(
            "Fila salva como coleção '{created}' ({added} vídeo(s))."
        ));
        Ok(created)
    }

    pub(crate) fn enqueue_all_videos(&mut self, videos: &[Video]) -> (usize, usize) {
        if videos.is_empty() {
            return (0, 0);
        }
        let mut added = 0usize;
        let mut rejected = 0usize;
        let mut first_rejected: Option<Video> = None;
        for video in videos {
            match self.queue.data.add(video.clone()) {
                Ok(()) => added += 1,
                Err(rej) => {
                    rejected += 1;
                    if first_rejected.is_none() {
                        first_rejected = Some(rej);
                    }
                }
            }
        }
        if added > 0 {
            self.invalidate_feed_cache();
            self.persist_queue();
        }
        if rejected == 0 {
            self.queue.error = None;
            self.queue.last_rejected_video = None;
            self.queue.notice = Some(format!(
                "{added} vídeo(s) adicionado(s) à fila ({}/{}).",
                self.queue.data.len(),
                self.queue.data.limit()
            ));
        } else {
            self.queue.last_rejected_video = first_rejected;
            self.queue.error = Some(format!(
                "Fila cheia ({}/{}): {added} adicionado(s) e {rejected} não coube(ram).",
                self.queue.data.len(),
                self.queue.data.limit()
            ));
        }
        (added, rejected)
    }

    pub(crate) fn play_all_videos(&mut self, videos: &[Video]) -> Option<Video> {
        let first = videos.first()?.clone();
        if videos.len() > 1 {
            let (added, rejected) = self.enqueue_all_videos(&videos[1..]);
            if rejected == 0 {
                self.library.notice = Some(format!(
                    "Reproduzindo '{}' e {added} item(ns) na fila.",
                    first.title
                ));
            } else {
                self.library.notice = Some(format!(
                    "Reproduzindo '{}': {added} na fila e {rejected} fora do limite.",
                    first.title
                ));
            }
        } else {
            self.library.notice = None;
        }
        Some(first)
    }

    pub(crate) fn report_pending_queue_playback_observed(&mut self) {
        self.queue.notice = Some(
            "Reprodu\u{00e7}\u{00e3}o observada; aguardando confirma\u{00e7}\u{00e3}o do libmpv."
                .into(),
        );
    }

    pub(crate) fn set_pending_queue_warning(&mut self) {
        self.queue.error = None;
    }

    pub(crate) fn report_pending_queue_load_error(&mut self) {
        if self.queue.pending_video.is_some() {
            self.queue.error = Some(
                "O libmpv relatou um erro durante a abertura; o item permanece na fila. Confira o player e remova-o manualmente pelo botão × se desejar.".into(),
            );
        }
    }

    pub(crate) fn cancel_pending_queue_video(&mut self) {
        self.queue.pending_video = None;
        self.queue.pending_entry_id = None;
        self.queue.correlation.cancel();
    }

    pub(crate) fn handle_queue_player_event(&mut self, event: PlaybackEvent) {
        match event {
            PlaybackEvent::StartFile { .. } => {
                self.playback.cached_raw_path_poll = None;
                self.watch.overlay_cache.chapters_last_poll = None;
                self.watch.overlay_cache.mpv_subs_last_poll = None;
                self.watch.overlay_cache.force_refresh = true;
            }
            PlaybackEvent::FileLoaded => {
                self.playback.current_file_loaded = true;
                self.playback.cached_raw_path_poll = None;
                self.watch.overlay_cache.chapters_last_poll = None;
                self.watch.overlay_cache.mpv_subs_last_poll = None;
                self.watch.overlay_cache.force_refresh = true;
            }
            PlaybackEvent::EndFile { failed: true, .. } => {
                self.handle_direct_stream_failure_fallback();
            }
            PlaybackEvent::EndFile { failed: false, .. } => {}
        }
        match self.queue.correlation.handle(event) {
            QueueLoadResult::Confirmed { .. } => {
                let entry_id = self.queue.pending_entry_id.take();
                if let Some(candidate) = self.queue.pending_video.take() {
                    let removed = match entry_id {
                        Some(entry_id) => self.queue.data.remove_entry(entry_id),
                        None => self
                            .queue
                            .data
                            .videos()
                            .iter()
                            .position(|video| *video == candidate)
                            .and_then(|index| self.queue.data.remove(index)),
                    };
                    if removed.is_some() {
                        self.invalidate_feed_cache();
                        self.persist_queue();
                        self.maybe_prefetch_next_video();
                        self.queue.error = None;
                        self.queue.notice = Some(format!(
                            "Reproduzindo da fila ({} restantes): {}",
                            self.queue.data.len(),
                            candidate.title
                        ));
                    } else {
                        self.queue.error = Some("O item confirmado nao esta mais na fila; nenhum outro item foi removido.".into());
                    }
                }
            }
            QueueLoadResult::Failed { .. } => {
                self.queue.pending_video = None;
                self.queue.pending_entry_id = None;
                self.queue.error =
                    Some("O libmpv falhou ao carregar o item; ele continua na fila.".into());
            }
            QueueLoadResult::None => {}
        }
    }

    pub(crate) fn queue_auto_advance_ready(&self, ended: bool) -> bool {
        ended
            && self.queue.pending_video.is_none()
            && !self.queue.data.is_empty()
            && self.selected.is_some()
            && self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.has_live_sample)
    }

    pub(crate) fn request_queue_advance(&mut self) {
        if self.in_app_ui_pass {
            self.queue.pending_advance = true;
        } else {
            let _ = self.advance_queue_at(Instant::now());
        }
    }

    pub(crate) fn poll_quality(&mut self) {
        match self.playback.quality_task.poll_event() {
            async_task::AsyncTaskPoll::Ready(Ok(meta)) => {
                self.playback.qualities = meta.heights;
                self.playback.storyboard = meta.storyboard.clone();
                self.playback.storyboard_video_url = self.selected.as_ref().map(|v| v.url.clone());
                self.watch.extras.apply_preloaded_subtitles(meta.subtitles);
                self.watch.overlay_cache.mpv_subs_last_poll = None;
                self.watch.overlay_cache.force_refresh = true;
                if let Some(selected_url) = self.selected.as_ref().map(|v| v.url.clone()) {
                    if meta.is_live
                        || meta.title.is_some()
                        || meta.channel.is_some()
                        || meta.channel_id.is_some()
                        || meta.channel_url.is_some()
                        || meta.description.is_some()
                        || meta.concurrent_view_count.is_some()
                    {
                        self.apply_live_metadata_to_video(
                            &selected_url,
                            meta.is_live,
                            None,
                            meta.title.as_deref(),
                            meta.channel.as_deref(),
                            meta.channel_id.as_deref(),
                            meta.channel_url.as_deref(),
                            None,
                            None,
                            meta.description.as_deref(),
                            meta.concurrent_view_count,
                            None,
                        );
                    }
                    self.mark_followed_video_seen(meta.channel_id.as_deref(), &selected_url);
                }
            }
            async_task::AsyncTaskPoll::Ready(Err(error)) => {
                self.playback.quality_error = Some(error);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.playback.quality_error =
                    Some("A consulta de resoluções foi interrompida.".into());
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_sponsorblock_fetch(&mut self, video_url: &str, ctx: &egui::Context) {
        let video_id = history::youtube_video_id(video_url);
        self.watch.sponsorblock.reset_for_video(video_id.clone());
        if !self.settings.prefs.value.sponsorblock.enabled {
            return;
        }
        let Some(video_id) = video_id else {
            return;
        };
        let categories = self
            .settings
            .prefs
            .value
            .sponsorblock
            .categories
            .enabled_categories();
        if categories.is_empty() {
            self.watch.sponsorblock.loaded = true;
            return;
        }
        self.watch.sponsorblock.next_request_id = self
            .watch
            .sponsorblock
            .next_request_id
            .wrapping_add(1)
            .max(1);
        let request_id = self.watch.sponsorblock.next_request_id;
        self.watch.sponsorblock.active_request_id = Some(request_id);
        let cancel = process::Cancellation::default();
        self.watch.sponsorblock.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.watch.sponsorblock.rx = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = sponsorblock::fetch_segments_with_cancel(&video_id, &categories, &cancel);
            let _ = sender.send(sponsorblock::SponsorBlockFetchMessage {
                request_id,
                video_id,
                result,
            });
            ctx.request_repaint();
        });
    }

    pub(crate) fn poll_sponsorblock(&mut self) {
        let mut disconnect = false;
        let mut received = None;
        if let Some(receiver) = &self.watch.sponsorblock.rx {
            match receiver.try_recv() {
                Ok(msg) => received = Some(msg),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => disconnect = true,
            }
        }
        if let Some(msg) = received {
            let matches_active = self.watch.sponsorblock.active_request_id == Some(msg.request_id)
                && self.watch.sponsorblock.video_id.as_deref() == Some(msg.video_id.as_str());
            self.watch.sponsorblock.rx = None;
            self.watch.sponsorblock.cancel = None;
            self.watch.sponsorblock.active_request_id = None;
            if matches_active {
                match msg.result {
                    Ok(segments) => {
                        self.watch.sponsorblock.segments = segments;
                        self.watch.sponsorblock.error = None;
                    }
                    Err(error) => {
                        self.watch.sponsorblock.segments.clear();
                        self.watch.sponsorblock.error = Some(error);
                    }
                }
                self.watch.sponsorblock.loaded = true;
            }
        } else if disconnect {
            self.watch.sponsorblock.rx = None;
            self.watch.sponsorblock.cancel = None;
            self.watch.sponsorblock.active_request_id = None;
        }
    }

    pub(crate) fn try_auto_skip_sponsor_segment(
        &mut self,
        pos_secs: f64,
        duration_secs: Option<f64>,
        is_live: bool,
        now: Instant,
    ) -> Option<f64> {
        if is_live || self.playback.seek_drag.is_some() {
            return None;
        }
        let pt = self.ui_pt();
        self.watch.sponsorblock.apply_auto_skip(
            pos_secs,
            duration_secs,
            &self.settings.prefs.value.sponsorblock,
            pt,
            now,
        )
    }

    pub(crate) fn record_manual_sponsor_skip(
        &mut self,
        pos_secs: f64,
        target_secs: f64,
        now: Instant,
    ) {
        let matched = self
            .watch
            .sponsorblock
            .segment_at(pos_secs, &self.settings.prefs.value.sponsorblock)
            .cloned()
            .or_else(|| {
                self.watch
                    .sponsorblock
                    .filtered_segments(&self.settings.prefs.value.sponsorblock)
                    .find(|seg| (seg.end_secs - target_secs).abs() < 0.25)
                    .cloned()
            });
        if let Some(seg) = matched {
            self.watch.sponsorblock.auto_skipped_uuids.insert(seg.uuid);
            if self.settings.prefs.value.sponsorblock.show_skip_toast {
                self.watch.sponsorblock.last_skip_notice =
                    Some((format!("Pulado: {}", seg.category.label_pt()), now));
            }
        }
    }

    pub(crate) fn resolved_initial_playback_speed(&self, channel: &str) -> f64 {
        let default_speed = f64::from(self.settings.playback.value.speed_quarters) / 4.0;
        if self.settings.prefs.value.remember_speed_per_channel
            && let Some(saved) = settings::get_channel_playback_speed(
                &self.settings.prefs.value.per_channel_speeds,
                channel,
            )
        {
            saved
        } else {
            default_speed
        }
    }

    pub(crate) fn apply_and_remember_playback_speed(&mut self, speed: f64) {
        let clamped = speed.clamp(0.25, 4.0);
        self.set_player_speed(clamped);
        if self.settings.prefs.value.remember_speed_per_channel
            && let Some(channel) = self
                .selected
                .as_ref()
                .map(|video| video.channel.trim())
                .filter(|ch| !ch.is_empty())
                .map(str::to_owned)
        {
            let mut next = self.settings.prefs.value.clone();
            settings::set_channel_playback_speed(&mut next.per_channel_speeds, &channel, clamped);
            if next.per_channel_speeds != self.settings.prefs.value.per_channel_speeds {
                self.settings.apply_app_preferences(next);
            }
        }
    }

    pub(crate) fn try_auto_skip_keyword_chapter(
        &mut self,
        pos_secs: f64,
        duration_secs: Option<f64>,
    ) -> Option<f64> {
        if !self.settings.prefs.value.auto_skip_chapters_enabled
            || self
                .settings
                .prefs
                .value
                .auto_skip_chapter_keywords
                .is_empty()
            || self.playback.seek_drag.is_some()
            || !pos_secs.is_finite()
            || pos_secs < 0.0
        {
            return None;
        }
        let chapters: Vec<(f64, String)> = if !self.watch.overlay_cache.chapters.is_empty() {
            self.watch
                .overlay_cache
                .chapters
                .iter()
                .map(|c| (c.time_secs, c.title.clone()))
                .collect()
        } else if !self.watch.metadata_cache.description_chapters.is_empty() {
            self.watch
                .metadata_cache
                .description_chapters
                .iter()
                .map(|c| (c.seconds as f64, c.title.clone()))
                .collect()
        } else {
            return None;
        };
        let active_idx = chapters
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (start, _))| start.is_finite() && *start <= pos_secs + 0.05)
            .map(|(idx, _)| idx)?;
        if self
            .watch
            .auto_skipped_chapter_indices
            .contains(&active_idx)
        {
            return None;
        }
        let (_, chapter_title) = &chapters[active_idx];
        if !settings::should_auto_skip_chapter(
            chapter_title,
            &self.settings.prefs.value.auto_skip_chapter_keywords,
        ) {
            return None;
        }
        let target = if let Some((next_start, _)) = chapters.get(active_idx + 1)
            && next_start.is_finite()
            && *next_start > pos_secs
        {
            *next_start
        } else {
            let dur = duration_secs.filter(|d| d.is_finite() && *d > 0.0)?;
            (dur - 0.5).max(pos_secs)
        };
        let title_owned = chapter_title.clone();
        self.watch.auto_skipped_chapter_indices.insert(active_idx);
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let _ = player.seek(target, "absolute+exact");
        }
        let msg = if self.ui_pt() {
            format!("Capítulo pulado: {title_owned}")
        } else {
            format!("Skipped chapter: {title_owned}")
        };
        self.toasts.info(msg);
        Some(target)
    }

    pub(crate) fn open_video(
        &mut self,
        video: Video,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) -> bool {
        let keep_docked = std::mem::take(&mut self.playback.pending_open_keep_mini_player);
        let suppress_back_stack =
            std::mem::take(&mut self.playback.pending_open_suppress_back_stack);
        let keep_playlist = std::mem::take(&mut self.playback.pending_open_keep_playlist);
        let pending_queue_entry_id = self.playback.pending_open_queue_entry_id.take();
        self.playback.manifest_task.cancel();
        self.playback.current_manifest = None;
        self.playback.active_direct_stream_url = None;
        self.playback.direct_stream_fallback_attempted = false;
        self.watch.next_task.cancel();
        self.watch.innertube_comments_task.cancel();
        self.watch.auto_skipped_chapter_indices.clear();
        let incoming_meta = self.metadata_for(&video.url);
        let incoming_is_short = video.url.contains("/shorts/")
            || incoming_meta.is_some_and(|m| m.kind == search::SearchResultKind::Short)
            || ((matches!(self.feed.filter, recommendations::FeedFilter::Shorts)
                || self.ui.browse_section == BrowseSection::Shorts)
                && recommendations::is_short_video(&video, incoming_meta));
        if !incoming_is_short {
            self.watch.shorts_side_panel = app_state::ShortsSidePanel::None;
        }
        if !keep_playlist {
            self.watch.active_playlist = None;
            if let Some(cancel) = self.watch.mix_cancel.take() {
                cancel.cancel();
            }
            self.watch.mix_rx = None;
            self.watch.mix_active_request_id = None;
        }
        self.cancel_pending_queue_video();
        if !keep_docked {
            self.playback.in_app_mini_player = false;
            self.ui.settings_open = false;
            self.close_channel_view();
        }
        self.playback.autoplay_eof_handled = false;
        self.playback.auto_subtitle_attempted = false;
        self.playback.save_menu_open = false;
        self.playback.pending_open_video = None;
        let explicit_seek_ms = self.playback.pending_open_seek_ms.take();
        #[cfg(windows)]
        {
            let now = Instant::now();
            let previous_player_pos_ms = self.current_player_live_pos_ms();
            self.sample_player_before_transition(now);
            self.flush_remote_watch_tracking();
            self.flush_playback_history_at(now);
            self.ensure_playback_settings_synchronized();
            let bookmark_seek_ms =
                self.library
                    .pending_bookmark_seek
                    .take()
                    .and_then(|(target_id, time_ms)| {
                        (history::youtube_video_id(&video.url).as_deref()
                            == Some(target_id.as_str()))
                        .then_some(time_ms)
                    });
            let resume_target_ms = explicit_seek_ms
                .or(bookmark_seek_ms)
                .or_else(|| self.planned_resume_ms(&video));

            let mut player_error: Option<String> = None;
            let profile = self.settings.buffer.profile;
            let hwdec_mode = self.settings.prefs.value.hwdec_mode;
            let codec_pref = self.settings.prefs.value.video_codec_preference;
            let config_changed = self.player.is_some()
                && (self.playback.active_buffer != Some(profile)
                    || self.playback.active_hwdec != Some(hwdec_mode));
            if config_changed {
                if let Some(player) = &mut self.player {
                    player.set_visible(false);
                }
                self.player = None;
                self.playback.active_buffer = None;
                self.playback.active_hwdec = None;
            }
            let player_created = self.player.is_none();
            if player_created {
                match player::Player::new_with_codec_pref(
                    frame,
                    profile.values(),
                    hwdec_mode,
                    codec_pref,
                    ctx,
                ) {
                    Ok(player) => {
                        self.player = Some(player);
                        self.playback.active_buffer = Some(profile);
                        self.playback.active_hwdec = Some(hwdec_mode);
                    }
                    Err(error) => {
                        player_error = Some(error);
                    }
                }
            } else if let Some(player) = &self.player {
                player.wake_from_park();
            }
            self.playback.autoplay = self.settings.prefs.value.default_autoplay;
            self.playback.loop_video = incoming_is_short || self.settings.prefs.value.default_loop;
            self.playback.clear_ab_loop();
            let initial_speed = self.resolved_initial_playback_speed(&video.channel);
            self.playback.speed = initial_speed;
            let is_local_file = Self::is_local_offline_media_path(&video.url);
            let is_live_initial = !is_local_file && self.is_video_known_live(&video);
            let yt_video_id = if is_local_file {
                None
            } else {
                history::youtube_video_id(&video.url)
            };
            let cached_manifest = yt_video_id
                .as_deref()
                .filter(|_| !is_live_initial)
                .and_then(|id| self.playback.stream_manifest_cache.get(id))
                .filter(|m| !m.is_live && !m.is_expired());
            let mut started_direct_manifest: Option<
                std::sync::Arc<stream_manifest::StreamManifest>,
            > = None;
            let mut defer_to_manifest_extract = false;
            if let Some(player) = &mut self.player {
                let ytdl_format = player::ytdl_format_for_codec_pref(
                    self.settings.playback.value.max_height,
                    codec_pref,
                );
                let _ = player.set("options/ytdl-format", &ytdl_format);
                let _ = player.set("speed", &format!("{initial_speed}"));
                let _ = player.set_audio_only(self.playback.audio_only);
                if player_created || !self.settings.prefs.value.remember_volume {
                    let volume = self.settings.prefs.value.default_volume_percent.min(200);
                    let _ = player.set_volume(f64::from(volume));
                    self.playback.mini_player_volume = f32::from(volume);
                } else if let Some(vol) = player.double_cstr(c"volume") {
                    self.playback.mini_player_volume = vol.clamp(0.0, 200.0) as f32;
                }
                let _ = player.apply_audio_filters(
                    self.settings.prefs.value.audio_normalization,
                    self.playback.audio_eq,
                );
                let _ = player.apply_caption_style(&self.settings.prefs.value.caption_style);
                let _ = player.set_ultrawide_zoom(
                    self.settings.prefs.value.ultrawide_zoom_mode,
                    self.settings.prefs.value.ultrawide_custom_zoom,
                );
                let _ = player.set_video_eq(
                    self.playback.brightness,
                    self.playback.contrast,
                    self.playback.saturation,
                );
                let _ = player.set("options/start", "none");
                let _ = player.set(
                    "loop-file",
                    if self.playback.loop_video && !is_live_initial {
                        "inf"
                    } else {
                        "no"
                    },
                );
                let _ = player.set("pause", "no");

                let resume_secs = resume_target_ms
                    .map(|ms| ms as f64 / 1000.0)
                    .filter(|&s| s > 0.5);
                if is_local_file {
                    if let Err(error) = player.play_local_file(Path::new(&video.url), resume_secs) {
                        player_error = Some(error);
                    }
                } else {
                    let mut direct_started = false;
                    if let Some(manifest) = cached_manifest
                        && let Some(pair) = stream_manifest::select_stream_pair(
                            &manifest,
                            self.settings.playback.value.max_height,
                            None,
                            codec_pref,
                            self.playback.preferred_audio_lang.as_deref(),
                            self.playback.audio_only,
                        )
                    {
                        let opts =
                            pair.to_direct_stream_options(resume_secs, self.playback.audio_only);
                        match player.play_direct_stream(&opts, pending_queue_entry_id) {
                            Ok(()) => {
                                self.playback.active_direct_stream_url = Some(pair.video_url);
                                started_direct_manifest = Some(manifest);
                                direct_started = true;
                            }
                            Err(_) => {
                                if let Some(id) = yt_video_id.as_deref() {
                                    self.playback.stream_manifest_cache.invalidate(id);
                                }
                            }
                        }
                    }
                    if !direct_started {
                        if !is_live_initial && yt_video_id.is_some() {
                            defer_to_manifest_extract = true;
                        } else if let Err(error) = player.play(&video.url) {
                            let _ = player.set("options/start", "none");
                            player_error = Some(error);
                        }
                    }
                }
            }
            if let Some(error) = &player_error {
                self.message = error.clone();
            } else {
                self.message.clear();
            }
            self.watch.share_copied_at = None;
            self.playback.quality = None;
            self.playback.qualities.clear();
            self.playback.quality_error = None;
            self.playback.quality_fallback_url = None;
            self.playback.current_file_loaded = false;
            self.playback.cached_raw_path = None;
            self.playback.cached_raw_path_poll = None;
            self.playback.storyboard = None;
            self.playback.storyboard_video_url = None;
            self.playback.audio_tracks.clear();
            self.playback.audio_tracks_video_url = None;
            self.playback.audio_tracks_last_poll = None;
            self.thumbnails.clear_storyboards(ctx);
            self.playback.seek_drag = None;
            self.playback.player_menu = None;
            self.playback.player_context_menu = None;
            self.playback.last_subtitle_id = None;
            self.reset_decoding_diagnostics();
            self.watch.extras.sync_video(Some(&video.url));
            if incoming_is_short
                && self.watch.shorts_side_panel == app_state::ShortsSidePanel::Comments
            {
                self.watch.extras.tab = watch_extras::Tab::Comments;
            }
            if !is_local_file {
                self.start_watch_next_fetch(&video.url, None, None, ctx);
                self.start_watch_live_for_selected(&video, ctx);
                self.start_sponsorblock_fetch(&video.url, ctx);
                self.start_community_extras_fetch(&video.url, ctx);
            } else {
                self.watch.live.reset_for_video(None, None);
                self.watch.sponsorblock.reset_for_video(None);
            }
            self.playback.quality_task.cancel();
            self.watch.metadata_cache = app_state::WatchMetadataUiCache::default();
            self.watch.overlay_cache = app_state::OverlayCache::default();
            if is_local_file {
                // Local offline media plays directly through libmpv without yt-dlp or remote lookups.
            } else if let Some(manifest) = started_direct_manifest {
                self.hydrate_from_stream_manifest(&video.url, &manifest);
            } else if defer_to_manifest_extract && let Some(video_id) = yt_video_id.clone() {
                self.start_stream_manifest_extract(
                    video_id,
                    video.url.clone(),
                    resume_target_ms,
                    ctx,
                );
            } else if let Some(cached) = search::cached_playback_metadata(&video.url) {
                self.playback.qualities = cached.heights;
                self.playback.storyboard = cached.storyboard;
                self.playback.storyboard_video_url = Some(video.url.clone());
                self.watch
                    .extras
                    .apply_preloaded_subtitles(cached.subtitles);
            } else if history::youtube_video_id(&video.url).is_none() {
                self.playback.quality_fallback_url = Some(video.url.clone());
            }
            self.selected_metadata = self.metadata_for(&video.url).cloned();
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            self.prune_session_caches();
            self.playback.last_video_size_url = None;
            self.playback.last_video_size = None;
            let sub_target = if is_local_file {
                None
            } else {
                self.channel_target_for_video(&video)
            };
            let sub_channel_id = self
                .selected_metadata
                .as_ref()
                .and_then(|meta| meta.channel_id.clone());
            if !is_local_file {
                self.mark_followed_video_seen(sub_channel_id.as_deref(), &video.url);
            }
            self.activate_video_playback(&video, resume_target_ms, previous_player_pos_ms, now);
            if player_error.is_none() && !is_local_file {
                self.record_video_accessed_in_history(&video, resume_target_ms);
                self.start_remote_watch_tracking(&video, resume_target_ms, ctx);
            }
            if explicit_seek_ms.is_some()
                && let Some(active) = &mut self.playback.active
                && !active.is_live
            {
                active.explicit_seek_pending = true;
            }
            if !suppress_back_stack && let Some(prev) = self.selected.clone() {
                self.record_video_back_stack(prev, &video.url);
            }
            self.playback.mini_player_paused = false;
            self.selected = Some(video);
            self.maybe_prefetch_next_video();
            if self.auth.authenticated
                && let Some(target) = sub_target
            {
                self.start_check_subscription(target, sub_channel_id, ctx);
            }
            let keep_persistent_nav_in_shorts = self.is_watch_short_mode()
                && ctx.input(|i| i.content_rect().width())
                    >= layout::PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH;
            if !keep_docked && !keep_persistent_nav_in_shorts {
                self.ui.navigation_open = false;
            }
            self.invalidate_feed_cache();
            self.sync_player_window_visibility();
            player_error.is_none()
        }
        #[cfg(not(windows))]
        {
            let _ = (video, frame, ctx, suppress_back_stack);
            self.message = "A reprodução está disponível somente no Windows.".into();
            false
        }
    }

    pub(crate) fn close_video(&mut self, ctx: &egui::Context) {
        self.cancel_pending_queue_video();
        self.playback.autoplay_eof_handled = false;
        self.playback.auto_subtitle_attempted = false;
        self.playback.save_menu_open = false;
        self.playback.pending_open_video = None;
        self.playback.pending_open_seek_ms = None;
        self.watch.extras.sync_video(None);
        self.watch.live.reset_for_video(None, None);
        self.watch.sponsorblock.reset_for_video(None);
        self.watch.auto_skipped_chapter_indices.clear();
        self.watch.shorts_side_panel = app_state::ShortsSidePanel::None;
        self.watch.community_task.cancel();
        self.watch.show_original_title = false;
        self.playback.quality_task.cancel();
        self.playback.manifest_task.cancel();
        self.playback.current_manifest = None;
        self.playback.active_direct_stream_url = None;
        self.playback.direct_stream_fallback_attempted = false;
        self.playback.pending_open_queue_entry_id = None;
        self.playback.stream_prefetcher.cancel();
        self.watch.next_task.cancel();
        self.watch.innertube_comments_task.cancel();
        self.watch.feedback_task.cancel();
        let now = Instant::now();
        #[cfg(windows)]
        {
            self.remember_current_player_volume();
            self.sample_player_before_transition(now);
        }
        self.flush_remote_watch_tracking();
        self.flush_playback_history_at(now);
        self.playback.active = None;
        self.playback.in_app_mini_player = false;
        self.playback.pending_open_keep_mini_player = false;
        self.playback.pending_open_suppress_back_stack = false;
        self.playback.pending_open_keep_playlist = false;
        self.watch.active_playlist = None;
        if let Some(cancel) = self.watch.mix_cancel.take() {
            cancel.cancel();
        }
        self.watch.mix_rx = None;
        self.watch.mix_active_request_id = None;
        self.watch.mix_active_request_id = None;
        self.playback.mini_player_paused = false;
        self.playback.video_back_stack.clear();
        self.playback.video_forward_stack.clear();
        self.selected = None;
        self.watch.share_copied_at = None;
        self.feed.share_modal_video = None;
        self.ui.navigation_open = false;
        self.invalidate_feed_cache();
        self.selected_metadata = None;
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.watch.metadata_cache = app_state::WatchMetadataUiCache::default();
        self.watch.sidebar_cache = app_state::WatchSidebarCache::default();
        self.watch.overlay_cache = app_state::OverlayCache::default();
        self.playback.last_video_size_url = None;
        self.playback.last_video_size = None;
        self.playback.seek_drag = None;
        self.playback.player_menu = None;
        self.playback.player_context_menu = None;
        self.playback.stats_for_nerds_open = false;
        self.playback.stats_for_nerds = app_state::PlayerNerdsStats::default();
        self.playback.stats_for_nerds_last_poll = None;
        self.playback.overlay_last_pointer_pos = None;
        self.playback.last_subtitle_id = None;
        self.playback.quality = None;
        self.playback.qualities.clear();
        self.playback.quality_task.cancel();
        self.playback.quality_error = None;
        self.playback.quality_fallback_url = None;
        self.playback.current_file_loaded = false;
        self.playback.cached_raw_path = None;
        self.playback.cached_raw_path_poll = None;
        self.playback.storyboard = None;
        self.playback.storyboard_video_url = None;
        self.playback.audio_tracks.clear();
        self.playback.audio_tracks_video_url = None;
        self.playback.audio_tracks_last_poll = None;
        self.thumbnails.clear_storyboards(ctx);
        self.prune_session_caches();
        #[cfg(not(windows))]
        {
            self.playback.active_buffer = None;
            self.playback.active_hwdec = None;
        }
        #[cfg(windows)]
        if self.player.is_none() {
            self.playback.active_buffer = None;
            self.playback.active_hwdec = None;
        }
        self.playback.clear_ab_loop();
        self.playback.audio_only = false;
        self.playback.speed = 1.0;
        self.reset_decoding_diagnostics();
        #[cfg(windows)]
        {
            self.playback.frame_snapshot = None;
            if let Some(player) = &mut self.player {
                player.set_visible(false);
                player.park_warm();
            }
        }
        if self.playback.mini_player {
            self.toggle_mini_player(ctx);
        }
        if self.playback.fullscreen {
            self.toggle_fullscreen(ctx);
        }
        self.sync_player_window_visibility();
    }

    pub(crate) fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        if !self.playback.fullscreen {
            let current_maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            let mut restore_maximized = current_maximized;
            if self.playback.mini_player {
                restore_maximized |=
                    std::mem::take(&mut self.playback.mini_player_restore_maximized);
                self.toggle_mini_player(ctx);
            }
            self.playback.in_app_mini_player = false;
            self.playback.fullscreen = true;
            self.playback.fullscreen_restore_maximized = restore_maximized;
            self.playback.fullscreen_sync_retries = 3;
            self.playback.player_context_menu = None;
            self.playback.overlay_last_activity = Instant::now();
            self.playback.overlay_last_pointer_pos = ctx.input(|i| i.pointer.hover_pos());
            if current_maximized {
                // Clear WS_MAXIMIZE before entering fullscreen; winit's WM_NCCALCSIZE clamps maximized windows to rcWork.
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            // Force egui-winit to clear MARKER_UNDECORATED_SHADOW while fullscreen to avoid a 1px WM_NCCALCSIZE offset.
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        } else {
            let restore_maximized = std::mem::take(&mut self.playback.fullscreen_restore_maximized);
            self.playback.fullscreen = false;
            self.playback.fullscreen_sync_retries = 0;
            self.playback.player_context_menu = None;
            self.playback.overlay_last_activity = Instant::now();
            self.playback.overlay_last_pointer_pos = ctx.input(|i| i.pointer.hover_pos());
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            // Restore undecorated drop shadow after leaving fullscreen.
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
            if restore_maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
        self.sync_player_window_visibility();
    }

    pub(crate) fn sync_fullscreen_viewport(&mut self, ctx: &egui::Context) {
        if !self.playback.fullscreen {
            return;
        }
        let (is_maximized, is_fullscreen) = ctx.input(|input| {
            let vp = input.viewport();
            (vp.maximized == Some(true), vp.fullscreen == Some(true))
        });
        if is_maximized {
            if self.playback.fullscreen_sync_retries > 0 {
                self.playback.fullscreen_sync_retries -= 1;
                self.playback.fullscreen_restore_maximized = true;
                if is_fullscreen {
                    // winit ignores set_fullscreen(Borderless(None)) when already fullscreen; reset first to reapply bounds.
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                ctx.request_repaint();
            }
        } else if is_fullscreen {
            self.playback.fullscreen_sync_retries = 3;
        }
    }

    pub(crate) fn toggle_theater_mode(&mut self) {
        self.playback.theater = !self.playback.theater;
        self.sync_player_window_visibility();
    }

    pub(crate) fn is_local_offline_media_path(url: &str) -> bool {
        let trimmed = url.trim();
        !trimmed.is_empty()
            && !trimmed.starts_with("http://")
            && !trimmed.starts_with("https://")
            && Path::new(trimmed).is_file()
    }

    pub(crate) fn adjust_player_volume_by(&mut self, delta: f64) {
        #[cfg(windows)]
        let current = self
            .player
            .as_ref()
            .and_then(|p| p.volume())
            .unwrap_or(f64::from(self.playback.mini_player_volume));
        #[cfg(not(windows))]
        let current = f64::from(self.playback.mini_player_volume);
        let next = player::clamp_volume_percent(current + delta);
        self.playback.mini_player_volume = next as f32;
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let _ = player.set_volume(next);
            if next > 0.0 && player.flag_cstr(c"mute") == Some(true) {
                let _ = player.set("mute", "no");
                self.playback.mini_player_muted = false;
            }
        }
        self.message = format!("Volume: {:.0}%", next);
    }

    pub(crate) fn step_caption_font_scale(&mut self, delta: f32) {
        let mut prefs = self.settings.prefs.value.clone();
        let next = ((prefs.caption_style.font_scale + delta) * 100.0).round() / 100.0;
        prefs.caption_style.font_scale = next.clamp(0.5, 2.5);
        let applied = prefs.caption_style.font_scale;
        self.apply_app_preferences(prefs);
        let pt = self.ui_pt();
        self.message = if pt {
            format!("Tamanho das legendas: {:.0}%", applied * 100.0)
        } else {
            format!("Caption size: {:.0}%", applied * 100.0)
        };
    }

    pub(crate) fn jump_to_adjacent_chapter(&mut self, forward: bool) {
        let mut chapters: Vec<f64> = if !self.watch.overlay_cache.chapters.is_empty() {
            self.watch
                .overlay_cache
                .chapters
                .iter()
                .map(|c| c.time_secs)
                .collect()
        } else if let Some(info) = &self.watch.live.info
            && !info.chapters.is_empty()
        {
            info.chapters.iter().map(|c| c.time_secs).collect()
        } else {
            self.watch
                .metadata_cache
                .description_chapters
                .iter()
                .map(|c| c.seconds as f64)
                .collect()
        };
        if chapters.is_empty() {
            return;
        }
        chapters.sort_by(|a, b| a.total_cmp(b));
        chapters.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        #[cfg(windows)]
        let cur_secs = self
            .player
            .as_ref()
            .and_then(|p| p.double_cstr(c"time-pos"))
            .filter(|&t| t >= 0.0)
            .unwrap_or_else(|| {
                self.playback
                    .active
                    .as_ref()
                    .map_or(0.0, |a| a.position_ms as f64 / 1000.0)
            });
        #[cfg(not(windows))]
        let cur_secs = self
            .playback
            .active
            .as_ref()
            .map_or(0.0, |a| a.position_ms as f64 / 1000.0);
        let target = if forward {
            chapters.into_iter().find(|&t| t > cur_secs + 1.0)
        } else {
            chapters
                .into_iter()
                .rev()
                .find(|&t| t + 2.0 < cur_secs)
                .or(Some(0.0))
        };
        if let Some(secs) = target {
            self.seek_to_seconds(secs);
        }
    }

    pub(crate) fn raw_path_matches_video(&self, raw_path: Option<&str>, video_url: &str) -> bool {
        raw_path.is_some_and(|path| {
            let trimmed = path.trim();
            trimmed == video_url
                || self
                    .playback
                    .active_direct_stream_url
                    .as_deref()
                    .is_some_and(|direct| direct == trimmed)
                || (history::youtube_video_id(trimmed).is_some()
                    && history::youtube_video_id(trimmed) == history::youtube_video_id(video_url))
        })
    }

    pub(crate) fn hydrate_from_stream_manifest(
        &mut self,
        video_url: &str,
        manifest: &std::sync::Arc<stream_manifest::StreamManifest>,
    ) {
        self.playback.current_manifest = Some(std::sync::Arc::clone(manifest));
        let heights = manifest.available_heights();
        if !heights.is_empty() {
            self.playback.qualities = heights;
            self.playback.quality_error = None;
            self.playback.quality_fallback_url = None;
        }
        if let Some(sb) = manifest.storyboard.as_ref() {
            self.playback.storyboard = Some(sb.to_storyboard_spec());
            self.playback.storyboard_video_url = Some(video_url.to_owned());
        }
        let chapters = manifest.to_overlay_chapters();
        if !chapters.is_empty() {
            self.watch.overlay_cache.chapters = chapters;
            self.watch.overlay_cache.chapters_video_url = Some(video_url.to_owned());
        }
        let audio_opts = stream_manifest::available_audio_tracks(manifest);
        if !audio_opts.is_empty() {
            let pref = self.playback.preferred_audio_lang.as_deref();
            self.playback.audio_tracks = audio_opts
                .iter()
                .map(|opt| {
                    let selected = match pref {
                        Some(p) => opt.language_code.eq_ignore_ascii_case(p),
                        None => opt.is_default,
                    };
                    opt.to_overlay_audio_track(selected)
                })
                .collect();
            self.playback.audio_tracks_video_url = Some(video_url.to_owned());
        }
        self.watch.extras.apply_preloaded_manifest_extras(manifest);
        self.watch.overlay_cache.mpv_subs_last_poll = None;
        self.watch.overlay_cache.force_refresh = true;
        if manifest.channel_verified || manifest.upload_date.is_some() {
            let mut changed = false;
            let meta = self.video_metadata.entry(video_url.to_owned()).or_default();
            if manifest.channel_verified && !meta.channel_verified {
                meta.channel_verified = true;
                changed = true;
            }
            if meta.upload_date.is_none()
                && let Some(date) = manifest.upload_date.clone()
            {
                meta.upload_date = Some(date);
                changed = true;
            }
            if changed {
                if self.selected.as_ref().is_some_and(|v| v.url == video_url) {
                    self.selected_metadata = Some(meta.clone());
                }
                self.watch.metadata_cache.valid = false;
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            }
        }
    }

    pub(crate) fn start_stream_manifest_extract(
        &mut self,
        video_id: String,
        video_url: String,
        resume_target_ms: Option<u64>,
        ctx: &egui::Context,
    ) {
        let cache = std::sync::Arc::clone(&self.playback.stream_manifest_cache);
        self.playback.manifest_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result = stream_manifest::extract_stream_manifest(&video_url, &cancel)
                    .map(|manifest| cache.insert(manifest));
                app_state::StreamManifestFetchMessage {
                    request_id,
                    video_id,
                    video_url,
                    resume_target_ms,
                    result,
                }
            },
        );
    }

    #[allow(dead_code)]
    #[cfg(test)]
    pub(crate) fn complete_stream_manifest(
        &mut self,
        msg: app_state::StreamManifestFetchMessage,
    ) -> bool {
        if self.playback.manifest_task.active_request_id() != Some(msg.request_id) {
            return false;
        }
        self.playback.manifest_task.cancel();
        self.apply_stream_manifest(msg)
    }

    pub(crate) fn apply_stream_manifest(
        &mut self,
        msg: app_state::StreamManifestFetchMessage,
    ) -> bool {
        if self
            .selected
            .as_ref()
            .is_none_or(|v| v.url != msg.video_url)
        {
            return false;
        }
        match msg.result {
            Ok(manifest) => {
                if !manifest.is_live && !manifest.is_expired() {
                    self.hydrate_from_stream_manifest(&msg.video_url, &manifest);
                    let codec_pref = self.settings.prefs.value.video_codec_preference;
                    if let Some(pair) = stream_manifest::select_stream_pair(
                        &manifest,
                        self.settings.playback.value.max_height,
                        self.playback.quality,
                        codec_pref,
                        self.playback.preferred_audio_lang.as_deref(),
                        self.playback.audio_only,
                    ) {
                        let resume_secs = msg
                            .resume_target_ms
                            .map(|ms| ms as f64 / 1000.0)
                            .filter(|&s| s > 0.5);
                        let opts =
                            pair.to_direct_stream_options(resume_secs, self.playback.audio_only);
                        #[cfg(windows)]
                        if let Some(player) = &mut self.player {
                            if player
                                .play_direct_stream(&opts, self.queue.pending_entry_id)
                                .is_ok()
                            {
                                self.playback.active_direct_stream_url = Some(pair.video_url);
                                return true;
                            }
                            self.playback
                                .stream_manifest_cache
                                .invalidate(&msg.video_id);
                            if let Err(err) = player.play(&msg.video_url) {
                                self.message = err;
                            }
                            return true;
                        }
                        #[cfg(not(windows))]
                        let _ = opts;
                        self.playback.active_direct_stream_url = Some(pair.video_url);
                        return true;
                    }
                }
                #[cfg(windows)]
                if let Some(player) = &mut self.player
                    && let Err(err) = player.play(&msg.video_url)
                {
                    self.message = err;
                }
            }
            Err(_) => {
                #[cfg(windows)]
                if let Some(player) = &mut self.player
                    && let Err(err) = player.play(&msg.video_url)
                {
                    self.message = err;
                }
            }
        }
        true
    }

    pub(crate) fn poll_stream_manifest(&mut self, _ctx: &egui::Context) {
        match self
            .playback
            .manifest_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                let _ = self.apply_stream_manifest(msg);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if self.playback.active_direct_stream_url.is_none()
                    && let Some(video_url) = self.selected.as_ref().map(|v| v.url.clone())
                {
                    #[cfg(windows)]
                    if let Some(player) = &mut self.player
                        && let Err(err) = player.play(&video_url)
                    {
                        self.message = err;
                    }
                    #[cfg(not(windows))]
                    let _ = video_url;
                }
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn handle_direct_stream_failure_fallback(&mut self) -> bool {
        if self.playback.active_direct_stream_url.is_none()
            || self.playback.direct_stream_fallback_attempted
        {
            return false;
        }
        self.playback.direct_stream_fallback_attempted = true;
        self.playback.active_direct_stream_url = None;
        self.playback.current_manifest = None;
        let Some(video) = self.selected.clone() else {
            return false;
        };
        if let Some(video_id) = history::youtube_video_id(&video.url) {
            self.playback.stream_manifest_cache.invalidate(&video_id);
        }
        #[cfg(windows)]
        if let Some(player) = &mut self.player
            && let Err(err) = player.play(&video.url)
        {
            self.message = err;
        }
        true
    }

    pub(crate) fn next_prefetch_candidate(&self) -> Option<(String, String)> {
        let current_id = self
            .selected
            .as_ref()
            .and_then(|v| history::youtube_video_id(&v.url));
        for queued in self.queue.data.videos() {
            if let Some(id) = history::youtube_video_id(&queued.url)
                && current_id.as_deref() != Some(id.as_str())
            {
                return Some((id, queued.url.clone()));
            }
        }
        if let Some((_, next_video)) = self
            .watch
            .active_playlist
            .as_ref()
            .and_then(app_state::ActivePlaylistPlayback::peek_next)
            && let Some(id) = history::youtube_video_id(&next_video.url)
            && current_id.as_deref() != Some(id.as_str())
        {
            return Some((id, next_video.url));
        }
        None
    }

    pub(crate) fn maybe_prefetch_next_video(&mut self) {
        #[cfg(not(test))]
        if let Some((next_id, next_url)) = self.next_prefetch_candidate() {
            let _ = self
                .playback
                .stream_prefetcher
                .prefetch(&next_id, &next_url);
        }
    }

    pub(crate) fn same_video_target(a: &str, b: &str) -> bool {
        match (history::youtube_video_id(a), history::youtube_video_id(b)) {
            (Some(id_a), Some(id_b)) => id_a == id_b,
            _ => a == b,
        }
    }

    pub(crate) fn find_known_video_by_id(&self, video_id: &str, fallback_title: &str) -> Video {
        let canonical_url = history::youtube_watch_url(video_id)
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={video_id}"));
        let matches_id = |video: &Video| {
            history::youtube_video_id(&video.url).as_deref() == Some(video_id)
                || video.url == canonical_url
        };
        if let Some(video) = self.selected.as_ref().filter(|v| matches_id(v)) {
            return video.clone();
        }
        if let Some(video) = self.feed.videos.iter().find(|v| matches_id(v)) {
            return video.clone();
        }
        if let Some(video) = self.queue.data.videos().iter().find(|v| matches_id(v)) {
            return video.clone();
        }
        if let Some(record) = self
            .library
            .data
            .watch_later
            .iter()
            .find(|r| matches_id(&r.video))
        {
            return record.video.clone();
        }
        if let Some(record) = self
            .library
            .data
            .favorites
            .iter()
            .find(|r| matches_id(&r.video))
        {
            return record.video.clone();
        }
        for collection in &self.library.data.collections {
            if let Some(record) = collection.videos.iter().find(|r| matches_id(&r.video)) {
                return record.video.clone();
            }
        }
        if let Some(entry) = self.history.data.get(video_id) {
            return Video {
                title: entry.title.clone(),
                channel: entry.channel.clone(),
                url: canonical_url,
                thumbnail: String::new(),
                duration: entry
                    .duration_ms
                    .map(command_palette::format_timestamp_ms)
                    .unwrap_or_default(),
            };
        }
        let title = if fallback_title.trim().is_empty() {
            video_id.to_owned()
        } else {
            fallback_title.to_owned()
        };
        Video {
            title,
            channel: String::new(),
            url: canonical_url,
            thumbnail: String::new(),
            duration: String::new(),
        }
    }

    pub(crate) fn open_video_at_timestamp(
        &mut self,
        video: Video,
        seek_ms: Option<u64>,
        ctx: &egui::Context,
    ) {
        self.ui.settings_open = false;
        self.close_channel_view();
        self.playback.pending_open_keep_mini_player = false;
        self.playback.pending_open_suppress_back_stack = false;
        if self
            .selected
            .as_ref()
            .is_some_and(|current| Self::same_video_target(&current.url, &video.url))
        {
            self.playback.pending_open_video = None;
            self.playback.pending_open_seek_ms = None;
            self.playback.in_app_mini_player = false;
            if let Some(ms) = seek_ms {
                self.seek_to_seconds(ms as f64 / 1000.0);
            }
            self.sync_player_window_visibility();
            ctx.request_repaint();
            return;
        }
        self.playback.in_app_mini_player = false;
        self.playback.pending_open_seek_ms = seek_ms;
        self.playback.pending_open_video = Some(video);
        ctx.request_repaint();
    }

    pub(crate) fn open_direct_youtube_target(
        &mut self,
        target: command_palette::DirectYouTubeTarget,
        ctx: &egui::Context,
    ) {
        match target {
            command_palette::DirectYouTubeTarget::Video {
                url,
                video_id,
                start_ms,
            } => {
                let mut video = self.find_known_video_by_id(&video_id, &url);
                if video.url.is_empty() {
                    video.url = url;
                }
                self.open_video_at_timestamp(video, start_ms, ctx);
            }
            command_palette::DirectYouTubeTarget::Channel(channel_target) => {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.queue.remote_playlist_id = None;
                self.ui.browse_section = BrowseSection::Home;
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.open_channel(channel_target, ctx);
            }
            command_palette::DirectYouTubeTarget::Playlist { url, list_id } => {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Home;
                self.feed.filter = FeedFilter::All;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.start_remote_playlist_fetch(&list_id, &url, ctx);
            }
        }
    }
}

pub(crate) fn screenshot_dir_candidates(
    user_profile: Option<&str>,
    local_app_data: Option<&str>,
) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(profile) = user_profile.map(str::trim).filter(|s| !s.is_empty()) {
        dirs.push(
            std::path::PathBuf::from(profile)
                .join("Pictures")
                .join("FastTube"),
        );
    }
    if let Some(local) = local_app_data.map(str::trim).filter(|s| !s.is_empty()) {
        let fallback = std::path::PathBuf::from(local)
            .join("FastTube")
            .join("screenshots");
        if !dirs.contains(&fallback) {
            dirs.push(fallback);
        }
    }
    if dirs.is_empty() {
        dirs.push(std::env::temp_dir().join("FastTube").join("screenshots"));
    }
    dirs
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn resolve_screenshot_dir_from(
    user_profile: Option<&str>,
    local_app_data: Option<&str>,
) -> std::path::PathBuf {
    screenshot_dir_candidates(user_profile, local_app_data)
        .into_iter()
        .next()
        .unwrap_or_else(|| std::env::temp_dir().join("FastTube").join("screenshots"))
}

pub(crate) fn build_screenshot_filename(
    video: &Video,
    position_secs: f64,
    now_epoch_secs: u64,
) -> String {
    let mut slug = String::new();
    let mut prev_sep = false;
    for ch in video.title.trim().chars() {
        if slug.len() >= 48 {
            break;
        }
        let valid = !ch.is_control()
            && !matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            && !ch.is_whitespace();
        if valid {
            slug.push(ch);
            prev_sep = false;
        } else if !slug.is_empty() && !prev_sep {
            slug.push('_');
            prev_sep = true;
        }
    }
    let slug = slug.trim_matches('_');
    let base = if slug.is_empty() {
        history::youtube_video_id(&video.url).unwrap_or_else(|| "video".to_owned())
    } else {
        slug.to_owned()
    };
    let clamped_secs = if position_secs.is_finite() && position_secs > 0.0 {
        position_secs.floor() as u64
    } else {
        0
    };
    let mins = clamped_secs / 60;
    let secs = clamped_secs % 60;
    format!("fasttube_{base}_{mins}m{secs:02}s_{now_epoch_secs}.png")
}
