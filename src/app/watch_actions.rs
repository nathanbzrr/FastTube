use crate::*;

impl FastTube {
    pub(crate) fn update_live_playback_state(
        &mut self,
        paused: bool,
        raw_position: Option<f64>,
        raw_duration: Option<f64>,
        seekable: Option<bool>,
        now: Instant,
    ) {
        let Some(active) = &mut self.playback.active else {
            return;
        };
        let live_info_for_active = self
            .watch
            .live
            .info
            .as_ref()
            .filter(|_| self.watch.live.video_url.as_deref() == Some(active.url.as_str()));
        let metadata_live = self
            .video_metadata
            .get(&active.url)
            .is_some_and(|meta| meta.is_live)
            || self
                .selected
                .as_ref()
                .filter(|v| v.url == active.url)
                .is_some_and(|v| is_live_duration_label(&v.duration));

        if (metadata_live || live_info_for_active.is_some_and(|info| info.is_live))
            && !active.is_live
        {
            active.is_live = true;
            active.at_live_edge = true;
        }
        if !active.is_live {
            active.live_dvr = false;
            active.at_live_edge = false;
            active.live_paused_since = None;
            active.live_paused_accumulated_secs = 0.0;
            active.live_behind_secs = 0.0;
            return;
        }

        active.live_dvr = match live_info_for_active.filter(|info| info.is_live) {
            Some(info) => info.is_live_dvr,
            None => seekable == Some(true) && raw_duration.is_some_and(|dur| dur > 30.0),
        };

        if paused {
            if active.live_paused_since.is_none() {
                active.live_paused_since = Some(now);
            }
        } else if let Some(since) = active.live_paused_since.take() {
            active.live_paused_accumulated_secs +=
                now.saturating_duration_since(since).as_secs_f64();
        }

        let paused_total_secs = active.live_paused_accumulated_secs
            + active.live_paused_since.map_or(0.0, |since| {
                now.saturating_duration_since(since).as_secs_f64()
            });

        if !active.live_dvr {
            if paused_total_secs >= 2.0 {
                active.at_live_edge = false;
                active.live_behind_secs = paused_total_secs;
            } else if active.at_live_edge {
                active.live_behind_secs = 0.0;
            } else {
                active.live_behind_secs = paused_total_secs.max(2.0);
            }
            return;
        }

        let (dvr_lag_secs, near_dvr_head) = match (raw_position, raw_duration) {
            (Some(pos), Some(dur)) if pos.is_finite() && dur.is_finite() && dur > 0.0 => {
                let lag = (dur - pos).max(0.0);
                let near = (pos / dur) >= 0.985 || lag <= 4.0;
                (lag, near)
            }
            _ => (0.0, false),
        };

        if active.at_live_edge {
            let drifted_far =
                raw_duration.is_some_and(|dur| dur > 20.0) && !near_dvr_head && dvr_lag_secs > 15.0;
            if paused_total_secs >= 2.0 || drifted_far {
                active.at_live_edge = false;
                active.live_behind_secs = dvr_lag_secs.max(paused_total_secs);
            } else {
                active.live_behind_secs = 0.0;
            }
        } else if near_dvr_head && paused_total_secs < 2.0 {
            active.at_live_edge = true;
            active.live_paused_accumulated_secs = 0.0;
            active.live_behind_secs = 0.0;
        } else {
            active.live_behind_secs = dvr_lag_secs.max(paused_total_secs);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_live_metadata_to_video(
        &mut self,
        video_url: &str,
        is_live: bool,
        is_live_dvr: Option<bool>,
        title: Option<&str>,
        channel: Option<&str>,
        channel_id: Option<&str>,
        channel_url: Option<&str>,
        channel_avatar_url: Option<&str>,
        subscriber_count_text: Option<&str>,
        description: Option<&str>,
        concurrent_view_count: Option<u64>,
        ctx: Option<&egui::Context>,
    ) {
        let mut changed = false;
        let meta = self.video_metadata.entry(video_url.to_owned()).or_default();
        if is_live && !meta.is_live {
            meta.is_live = true;
            changed = true;
        }
        if meta
            .channel_id
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(id) = channel_id.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.channel_id = Some(id.to_owned());
            changed = true;
        }
        if meta
            .channel_url
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(url) = channel_url.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.channel_url = Some(url.to_owned());
            changed = true;
        }
        if meta
            .channel_avatar_url
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(url) = channel_avatar_url.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.channel_avatar_url = Some(url.to_owned());
            changed = true;
        }
        if meta
            .subscriber_count_text
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(subs) = subscriber_count_text
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            meta.subscriber_count_text = Some(subs.to_owned());
            changed = true;
        }
        if meta
            .description
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
            && let Some(desc) = description.map(str::trim).filter(|s| !s.is_empty())
        {
            meta.description = Some(desc.to_owned());
            changed = true;
        }
        if let Some(viewers) = concurrent_view_count
            && meta.view_count != Some(viewers)
        {
            meta.view_count = Some(viewers);
            changed = true;
        }
        let cloned_meta = meta.clone();

        if let Some(av) = channel_avatar_url.map(str::trim).filter(|s| !s.is_empty())
            && let Some(ch) = channel.map(str::trim).filter(|s| !s.is_empty())
        {
            let key = recommendations::normalize_channel_key(ch);
            if !key.is_empty() {
                self.feed
                    .home
                    .channel_avatars
                    .entry(key)
                    .or_insert_with(|| av.to_owned());
            }
        }

        if let Some(selected) = &mut self.selected
            && selected.url == video_url
        {
            if let Some(t) = title.map(str::trim).filter(|s| !s.is_empty())
                && (selected.title.trim().is_empty()
                    || selected.title == selected.url
                    || selected.title.starts_with("http"))
            {
                selected.title = t.to_owned();
                changed = true;
            }
            if let Some(ch) = channel.map(str::trim).filter(|s| !s.is_empty()) {
                let current_ch = selected.channel.trim();
                let looks_like_id = current_ch.is_empty()
                    || (current_ch.starts_with("UC")
                        && current_ch.len() >= 20
                        && !current_ch.contains(' '))
                    || channel_id.is_some_and(|id| id == current_ch);
                if looks_like_id {
                    selected.channel = ch.to_owned();
                    changed = true;
                }
            }
            if is_live && !is_live_duration_label(&selected.duration) {
                selected.duration = "Ao vivo".to_owned();
                changed = true;
            }
            self.selected_metadata = Some(cloned_meta.clone());
        }

        if let Some(active) = &mut self.playback.active
            && active.url == video_url
        {
            if is_live && !active.is_live {
                active.is_live = true;
                active.at_live_edge = true;
                active.resume_target_ms = None;
                active.resume_seek_pending = false;
            }
            if let Some(dvr) = is_live_dvr {
                active.live_dvr = dvr;
            }
            if let Some(selected) = &self.selected
                && selected.url == video_url
            {
                active.title = selected.title.clone();
                active.channel = selected.channel.clone();
            }
        }

        if changed {
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            self.invalidate_feed_cache();
        }

        if let Some(ctx) = ctx
            && self.auth.authenticated
            && !self.auth.sub_check_task.is_running()
            && self.selected.as_ref().is_some_and(|v| v.url == video_url)
            && let Some(target) = search::channel_target_for_metadata(&cloned_meta)
            && self
                .subscription_status_for(Some(&target), cloned_meta.channel_id.as_deref())
                .is_none()
        {
            self.start_check_subscription(target, cloned_meta.channel_id, ctx);
        }
    }

    pub(crate) fn go_live_edge(&mut self, video: &Video) {
        #[cfg(windows)]
        if let Some(player) = &self.player {
            let format = player::ytdl_format_for_codec_pref(
                self.settings.playback.value.max_height,
                self.settings.prefs.value.video_codec_preference,
            );
            if let Err(error) = player.quality(&video.url, self.playback.quality, &format, false) {
                self.message = error;
                return;
            }
            self.playback.cached_raw_path_poll = None;
            let _ = player.set("pause", "no");
        }
        #[cfg(not(windows))]
        let _ = video;
        if let Some(active) = &mut self.playback.active
            && active.url == video.url
        {
            active.at_live_edge = true;
            active.live_paused_since = None;
            active.live_paused_accumulated_secs = 0.0;
            active.live_behind_secs = 0.0;
            active.resume_target_ms = None;
            active.resume_seek_pending = false;
            active.has_live_sample = false;
        }
        self.playback.overlay_last_activity = Instant::now();
    }

    pub(crate) fn start_watch_live_for_selected(&mut self, video: &Video, ctx: &egui::Context) {
        let video_id = history::youtube_video_id(&video.url);
        self.watch
            .live
            .reset_for_video(Some(&video.url), video_id.clone());
        if let Some(id) = video_id {
            let (rx, cancel) = live_chat::start_watch_live_fetch(
                self.watch.live.session_id,
                id,
                self.auth.youtube_web_session.clone(),
                ctx,
            );
            self.watch.live.watch_info_loading = true;
            self.watch.live.watch_rx = Some(rx);
            self.watch.live.watch_cancel = Some(cancel);
        }
    }

    pub(crate) fn start_live_chat_stream_if_needed(&mut self, ctx: &egui::Context) {
        let continuation = self.watch.live.active_continuation.clone().or_else(|| {
            self.watch
                .live
                .info
                .as_ref()
                .and_then(|info| info.initial_continuation.clone())
        });
        let Some(cont) = continuation else {
            return;
        };
        self.watch.live.stop_chat_stream();
        self.watch.live.active_continuation = Some(cont.clone());
        self.watch.live.chat_loading = true;
        self.watch.live.chat_error = None;
        let version = if self.watch.live.client_version.is_empty() {
            comment_actions::FALLBACK_CLIENT_VERSION.to_owned()
        } else {
            self.watch.live.client_version.clone()
        };
        let (rx, cancel) = live_chat::start_live_chat_stream(
            self.watch.live.session_id,
            self.watch.live.chat_stream_id,
            cont,
            version,
            self.watch.live.visitor_data.clone(),
            self.auth.youtube_web_session.clone(),
            ctx,
        );
        self.watch.live.stream_rx = Some(rx);
        self.watch.live.stream_cancel = Some(cancel);
    }

    pub(crate) fn switch_live_chat_mode(&mut self, mode_index: usize, ctx: &egui::Context) {
        let Some(mode) = self.watch.live.chat_modes.get(mode_index).cloned() else {
            return;
        };
        for (idx, item) in self.watch.live.chat_modes.iter_mut().enumerate() {
            item.selected = idx == mode_index;
        }
        self.watch.live.messages.clear();
        self.watch.live.seen_ids.clear();
        self.watch.live.auto_scroll = true;
        self.watch.live.active_continuation = Some(mode.continuation);
        self.start_live_chat_stream_if_needed(ctx);
    }

    pub(crate) fn send_live_chat_draft(
        &mut self,
        allow_interactive_login: bool,
        ctx: &egui::Context,
    ) {
        let Some(send_params) = self.watch.live.send_params.clone() else {
            self.watch.live.send_error =
                Some("O envio de mensagens ainda não está disponível neste chat.".into());
            return;
        };
        let text = self.watch.live.composer_draft.trim().to_owned();
        if text.is_empty() {
            self.watch.live.send_error = Some("Digite uma mensagem para enviar.".into());
            return;
        }
        if let Some(cancel) = self.watch.live.send_cancel.take() {
            cancel.cancel();
        }
        self.watch.live.sending = true;
        self.watch.live.send_error = None;
        let version = if self.watch.live.client_version.is_empty() {
            comment_actions::FALLBACK_CLIENT_VERSION.to_owned()
        } else {
            self.watch.live.client_version.clone()
        };
        let (rx, cancel) = live_chat::start_send_live_chat_message(
            self.watch.live.session_id,
            self.auth.youtube_web_session.clone(),
            allow_interactive_login,
            send_params,
            text,
            version,
            self.watch.live.visitor_data.clone(),
            ctx,
        );
        self.watch.live.send_rx = Some(rx);
        self.watch.live.send_cancel = Some(cancel);
    }

    pub(crate) fn poll_live_watch(&mut self, ctx: &egui::Context) {
        let watch_outcome = self.watch.live.watch_rx.as_ref().map(Receiver::try_recv);
        match watch_outcome {
            Some(Ok(live_chat::WatchLiveEvent::WebSessionUpdated(sess))) => {
                self.auth.youtube_web_session = Some(sess);
                self.auth.youtube_web_session_checked = true;
                // Drain WatchInfo if queued right after WebSessionUpdated.
                if let Some(Ok(live_chat::WatchLiveEvent::WatchInfo { session_id, result })) =
                    self.watch.live.watch_rx.as_ref().map(Receiver::try_recv)
                {
                    self.handle_watch_live_info_result(session_id, *result, ctx);
                }
            }
            Some(Ok(live_chat::WatchLiveEvent::WatchInfo { session_id, result })) => {
                self.handle_watch_live_info_result(session_id, *result, ctx);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.watch.live.watch_info_loading = false;
                self.watch.live.watch_rx = None;
                self.watch.live.watch_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }

        let mut stream_events = Vec::new();
        let mut stream_disconnected = false;
        if let Some(rx) = &self.watch.live.stream_rx {
            loop {
                match rx.try_recv() {
                    Ok(ev) => stream_events.push(ev),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        stream_disconnected = true;
                        break;
                    }
                }
            }
        }
        for ev in stream_events {
            match ev {
                live_chat::LiveChatStreamEvent::WebSessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                live_chat::LiveChatStreamEvent::Batch {
                    session_id,
                    stream_id,
                    result,
                } => {
                    if session_id != self.watch.live.session_id
                        || stream_id != self.watch.live.chat_stream_id
                    {
                        continue;
                    }
                    self.watch.live.chat_loading = false;
                    match result {
                        Ok(batch) => {
                            self.watch.live.chat_error = None;
                            self.watch.live.apply_batch(batch);
                        }
                        Err(err) => {
                            self.watch.live.chat_error = Some(err);
                        }
                    }
                }
            }
        }
        if stream_disconnected && self.watch.live.stream_rx.is_some() {
            self.watch.live.stream_rx = None;
            self.watch.live.stream_cancel = None;
            self.watch.live.chat_loading = false;
        }

        let send_outcome = self.watch.live.send_rx.as_ref().map(Receiver::try_recv);
        match send_outcome {
            Some(Ok(ev)) => {
                if ev.auth_expired {
                    self.auth.youtube_web_session = None;
                } else if let Some(sess) = ev.web_session {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                if ev.session_id == self.watch.live.session_id {
                    self.watch.live.sending = false;
                    self.watch.live.send_rx = None;
                    self.watch.live.send_cancel = None;
                    match ev.result {
                        Ok(msgs) => {
                            self.watch.live.composer_draft.clear();
                            self.watch.live.send_error = None;
                            for msg in msgs {
                                self.watch.live.push_message(msg);
                            }
                        }
                        Err(err) => {
                            self.watch.live.send_error = Some(err);
                        }
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.watch.live.sending = false;
                self.watch.live.send_rx = None;
                self.watch.live.send_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn handle_watch_live_info_result(
        &mut self,
        session_id: u64,
        result: Result<live_chat::WatchLiveInfo, String>,
        ctx: &egui::Context,
    ) {
        if session_id != self.watch.live.session_id {
            return;
        }
        self.watch.live.watch_info_loading = false;
        self.watch.live.watch_rx = None;
        self.watch.live.watch_cancel = None;
        match result {
            Ok(info) => {
                self.watch.live.watch_info_error = None;
                if !info.client_version.is_empty() {
                    self.watch.live.client_version = info.client_version.clone();
                }
                if info.visitor_data.is_some() {
                    self.watch.live.visitor_data = info.visitor_data.clone();
                }
                if !info.modes.is_empty() {
                    self.watch.live.chat_modes = info.modes.clone();
                }
                if self.watch.live.active_continuation.is_none() {
                    self.watch.live.active_continuation = info.initial_continuation.clone();
                }
                self.watch.live.info_revision = self.watch.live.info_revision.wrapping_add(1);
                let video_url = self.watch.live.video_url.clone();
                let is_live = info.is_live;
                let is_live_dvr = info.is_live_dvr;
                let title = info.title.clone();
                let channel = info.channel.clone();
                let channel_id = info.channel_id.clone();
                let channel_url = info.channel_url.clone();
                let channel_avatar_url = info.channel_avatar_url.clone();
                let channel_verified = info.channel_verified;
                let subscriber_count_text = info.subscriber_count_text.clone();
                let description = info.description.clone();
                let concurrent_view_count = info.concurrent_view_count;
                let heights = info.heights.clone();
                let subtitles = info.subtitles.clone();
                let subtitles_parsed = info.subtitles_parsed;
                let storyboard = info.storyboard.clone();
                let relative_date_text = info.relative_date_text.clone();
                let started_text = info.started_text.clone();
                let live_view_count_text = info.view_count_text.clone();
                self.watch.live.info = Some(info);
                let fallback_date = relative_date_text.or_else(|| {
                    (!is_live)
                        .then_some(started_text)
                        .flatten()
                        .and_then(|s| search::clean_published_time_text(&s))
                });
                let fallback_views = (!is_live).then_some(live_view_count_text).flatten();
                if (channel_verified || fallback_date.is_some() || fallback_views.is_some())
                    && let Some(url) = video_url.as_deref()
                {
                    let mut changed = false;
                    let meta = self.video_metadata.entry(url.to_owned()).or_default();
                    if channel_verified && !meta.channel_verified {
                        meta.channel_verified = true;
                        changed = true;
                    }
                    if meta.view_count_text.is_none()
                        && let Some(raw_vtext) = fallback_views.as_deref()
                        && let Some(vtext) =
                            search::clean_pt_br_view_label(raw_vtext).or_else(|| {
                                (!raw_vtext.trim().is_empty()).then(|| raw_vtext.trim().to_owned())
                            })
                    {
                        if meta.view_count.is_none() {
                            meta.view_count = search::parse_pt_br_compact_count(&vtext);
                        }
                        meta.view_count_text = Some(vtext);
                        changed = true;
                    }
                    if meta.published_time_text.is_none()
                        && let Some(pub_text) = fallback_date
                    {
                        meta.published_time_text = Some(pub_text);
                        changed = true;
                    }
                    if changed {
                        let updated = meta.clone();
                        if self.selected.as_ref().is_some_and(|v| v.url == url) {
                            self.selected_metadata = Some(updated);
                        }
                        self.watch.metadata_cache.valid = false;
                        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                    }
                }
                if !heights.is_empty() {
                    self.playback.qualities = heights.clone();
                    self.playback.quality_error = None;
                    self.playback.quality_fallback_url = None;
                }
                if storyboard.is_some() {
                    self.playback.storyboard = storyboard.clone();
                    self.playback.storyboard_video_url = video_url.clone();
                }
                if subtitles_parsed {
                    self.watch
                        .extras
                        .apply_preloaded_subtitles(subtitles.clone());
                    self.watch.overlay_cache.mpv_subs_last_poll = None;
                    self.watch.overlay_cache.force_refresh = true;
                }
                if let Some(url) = video_url.as_deref() {
                    search::prime_playback_metadata_cache(
                        url,
                        &heights,
                        &subtitles,
                        subtitles_parsed,
                        search::PlaybackExtraInfo {
                            is_live,
                            title: title.clone(),
                            channel: channel.clone(),
                            channel_id: channel_id.clone(),
                            channel_url: channel_url.clone(),
                            description: description.clone(),
                            concurrent_view_count,
                            storyboard: storyboard.clone(),
                        },
                    );
                    self.apply_live_metadata_to_video(
                        url,
                        is_live,
                        Some(is_live_dvr),
                        title.as_deref(),
                        channel.as_deref(),
                        channel_id.as_deref(),
                        channel_url.as_deref(),
                        channel_avatar_url.as_deref(),
                        subscriber_count_text.as_deref(),
                        description.as_deref(),
                        concurrent_view_count,
                        Some(ctx),
                    );
                }
                if is_live && self.watch.live.has_chat() && !self.watch.live.chat_user_toggled {
                    self.watch.live.chat_visible = true;
                }
                if self.watch.live.chat_visible && self.watch.live.stream_rx.is_none() {
                    self.start_live_chat_stream_if_needed(ctx);
                }
                if heights.is_empty()
                    && self.playback.qualities.is_empty()
                    && !self.playback.quality_task.is_running()
                    && let Some(url) = video_url
                {
                    self.playback.quality_fallback_url = Some(url);
                    self.maybe_start_deferred_quality_fallback(ctx);
                }
            }
            Err(err) => {
                self.watch.live.watch_info_error = Some(err);
                if self.playback.qualities.is_empty()
                    && !self.playback.quality_task.is_running()
                    && let Some(url) = self.watch.live.video_url.clone()
                {
                    self.playback.quality_fallback_url = Some(url);
                    self.maybe_start_deferred_quality_fallback(ctx);
                }
            }
        }
    }

    pub(crate) fn current_player_live_pos_ms(&self) -> Option<u64> {
        self.playback
            .active
            .as_ref()
            .filter(|active| active.has_live_sample)
            .map(|active| active.position_ms)
    }

    pub(crate) fn start_video_report(
        &mut self,
        video_id: String,
        reason: oauth::AbuseReportReason,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        let reason_label = reason.label().to_owned();
        self.message = format!("Enviando denúncia ({reason_label})...");
        self.auth
            .report_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| {
                        oauth::report_video_abuse(&session.access_token, &video_id, reason, &cancel)
                    })
                    .map(|()| reason_label);
                (Some(session), result)
            });
    }

    pub(crate) fn poll_report(&mut self, _ctx: &egui::Context) {
        if let Some((session, result)) = self.auth.report_task.poll() {
            let _ = self.set_oauth_session(session);
            match result {
                Ok(reason_label) => {
                    self.message = format!(
                        "Denúncia enviada ao YouTube ({reason_label}). Resultado ocultado."
                    );
                }
                Err(error) => {
                    self.message = format!(
                        "Resultado ocultado. Não foi possível enviar a denúncia ao YouTube: {error}"
                    );
                }
            }
        }
    }

    pub(crate) fn render_timestamp_lines_ui(
        ui: &mut egui::Ui,
        lines: &[watch_extras::CachedTimestampLine],
        color: Color32,
        font_size: f32,
        link_color: Color32,
    ) -> Option<f64> {
        let mut clicked_secs = None;
        for line in lines {
            if line.timestamps.is_empty() {
                ui.label(RichText::new(&line.text).color(color).size(font_size));
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut cursor = 0;
                for ts in &line.timestamps {
                    if ts.byte_start > cursor
                        && let Some(prefix) = line.text.get(cursor..ts.byte_start)
                    {
                        ui.label(RichText::new(prefix).color(color).size(font_size));
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(&ts.label)
                                    .color(link_color)
                                    .size(font_size)
                                    .strong(),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(4)),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        clicked_secs = Some(ts.seconds as f64);
                    }
                    cursor = ts.byte_end;
                }
                if cursor < line.text.len()
                    && let Some(suffix) = line.text.get(cursor..)
                {
                    ui.label(RichText::new(suffix).color(color).size(font_size));
                }
            });
        }
        clicked_secs
    }

    #[allow(dead_code)]
    pub(crate) fn render_cached_timestamp_lines_ui(
        &mut self,
        ui: &mut egui::Ui,
        lines: &[watch_extras::CachedTimestampLine],
        color: Color32,
        font_size: f32,
    ) {
        let link_color = if self.is_dark_theme() {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        if let Some(secs) = Self::render_timestamp_lines_ui(ui, lines, color, font_size, link_color)
        {
            self.seek_to_seconds(secs);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn render_text_with_timestamps_ui(
        &mut self,
        ui: &mut egui::Ui,
        text: &str,
        color: Color32,
        font_size: f32,
    ) {
        let link_color = if self.is_dark_theme() {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let mut clicked_secs = None;
        for line in text.lines() {
            let timestamps = watch_extras::extract_timestamps(line);
            if timestamps.is_empty() {
                ui.label(RichText::new(line).color(color).size(font_size));
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut cursor = 0;
                for ts in timestamps {
                    if ts.byte_start > cursor
                        && let Some(prefix) = line.get(cursor..ts.byte_start)
                    {
                        ui.label(RichText::new(prefix).color(color).size(font_size));
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(&ts.label)
                                    .color(link_color)
                                    .size(font_size)
                                    .strong(),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(4)),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        clicked_secs = Some(ts.seconds as f64);
                    }
                    cursor = ts.byte_end;
                }
                if cursor < line.len()
                    && let Some(suffix) = line.get(cursor..)
                {
                    ui.label(RichText::new(suffix).color(color).size(font_size));
                }
            });
        }
        if let Some(secs) = clicked_secs {
            self.seek_to_seconds(secs);
        }
    }

    pub(crate) fn start_community_extras_fetch(&mut self, video_url: &str, ctx: &egui::Context) {
        self.watch.community_task.cancel();
        self.watch.show_original_title = false;
        let ryd_enabled = self.settings.prefs.value.ryd_enabled;
        let dearrow_enabled = self.settings.prefs.value.dearrow_enabled;
        if !ryd_enabled && !dearrow_enabled {
            return;
        }
        let Some(video_id) = history::youtube_video_id(video_url) else {
            return;
        };
        if let Some(cached) = self.watch.community_cache.get(&video_id) {
            let needs_ryd = ryd_enabled && cached.ryd.is_none();
            let needs_dearrow = dearrow_enabled && cached.dearrow.is_none();
            if !needs_ryd && !needs_dearrow {
                return;
            }
        }
        self.watch.community_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let outcome = community_apis::fetch_community_extras(
                    &video_id,
                    ryd_enabled,
                    dearrow_enabled,
                    &cancel,
                );
                app_state::CommunityExtrasFetchMessage {
                    request_id,
                    outcome,
                }
            },
        );
    }

    pub(crate) fn poll_community_extras(&mut self) {
        if let async_task::AsyncTaskPoll::Ready(msg) = self
            .watch
            .community_task
            .poll_matching_id(|msg| msg.request_id)
        {
            let vid = msg.outcome.video_id.clone();
            self.watch.community_cache.insert(vid, msg.outcome);
            self.watch.community_revision = self.watch.community_revision.wrapping_add(1);
        }
    }

    pub(crate) fn start_video_rating(
        &mut self,
        video_id: String,
        rating: Option<oauth::VideoRating>,
        ctx: &egui::Context,
    ) {
        if self.auth.rating_task.is_running() {
            return;
        }
        let oauth_session = self.auth.session.clone();
        let web_session = Self::load_innertube_web_session_cached();
        if oauth_session.is_none() && web_session.is_none() {
            self.message = "Entre com Google para avaliar este vídeo.".into();
            self.start_reauth(ctx);
            return;
        }
        let api_rating = rating.unwrap_or(oauth::VideoRating::None);
        let it_rating = Self::to_innertube_rating(rating);
        let it_ctx = self.innertube_context();
        self.message = "Enviando avaliação...".into();
        self.auth
            .rating_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                if let Some(mut it_sess) = web_session {
                    match innertube::mutate_video_rating(
                        &video_id,
                        it_rating,
                        &it_ctx,
                        &mut it_sess,
                        &cancel,
                    ) {
                        Ok(_) => {
                            return (oauth_session, Ok((video_id, rating)));
                        }
                        Err(err) => {
                            if err.is_auth_expired() {
                                #[cfg(not(test))]
                                let _ = innertube::invalidate_web_session();
                            }
                            if oauth_session.is_none() {
                                return (None, Err(err.into_message()));
                            }
                        }
                    }
                }
                let Some(mut session) = oauth_session else {
                    return (
                        None,
                        Err("Sessão indisponível para avaliar o vídeo.".into()),
                    );
                };
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| {
                        oauth::rate_video(&session.access_token, &video_id, api_rating, &cancel)
                    })
                    .map(|()| (video_id, rating));
                (Some(session), result)
            });
    }

    pub(crate) fn poll_rating(&mut self) {
        match self.auth.rating_task.poll_event() {
            async_task::AsyncTaskPoll::Ready((updated_session, result)) => {
                if let Some(session) = updated_session
                    && self.auth.session.is_some()
                {
                    let _ = self.set_oauth_session(Some(session));
                }
                match result {
                    Ok((video_id, rating)) => {
                        if rating == Some(oauth::VideoRating::Like) {
                            if self.library.liked_videos.rx.is_none() {
                                self.library.liked_videos.loaded = false;
                            }
                        } else {
                            let before_len = self.library.liked_videos.items.len();
                            self.library.liked_videos.items.retain(|item| {
                                history::youtube_video_id(&item.video.url).as_deref()
                                    != Some(video_id.as_str())
                            });
                            let removed = before_len - self.library.liked_videos.items.len();
                            if removed > 0 {
                                self.library.liked_videos.seen_keys.remove(&video_id);
                                if let Some(total) = &mut self.library.liked_videos.total_results {
                                    *total = total.saturating_sub(removed as u64);
                                }
                                self.library.liked_videos.revision =
                                    self.library.liked_videos.revision.wrapping_add(1);
                            }
                        }
                        if let Some(rating) = rating {
                            self.auth.ratings.insert(video_id, rating);
                            self.message = match rating {
                                oauth::VideoRating::Like => "Você gostou deste vídeo.".into(),
                                oauth::VideoRating::Dislike => {
                                    "Você não gostou deste vídeo.".into()
                                }
                                oauth::VideoRating::None => "Avaliação removida.".into(),
                            };
                        } else {
                            self.auth.ratings.remove(&video_id);
                            self.message = "Avaliação removida.".into();
                        }
                    }
                    Err(error) => {
                        self.message = error;
                    }
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.message = "A avaliação foi interrompida.".into();
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_edit_comment(
        &mut self,
        comment_id: String,
        new_text: String,
        ctx: &egui::Context,
    ) {
        let trimmed = new_text.trim().to_owned();
        if trimmed.is_empty() {
            return;
        }
        for comment in &mut self.watch.extras.comments {
            if comment.id == comment_id {
                comment.text = trimmed.clone();
            }
        }
        for replies in self.watch.extras.comment_replies.values_mut() {
            for reply in &mut replies.comments {
                if reply.id == comment_id {
                    reply.text = trimmed.clone();
                }
            }
            replies.render_cache.clear();
        }
        self.watch.extras.comment_render_cache.clear();

        self.watch.comment_mutation_task.cancel();
        let pt = self.ui_pt();
        if cfg!(test) {
            self.toasts.success(if pt {
                "Coment\u{00e1}rio atualizado."
            } else {
                "Comment updated."
            });
            return;
        }
        let Some(mut session) = self.resolve_active_web_session() else {
            self.toasts.success(if pt {
                "Coment\u{00e1}rio atualizado localmente."
            } else {
                "Comment updated locally."
            });
            return;
        };
        let cid = comment_id.clone();
        let text_clone = trimmed.clone();
        self.watch.comment_mutation_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result =
                    comment_actions::edit_comment(&mut session, &cid, &text_clone, &cancel)
                        .map(|_| ())
                        .map_err(|e| e.into_message());
                app_state::CommentMutationFetchMessage {
                    request_id,
                    kind: app_state::CommentMutationKind::Edit,
                    result,
                }
            },
        );
    }

    pub(crate) fn start_delete_comment(&mut self, comment_id: String, ctx: &egui::Context) {
        self.watch.extras.comments.retain(|c| c.id != comment_id);
        self.watch.extras.comment_render_cache.clear();
        for replies in self.watch.extras.comment_replies.values_mut() {
            replies.comments.retain(|c| c.id != comment_id);
            replies.render_cache.clear();
        }

        self.watch.comment_mutation_task.cancel();
        let pt = self.ui_pt();
        if cfg!(test) {
            self.toasts.success(if pt {
                "Coment\u{00e1}rio exclu\u{00ed}do."
            } else {
                "Comment deleted."
            });
            return;
        }
        let oauth_token = self
            .auth
            .session
            .as_ref()
            .map(|s| s.access_token.clone())
            .filter(|t| !t.trim().is_empty());
        let web_session = self.resolve_active_web_session();
        if oauth_token.is_none() && web_session.is_none() {
            self.toasts.success(if pt {
                "Coment\u{00e1}rio removido localmente."
            } else {
                "Comment removed locally."
            });
            return;
        }
        let cid = comment_id.clone();
        self.watch.comment_mutation_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result = if let Some(token) = oauth_token {
                    comment_actions::delete_comment_oauth(&token, &cid, &cancel)
                        .map_err(|e| e.into_message())
                } else if let Some(mut session) = web_session {
                    comment_actions::delete_comment(&mut session, &cid, &cancel)
                        .map(|_| ())
                        .map_err(|e| e.into_message())
                } else {
                    Ok(())
                };
                app_state::CommentMutationFetchMessage {
                    request_id,
                    kind: app_state::CommentMutationKind::Delete,
                    result,
                }
            },
        );
    }

    pub(crate) fn poll_comment_mutation(&mut self) {
        if let async_task::AsyncTaskPoll::Ready(msg) = self
            .watch
            .comment_mutation_task
            .poll_matching_id(|msg| msg.request_id)
        {
            let pt = self.ui_pt();
            match msg.result {
                Ok(()) => {
                    let toast = match msg.kind {
                        app_state::CommentMutationKind::Edit => {
                            if pt {
                                "Coment\u{00e1}rio editado."
                            } else {
                                "Comment updated."
                            }
                        }
                        app_state::CommentMutationKind::Delete => {
                            if pt {
                                "Coment\u{00e1}rio exclu\u{00ed}do."
                            } else {
                                "Comment deleted."
                            }
                        }
                    };
                    self.toasts.success(toast);
                }
                Err(err) => {
                    self.toasts.warning(err);
                }
            }
        }
    }

    pub(crate) fn default_report_history_path() -> io::Result<PathBuf> {
        let history_path = history::history_path()?;
        Ok(history_path.with_file_name("report-history.json"))
    }

    pub(crate) fn load_report_history_entries_from(
        path: &Path,
    ) -> io::Result<Vec<app_state::ReportedContentEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(path)?;
        let records: Vec<PersistedVideoReportRecord> = serde_json::from_str(&raw)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        Ok(records
            .into_iter()
            .map(|r| app_state::ReportedContentEntry {
                kind: "V\u{00ed}deo".to_owned(),
                title: r.title,
                channel: String::new(),
                video_id: (!r.video_id.is_empty()).then_some(r.video_id),
                reason: r.reason,
                timestamp_label: format!("{}", r.submitted_at_unix),
                status: "Registrado".to_owned(),
            })
            .collect())
    }

    pub(crate) fn append_report_history_record_at(
        path: &Path,
        record: PersistedVideoReportRecord,
    ) -> io::Result<()> {
        let mut records: Vec<PersistedVideoReportRecord> = if path.exists() {
            let raw = std::fs::read_to_string(path)?;
            serde_json::from_str(&raw)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?
        } else {
            Vec::new()
        };
        records.push(record);
        let bytes = serde_json::to_vec_pretty(&records)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        history::write_atomic(path, &bytes)
    }

    pub(crate) fn submit_video_report(
        &mut self,
        reason: String,
        details: String,
        ctx: &egui::Context,
    ) {
        let pt = self.ui_pt();
        let reason_trimmed = reason.trim().to_owned();
        if reason_trimmed.is_empty() {
            return;
        }
        let details_trimmed: String = details.trim().chars().take(1000).collect();
        let (video_id, url, title, channel) = if let Some(video) = self.selected.as_ref() {
            (
                history::youtube_video_id(&video.url).unwrap_or_default(),
                video.url.clone(),
                video.title.clone(),
                video.channel.clone(),
            )
        } else {
            (String::new(), String::new(), String::new(), String::new())
        };
        let submitted_at_unix = (history::current_unix_epoch_ms() / 1000) as i64;
        let record = PersistedVideoReportRecord {
            video_id: video_id.clone(),
            url: url.clone(),
            title: title.clone(),
            reason: reason_trimmed.clone(),
            details: details_trimmed,
            submitted_at_unix,
        };
        let persist_path = self.feed.report_history_path.clone().or_else(|| {
            if cfg!(test) {
                None
            } else {
                Self::default_report_history_path().ok()
            }
        });
        let mut persist_error: Option<String> = None;
        if let Some(path) = persist_path
            && let Err(err) = Self::append_report_history_record_at(&path, record)
        {
            persist_error = Some(if pt {
                format!(
                    "Arquivo report-history.json corrompido ou inacess\u{00ed}vel ({err}); preservado sem sobrescrita."
                )
            } else {
                format!(
                    "report-history.json is corrupt or unreadable ({err}); preserved untouched on disk."
                )
            });
        }
        self.feed.report_history.insert(
            0,
            app_state::ReportedContentEntry {
                kind: if pt {
                    "V\u{00ed}deo".to_owned()
                } else {
                    "Video".to_owned()
                },
                title: if title.is_empty() { url.clone() } else { title },
                channel,
                video_id: (!video_id.is_empty()).then_some(video_id.clone()),
                reason: reason_trimmed.clone(),
                timestamp_label: if pt {
                    "Agora".to_owned()
                } else {
                    "Just now".to_owned()
                },
                status: if pt {
                    "Registrado".to_owned()
                } else {
                    "Recorded".to_owned()
                },
            },
        );
        if let Some(err) = persist_error {
            self.toasts.error(err);
        } else {
            self.toasts.success(if pt {
                format!("Den\u{00fa}ncia registrada ({reason_trimmed}).")
            } else {
                format!("Report recorded ({reason_trimmed}).")
            });
        }
        if !cfg!(test) && self.auth.authenticated && !video_id.is_empty() {
            let lower = reason_trimmed.to_lowercase();
            let abuse_reason = if lower.contains("sex") {
                oauth::AbuseReportReason::SexualContent
            } else if lower.contains("violen") || lower.contains("repuls") {
                oauth::AbuseReportReason::ViolentOrRepulsive
            } else if lower.contains("hate") || lower.contains("abus") || lower.contains("ódio") {
                oauth::AbuseReportReason::HatefulOrAbusive
            } else if lower.contains("harm") || lower.contains("danger") || lower.contains("perig")
            {
                oauth::AbuseReportReason::HarmfulOrDangerous
            } else {
                oauth::AbuseReportReason::SpamOrMisleading
            };
            self.start_video_report(video_id, abuse_reason, ctx);
        }
    }

    pub(crate) fn poll_live_chat_replay(&mut self, ctx: &egui::Context) {
        match self
            .watch
            .replay_chat_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                if msg.session_id != self.watch.live.session_id {
                    return;
                }
                self.watch.live.chat_loading = false;
                match msg.result {
                    Ok(batch) => {
                        self.watch.live.chat_error = None;
                        self.watch.live.apply_batch(batch);
                    }
                    Err(err) => {
                        self.watch.live.chat_error = Some(err);
                    }
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.watch.live.chat_loading = false;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }

        if cfg!(test)
            || self.active_video_is_live()
            || !self.watch.live.chat_visible
            || self.watch.live.stream_rx.is_some()
            || self.watch.replay_chat_task.is_running()
        {
            return;
        }
        let Some(continuation) = self.watch.live.active_continuation.clone() else {
            return;
        };
        if self
            .watch
            .replay_chat_last_poll_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(4))
        {
            return;
        }
        let player_offset_ms = self.playback.active.as_ref().map_or(0, |a| a.position_ms);
        let session_id = self.watch.live.session_id;
        self.watch.replay_chat_last_poll_at = Some(Instant::now());
        self.watch.live.chat_loading = true;
        let version = if self.watch.live.client_version.is_empty() {
            comment_actions::FALLBACK_CLIENT_VERSION.to_owned()
        } else {
            self.watch.live.client_version.clone()
        };
        let visitor = self.watch.live.visitor_data.clone();
        let mut session = self.resolve_active_web_session();
        self.watch.replay_chat_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result = live_chat::fetch_live_chat_replay_page(
                    &continuation,
                    player_offset_ms,
                    &version,
                    visitor.as_deref(),
                    session.as_mut(),
                    &cancel,
                )
                .map_err(|e| e.into_message());
                app_state::LiveChatReplayFetchMessage {
                    request_id,
                    session_id,
                    result,
                }
            },
        );
    }

    pub(crate) fn to_innertube_rating(
        rating: Option<oauth::VideoRating>,
    ) -> innertube::VideoRating {
        match rating {
            Some(oauth::VideoRating::Like) => innertube::VideoRating::Like,
            Some(oauth::VideoRating::Dislike) => innertube::VideoRating::Dislike,
            Some(oauth::VideoRating::None) | None => innertube::VideoRating::Remove,
        }
    }

    pub(crate) fn start_watch_next_fetch(
        &mut self,
        video_url: &str,
        chip_index: Option<usize>,
        continuation: Option<String>,
        ctx: &egui::Context,
    ) {
        let video_id = history::youtube_video_id(video_url).unwrap_or_default();
        if video_id.is_empty() && continuation.is_none() {
            return;
        }
        let is_chip_reload = chip_index.is_some() || continuation.is_some();
        let it_ctx = self.innertube_context();
        let mut web_session = Self::load_innertube_web_session_cached();
        let video_url_owned = video_url.to_owned();
        self.watch
            .next_task
            .spawn_with_id_and_cancel(ctx.clone(), move |request_id, cancel| {
                let outcome = innertube::fetch_watch_next(
                    &video_id,
                    continuation.as_deref(),
                    &it_ctx,
                    web_session.as_mut(),
                    &cancel,
                );
                app_state::WatchNextFetchMessage {
                    request_id,
                    video_url: video_url_owned,
                    chip_index,
                    is_chip_reload,
                    result: outcome.map_err(innertube::InnerTubeError::into_message),
                }
            });
    }

    #[cfg(test)]
    pub(crate) fn complete_watch_next(
        &mut self,
        msg: app_state::WatchNextFetchMessage,
        ctx: &egui::Context,
    ) -> bool {
        if self.watch.next_task.active_request_id() != Some(msg.request_id) {
            return false;
        }
        self.watch.next_task.cancel();
        self.apply_watch_next(msg, ctx)
    }

    pub(crate) fn apply_watch_next(
        &mut self,
        msg: app_state::WatchNextFetchMessage,
        ctx: &egui::Context,
    ) -> bool {
        if self
            .selected
            .as_ref()
            .is_none_or(|v| v.url != msg.video_url)
            && self.watch.extras.video_url.as_deref() != Some(msg.video_url.as_str())
        {
            return false;
        }
        if let Ok(next) = msg.result {
            let mut meta_changed = false;
            let full_desc: Option<String> = (!next.description_runs.is_empty())
                .then(|| {
                    next.description_runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>()
                })
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            {
                let meta = self
                    .video_metadata
                    .entry(msg.video_url.clone())
                    .or_default();
                if next.channel_verified && !meta.channel_verified {
                    meta.channel_verified = true;
                    meta_changed = true;
                }
                if meta.channel_id.is_none()
                    && let Some(cid) = next.channel_id.clone()
                {
                    meta.channel_id = Some(cid);
                    meta_changed = true;
                }
                if meta.channel_url.is_none()
                    && let Some(curl) = next.channel_url.clone()
                {
                    meta.channel_url = Some(curl);
                    meta_changed = true;
                }
                if meta.channel_avatar_url.is_none()
                    && let Some(av) = next.channel_avatar_url.clone()
                {
                    meta.channel_avatar_url = Some(av);
                    meta_changed = true;
                }
                if meta.subscriber_count_text.is_none()
                    && let Some(subs) = next.subscriber_count_text.clone()
                {
                    meta.subscriber_count_text = Some(subs);
                    meta_changed = true;
                }
                if meta.view_count_text.is_none()
                    && let Some(vtext) = next.view_count_text.clone()
                {
                    if meta.view_count.is_none() {
                        meta.view_count = search::parse_pt_br_compact_count(&vtext);
                    }
                    meta.view_count_text = Some(vtext);
                    meta_changed = true;
                }
                if meta.published_time_text.is_none()
                    && let Some(pub_text) = next.published_date_text.clone()
                {
                    meta.published_time_text = Some(pub_text);
                    meta_changed = true;
                }
                if let Some(desc) = full_desc
                    && meta
                        .description
                        .as_deref()
                        .is_none_or(|cur| desc.len() > cur.trim().len())
                {
                    meta.description = Some(desc);
                    meta_changed = true;
                }
                if meta_changed
                    && self
                        .selected
                        .as_ref()
                        .is_some_and(|v| v.url == msg.video_url)
                {
                    self.selected_metadata = Some(meta.clone());
                }
            }
            let avatar_channel = next
                .channel_title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    self.selected
                        .as_ref()
                        .filter(|v| v.url == msg.video_url)
                        .map(|v| v.channel.trim().to_owned())
                        .filter(|s| !s.is_empty())
                });
            if let Some(ch_title) = next
                .channel_title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                && let Some(sel) = self.selected.as_mut().filter(|v| v.url == msg.video_url)
                && sel.channel.trim().is_empty()
            {
                sel.channel = ch_title.to_owned();
            }
            if let (Some(ch), Some(av)) = (
                avatar_channel.as_deref(),
                next.channel_avatar_url.as_deref(),
            ) {
                self.cache_channel_avatar(ch, Some(av));
            }
            for (rel_video, rel_meta) in
                next.related_videos.iter().zip(next.related_metadata.iter())
            {
                self.video_metadata
                    .entry(rel_video.url.clone())
                    .and_modify(|existing| existing.merge_missing_from(rel_meta))
                    .or_insert_with(|| rel_meta.clone());
            }
            let comments_cont = next.comments_continuation.clone();
            self.watch.extras.apply_watch_next(next);
            if msg.is_chip_reload {
                self.watch.extras.selected_related_chip = msg.chip_index;
            }
            self.watch.sidebar_cache.valid = false;
            self.watch.metadata_cache.valid = false;
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            if !msg.is_chip_reload
                && !self.watch.extras.comments_loaded
                && !self.watch.extras.innertube_comments_loading
                && self.watch.extras.comments_loading.is_none()
                && let Some(cont) = comments_cont
                    .or_else(|| self.watch.extras.innertube_comments_continuation.clone())
            {
                self.start_load_innertube_comments(cont, true, ctx);
            }
        }
        true
    }

    pub(crate) fn poll_watch_next(&mut self, ctx: &egui::Context) {
        if let async_task::AsyncTaskPoll::Ready(msg) =
            self.watch.next_task.poll_matching_id(|msg| msg.request_id)
        {
            let _ = self.apply_watch_next(msg, ctx);
        }
    }

    pub(crate) fn select_watch_related_chip(
        &mut self,
        index: Option<usize>,
        continuation: Option<String>,
        ctx: &egui::Context,
    ) {
        self.watch.extras.selected_related_chip = index;
        for (i, chip) in self.watch.extras.related_chips.iter_mut().enumerate() {
            chip.selected = Some(i) == index;
        }
        self.watch.sidebar_cache.valid = false;
        self.watch.extras.cached_sidebar_related_key = None;
        if let Some(video_url) = self
            .selected
            .as_ref()
            .map(|v| v.url.clone())
            .or_else(|| self.watch.extras.video_url.clone())
        {
            self.start_watch_next_fetch(&video_url, index, continuation, ctx);
        }
    }

    pub(crate) fn start_load_innertube_comments(
        &mut self,
        continuation: String,
        reset: bool,
        ctx: &egui::Context,
    ) {
        if continuation.trim().is_empty() {
            return;
        }
        let Some(video_url) = self
            .watch
            .extras
            .video_url
            .clone()
            .or_else(|| self.selected.as_ref().map(|v| v.url.clone()))
        else {
            return;
        };
        self.watch.extras.innertube_comments_loading = true;
        self.watch.extras.comments_error = None;
        let it_ctx = self.innertube_context();
        let mut web_session = Self::load_innertube_web_session_cached();
        self.watch.innertube_comments_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let outcome = innertube::fetch_comments_next(
                    &continuation,
                    &it_ctx,
                    web_session.as_mut(),
                    &cancel,
                );
                app_state::InnerTubeCommentsFetchMessage {
                    request_id,
                    video_url,
                    append: !reset,
                    result: outcome.map_err(innertube::InnerTubeError::into_message),
                }
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn complete_innertube_comments(
        &mut self,
        msg: app_state::InnerTubeCommentsFetchMessage,
    ) -> bool {
        if self.watch.innertube_comments_task.active_request_id() != Some(msg.request_id) {
            return false;
        }
        self.watch.innertube_comments_task.cancel();
        self.apply_innertube_comments(msg)
    }

    pub(crate) fn apply_innertube_comments(
        &mut self,
        msg: app_state::InnerTubeCommentsFetchMessage,
    ) -> bool {
        if self.watch.extras.video_url.as_deref() != Some(msg.video_url.as_str()) {
            self.watch.extras.innertube_comments_loading = false;
            return false;
        }
        match msg.result {
            Ok(page) => {
                self.watch
                    .extras
                    .apply_innertube_comments_page(page, msg.append);
            }
            Err(error) => {
                self.watch.extras.innertube_comments_loading = false;
                self.watch.extras.comments_error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_innertube_comments(&mut self) {
        match self
            .watch
            .innertube_comments_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                let _ = self.apply_innertube_comments(msg);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.watch.extras.innertube_comments_loading = false;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    #[allow(dead_code)]
    pub(crate) fn start_zero_quota_feedback(
        &mut self,
        video_id: String,
        feedback_token: Option<String>,
        not_interested: bool,
        ctx: &egui::Context,
    ) {
        if not_interested && !video_id.trim().is_empty() {
            let url = format!("https://www.youtube.com/watch?v={}", video_id.trim());
            self.feed.search_reported_urls.insert(url.clone());
            self.feed.videos.retain(|v| {
                v.url != url
                    && history::youtube_video_id(&v.url).as_deref() != Some(video_id.trim())
            });
            self.feed.generation = self.feed.generation.wrapping_add(1);
            self.feed.cache_dirty = true;
        }
        let Some(token) = feedback_token
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
        else {
            self.message = self
                .pick(
                    if not_interested {
                        "Vídeo ocultado das recomendações."
                    } else {
                        "Preferência de canal registrada."
                    },
                    if not_interested {
                        "Video hidden from recommendations."
                    } else {
                        "Channel preference saved."
                    },
                )
                .into();
            return;
        };
        let Some(mut web_session) = Self::load_innertube_web_session_cached() else {
            return;
        };
        let it_ctx = self.innertube_context();
        self.watch.feedback_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let outcome =
                    innertube::mutate_feedback(&token, &it_ctx, &mut web_session, &cancel);
                if let Err(ref err) = outcome
                    && err.is_auth_expired()
                {
                    #[cfg(not(test))]
                    let _ = innertube::invalidate_web_session();
                }
                app_state::ZeroQuotaFeedbackMessage {
                    request_id,
                    not_interested,
                    result: outcome
                        .map(|_| ())
                        .map_err(innertube::InnerTubeError::into_message),
                }
            },
        );
    }

    pub(crate) fn poll_zero_quota_feedback(&mut self) {
        if let async_task::AsyncTaskPoll::Ready(msg) = self
            .watch
            .feedback_task
            .poll_matching_id(|msg| msg.request_id)
        {
            match msg.result {
                Ok(()) => {
                    self.message = self
                        .pick(
                            if msg.not_interested {
                                "Feedback \"Não tenho interesse\" enviado ao YouTube."
                            } else {
                                "Feedback de recomendação enviado ao YouTube."
                            },
                            if msg.not_interested {
                                "\"Not interested\" feedback sent to YouTube."
                            } else {
                                "Recommendation feedback sent to YouTube."
                            },
                        )
                        .into();
                }
                Err(err) => {
                    self.message = err;
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PersistedVideoReportRecord {
    pub(crate) video_id: String,
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) reason: String,
    pub(crate) details: String,
    pub(crate) submitted_at_unix: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareTargetKind {
    Embed,
    WhatsApp,
    X,
    Messages,
    Facebook,
    Email,
    Reddit,
    LinkedIn,
}

impl ShareTargetKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Embed => "Incorporar",
            Self::WhatsApp => "WhatsApp",
            Self::X => "X",
            Self::Messages => "Mensagens",
            Self::Facebook => "Facebook",
            Self::Email => "E-mail",
            Self::Reddit => "Reddit",
            Self::LinkedIn => "LinkedIn",
        }
    }

    pub(crate) fn launch_url(self, share_url: &str, title: &str) -> String {
        let encoded_url = encode_uri_component(share_url);
        let encoded_title = encode_uri_component(title);
        match self {
            Self::Embed => String::new(),
            Self::WhatsApp => format!("https://api.whatsapp.com/send?text={encoded_url}"),
            Self::X => {
                format!("https://twitter.com/intent/tweet?url={encoded_url}&text={encoded_title}")
            }
            Self::Messages => format!("sms:?body={encoded_url}"),
            Self::Facebook => format!("https://www.facebook.com/sharer/sharer.php?u={encoded_url}"),
            Self::Email => {
                format!("mailto:?subject={encoded_title}&body={encoded_url}")
            }
            Self::Reddit => {
                format!("https://www.reddit.com/submit?url={encoded_url}&title={encoded_title}")
            }
            Self::LinkedIn => {
                format!("https://www.linkedin.com/sharing/share-offsite/?url={encoded_url}")
            }
        }
    }
}

pub(crate) fn paint_share_target_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    kind: ShareTargetKind,
    dark_theme: bool,
    hovered: bool,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    let bg = match kind {
        ShareTargetKind::Embed => Color32::from_rgb(241, 241, 241),
        ShareTargetKind::WhatsApp => Color32::from_rgb(37, 211, 102),
        ShareTargetKind::X => Color32::BLACK,
        ShareTargetKind::Messages => Color32::WHITE,
        ShareTargetKind::Facebook => Color32::from_rgb(24, 119, 242),
        ShareTargetKind::Email => Color32::from_rgb(117, 117, 117),
        ShareTargetKind::Reddit => Color32::from_rgb(255, 69, 0),
        ShareTargetKind::LinkedIn => Color32::from_rgb(10, 102, 194),
    };
    painter.circle_filled(center, radius, bg);
    if hovered {
        painter.circle_filled(
            center,
            radius,
            Color32::from_rgba_unmultiplied(255, 255, 255, 28),
        );
    }
    if (kind == ShareTargetKind::X && dark_theme)
        || (kind == ShareTargetKind::Messages && !dark_theme)
    {
        let border = if dark_theme {
            Color32::from_rgb(66, 66, 66)
        } else {
            Color32::from_rgb(218, 218, 218)
        };
        painter.circle_stroke(center, radius, egui::Stroke::new(1.0, border));
    }

    match kind {
        ShareTargetKind::Embed => {
            let stroke = egui::Stroke::new(2.0, Color32::from_rgb(24, 24, 24));
            painter.line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y - 6.0),
                    egui::pos2(center.x - 11.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 11.0, center.y),
                    egui::pos2(center.x - 5.0, center.y + 6.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 5.0, center.y - 6.0),
                    egui::pos2(center.x + 11.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 11.0, center.y),
                    egui::pos2(center.x + 5.0, center.y + 6.0),
                ],
                stroke,
            );
        }
        ShareTargetKind::WhatsApp => {
            let white = egui::Stroke::new(2.2, Color32::WHITE);
            painter.circle_stroke(center, 12.0, white);
            painter.line_segment(
                [
                    egui::pos2(center.x - 8.5, center.y + 8.5),
                    egui::pos2(center.x - 12.5, center.y + 13.0),
                ],
                white,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 12.5, center.y + 13.0),
                    egui::pos2(center.x - 6.5, center.y + 11.0),
                ],
                white,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - 4.0, center.y - 4.0),
                    egui::pos2(center.x + 4.5, center.y + 4.5),
                ],
                egui::Stroke::new(2.4, Color32::WHITE),
            );
        }
        ShareTargetKind::X => {
            let stroke = egui::Stroke::new(2.2, Color32::WHITE);
            painter.line_segment(
                [
                    egui::pos2(center.x - 9.0, center.y - 9.0),
                    egui::pos2(center.x + 9.0, center.y + 9.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + 9.0, center.y - 9.0),
                    egui::pos2(center.x - 9.0, center.y + 9.0),
                ],
                stroke,
            );
        }
        ShareTargetKind::Messages => {
            let back = egui::Rect::from_center_size(
                egui::pos2(center.x + 2.0, center.y + 1.0),
                egui::vec2(22.0, 17.0),
            );
            painter.rect_filled(back, 6.0, Color32::from_rgb(66, 133, 244));
            let front = egui::Rect::from_center_size(
                egui::pos2(center.x - 2.0, center.y - 2.0),
                egui::vec2(20.0, 15.0),
            );
            painter.rect_filled(front, 6.0, Color32::from_rgb(26, 115, 232));
        }
        ShareTargetKind::Facebook => {
            painter.text(
                egui::pos2(center.x, center.y + 1.0),
                egui::Align2::CENTER_CENTER,
                "f",
                egui::FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
        ShareTargetKind::Email => {
            let env = egui::Rect::from_center_size(center, egui::vec2(24.0, 17.0));
            let stroke = egui::Stroke::new(1.8, Color32::WHITE);
            painter.rect_stroke(env, 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [env.left_top(), egui::pos2(center.x, center.y + 2.0)],
                stroke,
            );
            painter.line_segment(
                [env.right_top(), egui::pos2(center.x, center.y + 2.0)],
                stroke,
            );
        }
        ShareTargetKind::Reddit => {
            painter.circle_filled(egui::pos2(center.x, center.y + 2.0), 9.0, Color32::WHITE);
            painter.circle_filled(egui::pos2(center.x - 3.5, center.y + 1.5), 1.6, bg);
            painter.circle_filled(egui::pos2(center.x + 3.5, center.y + 1.5), 1.6, bg);
            painter.line_segment(
                [
                    egui::pos2(center.x, center.y - 7.0),
                    egui::pos2(center.x + 3.5, center.y - 12.0),
                ],
                egui::Stroke::new(1.8, Color32::WHITE),
            );
            painter.circle_filled(
                egui::pos2(center.x + 6.0, center.y - 11.0),
                2.3,
                Color32::WHITE,
            );
        }
        ShareTargetKind::LinkedIn => {
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "in",
                egui::FontId::proportional(20.0),
                Color32::WHITE,
            );
        }
    }
}

pub(crate) fn open_external_url(url: &str) {
    #[cfg(all(windows, not(test)))]
    {
        let _ = std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
    }
    #[cfg(not(all(windows, not(test))))]
    {
        let _ = url;
    }
}

fn encode_uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(*byte as char);
        } else {
            out.push('%');
            let hi = (byte >> 4) & 0x0f;
            let lo = byte & 0x0f;
            out.push(
                char::from_digit(u32::from(hi), 16)
                    .unwrap_or('0')
                    .to_ascii_uppercase(),
            );
            out.push(
                char::from_digit(u32::from(lo), 16)
                    .unwrap_or('0')
                    .to_ascii_uppercase(),
            );
        }
    }
    out
}

pub(crate) fn parse_share_timestamp_secs(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.len() {
        1 => parts[0].trim().parse::<u64>().ok(),
        2 => {
            let mins = parts[0].trim().parse::<u64>().ok()?;
            let secs = parts[1].trim().parse::<u64>().ok()?;
            Some(mins.saturating_mul(60).saturating_add(secs))
        }
        3 => {
            let hours = parts[0].trim().parse::<u64>().ok()?;
            let mins = parts[1].trim().parse::<u64>().ok()?;
            let secs = parts[2].trim().parse::<u64>().ok()?;
            Some(
                hours
                    .saturating_mul(3600)
                    .saturating_add(mins.saturating_mul(60))
                    .saturating_add(secs),
            )
        }
        _ => None,
    }
}

pub(crate) fn format_share_timestamp(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

pub(crate) fn build_youtube_share_url(video_url: &str, start_secs: Option<u64>) -> String {
    if let Some(id) = history::youtube_video_id(video_url) {
        if let Some(secs) = start_secs.filter(|&s| s > 0) {
            format!("https://youtu.be/{id}?t={secs}")
        } else {
            format!("https://youtu.be/{id}")
        }
    } else {
        video_url.to_owned()
    }
}

pub(crate) fn build_youtube_embed_code(
    video_id: &str,
    title: &str,
    start_secs: Option<u64>,
) -> String {
    let escaped_title = title
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let src = if let Some(secs) = start_secs.filter(|&s| s > 0) {
        format!("https://www.youtube.com/embed/{video_id}?start={secs}")
    } else {
        format!("https://www.youtube.com/embed/{video_id}")
    };
    format!(
        "<iframe width=\"560\" height=\"315\" src=\"{src}\" title=\"{escaped_title}\" frameborder=\"0\" allow=\"accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share\" referrerpolicy=\"strict-origin-when-cross-origin\" allowfullscreen></iframe>"
    )
}

pub(crate) fn format_subscribers_pt(count: u64) -> String {
    if count == 1 {
        "1 inscrito".to_owned()
    } else if count < 1_000 {
        format!("{count} inscritos")
    } else if count < 1_000_000 {
        let tenths = count / 100;
        let whole = tenths / 10;
        let frac = tenths % 10;
        if frac == 0 {
            format!("{whole} mil inscritos")
        } else {
            format!("{whole},{frac} mil inscritos")
        }
    } else {
        let tenths = count / 100_000;
        let whole = tenths / 10;
        let frac = tenths % 10;
        if frac == 0 {
            format!("{whole} mi inscritos")
        } else {
            format!("{whole},{frac} mi inscritos")
        }
    }
}
