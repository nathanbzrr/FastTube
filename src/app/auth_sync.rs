use crate::*;

impl FastTube {
    pub(crate) fn load_initial_history() -> (
        History,
        Option<PathBuf>,
        Option<history::FileFingerprint>,
        bool,
        Option<String>,
    ) {
        match history::history_path() {
            Ok(path) => match app_state::load_history_with_fingerprint_from_path(&path) {
                Ok((loaded, fingerprint)) => (loaded, Some(path), fingerprint, false, None),
                Err(error) => (History::default(), Some(path), None, true, Some(error)),
            },
            Err(error) => (
                History::default(),
                None,
                None,
                false,
                Some(format!(
                    "Não foi possível localizar o arquivo de histórico ({error}). O histórico funcionará apenas nesta sessão."
                )),
            ),
        }
    }

    pub(crate) fn ensure_history_synchronized(&mut self) {
        if self.history.ensure_synchronized(&self.persistence) {
            self.invalidate_feed_cache();
        }
    }

    pub(crate) fn save_history_to_disk(&mut self, now: Instant) -> bool {
        let (saved, invalidated) =
            self.history
                .save_to_disk(&self.persistence, now, self.playback.active.as_ref());
        if invalidated {
            self.invalidate_feed_cache();
        }
        saved
    }

    pub(crate) fn should_persist_playback_now(&self, now: Instant, force: bool) -> bool {
        self.history
            .should_persist_now(self.playback.active.as_ref(), now, force)
    }

    pub(crate) fn flush_playback_history_at(&mut self, now: Instant) {
        self.ensure_history_synchronized();
        self.record_active_playback_in_memory();
        if self.should_persist_playback_now(now, true) {
            self.save_history_to_disk(now);
        }
    }

    #[cfg(test)]
    pub(crate) fn record_video_in_history(
        &mut self,
        video: &Video,
        position_ms: u64,
        duration_ms: Option<u64>,
    ) -> io::Result<()> {
        if self.settings.prefs.value.pause_watch_history {
            return Ok(());
        }
        let (result, invalidated) =
            self.history
                .record_video(video, position_ms, duration_ms, &self.persistence);
        if invalidated {
            self.invalidate_feed_cache();
        }
        result
    }

    pub(crate) fn clear_history(&mut self) {
        self.clear_history_inner(false, None);
    }

    pub(crate) fn clear_history_with_remote(&mut self, also_remote: bool, ctx: &egui::Context) {
        self.clear_history_inner(also_remote, Some(ctx));
    }

    fn clear_history_inner(&mut self, also_remote: bool, ctx: Option<&egui::Context>) {
        self.ensure_history_synchronized();
        self.history.data.clear();
        self.invalidate_feed_cache();
        self.history.dirty = false;
        self.history.save.last_position_ms = None;
        self.history.save.last_duration_ms = None;
        self.history.remote.feeds.clear();
        self.history.remote.feedback_tokens.clear();
        self.history.remote.shorts_shelf_offsets.clear();
        self.history.remote.confirm_remote_clear = false;
        if also_remote
            && !cfg!(test)
            && let Some(token) = self.history.remote.actions.clear_feedback_token.take()
            && let Some(ctx) = ctx
        {
            self.dispatch_history_feedback(token, ctx);
        }
        if let Some(active) = &mut self.playback.active {
            active.resume_target_ms = None;
            active.resume_seek_pending = false;
        }
        if self.history.read_failed {
            self.history.notice = Some(
                "Histórico da sessão limpo. O arquivo corrompido em disco foi preservado sem sobrescrita."
                    .into(),
            );
            return;
        }
        if self.history.path.is_some() {
            if self.save_history_to_disk(Instant::now()) {
                self.history.notice = Some("Histórico local limpo.".into());
            }
        } else {
            self.history.notice = Some("Histórico local limpo.".into());
        }
    }

    pub(crate) fn remove_history_entry(&mut self, video_id: &str) -> bool {
        self.remove_history_entry_with_remote(video_id, None)
    }

    pub(crate) fn remove_history_entry_with_remote(
        &mut self,
        video_id: &str,
        ctx: Option<&egui::Context>,
    ) -> bool {
        self.ensure_history_synchronized();
        let removed_local = self.history.data.remove(video_id);
        let mut removed_remote = false;
        for feed in self.history.remote.feeds.values_mut() {
            feed.seen_ids.remove(video_id);
            for section in &mut feed.sections {
                let prev = section.items.len();
                section.items.retain(|item| item.video_id != video_id);
                if section.items.len() != prev {
                    removed_remote = true;
                }
            }
            feed.sections.retain(|section| !section.items.is_empty());
        }
        if !removed_local && !removed_remote {
            return false;
        }
        if let Some(token) = self.history.remote.feedback_tokens.remove(video_id)
            && !cfg!(test)
            && let Some(ctx) = ctx
        {
            self.dispatch_history_feedback(token, ctx);
        }
        self.invalidate_feed_cache();
        if removed_local && !self.history.read_failed && self.history.path.is_some() {
            self.save_history_to_disk(Instant::now());
        }
        true
    }

    pub(crate) fn dispatch_history_feedback(
        &mut self,
        feedback_token: String,
        ctx: &egui::Context,
    ) {
        if cfg!(test) {
            return;
        }
        if let Some(cancel) = self.history.remote.feedback_cancel.take() {
            cancel.cancel();
        }
        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let (rx, cancel) = history_sync::start_history_feedback(
            feedback_token,
            self.auth.youtube_web_session.clone(),
            hl,
            ctx,
        );
        self.history.remote.feedback_rx = Some(rx);
        self.history.remote.feedback_cancel = Some(cancel);
    }

    pub(crate) fn on_enter_history_section(&mut self, ctx: &egui::Context) {
        if cfg!(test) {
            return;
        }
        let has_session = self.auth.youtube_web_session.is_some()
            || (!self.auth.youtube_web_session_checked && self.auth.cached_has_saved_profile);
        if has_session && (self.history.remote.stale || !self.history.remote.loaded) {
            self.start_history_sync(self.history.remote.stale, false, ctx);
        }
    }

    pub(crate) fn start_history_sync(
        &mut self,
        force_refresh: bool,
        allow_interactive: bool,
        ctx: &egui::Context,
    ) {
        if !force_refresh
            && (self.history.remote.sync_rx.is_some()
                || (self.history.remote.loaded && !self.history.remote.stale))
        {
            return;
        }
        if let Some(cancel) = self.history.remote.sync_cancel.take() {
            cancel.cancel();
        }
        if let Some(cancel) = self.history.remote.chip_cancel.take() {
            cancel.cancel();
        }
        self.history.remote.chip_rx = None;
        self.history.remote.chip_active_request_id = None;
        self.history.remote.chip_active_filter = None;

        self.history.remote.sync_next_request_id =
            self.history.remote.sync_next_request_id.wrapping_add(1);
        let request_id = self.history.remote.sync_next_request_id;
        self.history.remote.sync_active_request_id = Some(request_id);
        self.history.remote.error = None;
        self.history.remote.needs_connect = false;
        self.history.remote.initial_sync_attempted = true;

        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let (rx, cancel) = history_sync::start_full_history_sync(
            request_id,
            self.auth.youtube_web_session.clone(),
            allow_interactive,
            hl,
            ctx,
        );
        self.history.remote.sync_rx = Some(rx);
        self.history.remote.sync_cancel = Some(cancel);
    }

    pub(crate) fn start_history_chip_fetch(
        &mut self,
        filter: history::HistoryFilter,
        load_more: bool,
        ctx: &egui::Context,
    ) {
        if cfg!(test) {
            return;
        }
        if self.history.remote.chip_rx.is_some()
            && self.history.remote.chip_active_filter == Some(filter)
        {
            return;
        }
        let action = if load_more {
            let Some(token) = self
                .history
                .remote
                .feeds
                .get(&filter)
                .and_then(|feed| feed.next_continuation.clone())
            else {
                return;
            };
            history_sync::HistoryChipAction::Continuation(token)
        } else if let Some(chip_action) = self.history.remote.chips.get(&filter).cloned() {
            chip_action
        } else {
            return;
        };

        let (fallback_date_label, page_index) = if load_more {
            let feed = self.history.remote.feeds.get(&filter);
            let label = feed
                .and_then(|f| f.sections.last())
                .map(|s| s.date_label.clone())
                .unwrap_or_default();
            let next_page = feed.map_or(0, |f| f.page_count);
            (label, next_page)
        } else {
            (String::new(), 0)
        };

        if let Some(cancel) = self.history.remote.chip_cancel.take() {
            cancel.cancel();
        }
        self.history.remote.chip_next_request_id =
            self.history.remote.chip_next_request_id.wrapping_add(1);
        let request_id = self.history.remote.chip_next_request_id;
        self.history.remote.chip_active_request_id = Some(request_id);
        self.history.remote.chip_active_filter = Some(filter);
        if let Some(feed) = self.history.remote.feeds.get_mut(&filter) {
            feed.loading_more = true;
        }
        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let (rx, cancel) = history_sync::start_history_chip_page_fetch(
            request_id,
            filter,
            action,
            fallback_date_label,
            page_index,
            self.auth.youtube_web_session.clone(),
            hl,
            ctx,
        );
        self.history.remote.chip_rx = Some(rx);
        self.history.remote.chip_cancel = Some(cancel);
    }

    pub(crate) fn apply_remote_history_page(
        &mut self,
        filter: history::HistoryFilter,
        page_index: usize,
        mut page: history_sync::RemoteHistoryPage,
    ) {
        for (chip_filter, action) in std::mem::take(&mut page.chips) {
            self.history.remote.chips.insert(chip_filter, action);
        }
        if filter == history::HistoryFilter::All
            && page_index == 0
            && (page.actions.clear_feedback_token.is_some()
                || page.actions.pause_feedback_token.is_some()
                || page.actions.remote_paused.is_some())
        {
            self.history.remote.actions = page.actions.clone();
        }

        let existing_count = if page_index == 0 {
            0
        } else {
            self.history
                .remote
                .feeds
                .get(&filter)
                .map_or(0, |f| f.seen_ids.len())
        };
        let cap_reached = history_sync::cap_remote_history_page(
            &mut page,
            existing_count,
            history_sync::MAX_REMOTE_HISTORY_FEED_ITEMS,
        );

        let now_ms = history::current_unix_epoch_ms();
        let mut converted_entries = Vec::new();
        let mut seq_idx = page_index.saturating_mul(1_000);
        let feed = self.history.remote.feeds.entry(filter).or_default();
        if page_index == 0 {
            feed.sections.clear();
            feed.seen_ids.clear();
            feed.page_count = 0;
        }
        feed.next_continuation = page.next_continuation;
        feed.loaded = true;
        feed.loading_more = false;
        feed.page_count = feed.page_count.max(page_index + 1);
        if feed.next_continuation.is_none() || cap_reached {
            feed.complete = true;
        }

        for section in page.sections {
            let mut dedup_items = Vec::with_capacity(section.items.len());
            for item in section.items {
                if let Some(tok) = &item.feedback_token {
                    self.history
                        .remote
                        .feedback_tokens
                        .insert(item.video_id.clone(), tok.clone());
                }
                let synthetic_ts =
                    synthetic_watched_at_ms_for_label(&item.date_label, now_ms, seq_idx as u64);
                seq_idx = seq_idx.saturating_add(1);
                converted_entries.push(item.to_history_entry(Some(synthetic_ts), filter));

                let canonical_url = format!("https://www.youtube.com/watch?v={}", item.video_id);
                let item_meta = item.to_video_metadata();
                self.video_metadata
                    .entry(canonical_url)
                    .and_modify(|existing| {
                        if existing.view_count.is_none() {
                            existing.view_count = item_meta.view_count;
                        }
                        if existing.view_count_text.is_none() {
                            existing.view_count_text = item_meta.view_count_text.clone();
                        }
                        if existing.published_time_text.is_none() {
                            existing.published_time_text = item_meta.published_time_text.clone();
                        }
                        if existing.channel_id.is_none() {
                            existing.channel_id = item_meta.channel_id.clone();
                        }
                        if existing.channel_url.is_none() {
                            existing.channel_url = item_meta.channel_url.clone();
                        }
                        if item_meta.channel_verified {
                            existing.channel_verified = true;
                        }
                        if item_meta.kind == search::SearchResultKind::Short {
                            existing.kind = search::SearchResultKind::Short;
                        }
                    })
                    .or_insert(item_meta);

                if feed.seen_ids.insert(item.video_id.clone()) {
                    dedup_items.push(item);
                }
            }
            if !dedup_items.is_empty() {
                if let Some(last) = feed.sections.last_mut()
                    && last.date_label == section.date_label
                {
                    last.items.extend(dedup_items);
                } else {
                    feed.sections.push(history_sync::RemoteHistorySection {
                        date_label: section.date_label,
                        items: dedup_items,
                    });
                }
            }
        }
        history_sync::enforce_sections_item_cap(
            &mut feed.sections,
            &mut feed.seen_ids,
            history_sync::MAX_REMOTE_HISTORY_FEED_ITEMS,
        );
        self.history
            .remote
            .feed_lru
            .touch(filter, &mut self.history.remote.feeds);
        if self.video_metadata.len() > MAX_VIDEO_METADATA_ENTRIES {
            self.prune_session_caches();
        }

        if !converted_entries.is_empty() {
            let prioritize_front = filter == history::HistoryFilter::All && page_index == 0;
            self.history
                .data
                .merge_remote_entries_ordered(&converted_entries, prioritize_front);
            self.history.dirty = true;
            self.invalidate_feed_cache();
            if page_index == 0 && !self.history.read_failed && self.history.path.is_some() {
                self.save_history_to_disk(Instant::now());
            }
        }
    }

    pub(crate) fn poll_history_sync(&mut self, _ctx: &egui::Context) {
        let mut sync_msgs = Vec::new();
        let mut sync_disconnected = false;
        if let Some(rx) = &self.history.remote.sync_rx {
            loop {
                match rx.try_recv() {
                    Ok(msg) => sync_msgs.push(msg),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        sync_disconnected = true;
                        break;
                    }
                }
            }
        }
        for msg in sync_msgs {
            match msg {
                history_sync::HistorySyncMessage::SessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                history_sync::HistorySyncMessage::Page {
                    request_id,
                    filter,
                    page_index,
                    page,
                } => {
                    if self.history.remote.sync_active_request_id != Some(request_id) {
                        continue;
                    }
                    self.apply_remote_history_page(filter, page_index, page);
                    if filter == history::HistoryFilter::All && page_index == 0 {
                        self.history.remote.loaded = true;
                    }
                }
                history_sync::HistorySyncMessage::CategoryComplete {
                    request_id,
                    filter,
                    complete,
                } => {
                    if self.history.remote.sync_active_request_id != Some(request_id) {
                        continue;
                    }
                    if let Some(feed) = self.history.remote.feeds.get_mut(&filter) {
                        feed.complete = complete;
                        feed.loaded = true;
                    }
                    let member_ids = self
                        .history
                        .remote
                        .feeds
                        .get(&filter)
                        .map(|f| f.seen_ids.clone())
                        .unwrap_or_default();
                    self.history.data.apply_remote_category_membership(
                        filter,
                        &member_ids,
                        complete,
                    );
                    self.history.dirty = true;
                    self.invalidate_feed_cache();
                }
                history_sync::HistorySyncMessage::Finished {
                    request_id,
                    auth_expired,
                    result,
                    ..
                } => {
                    if self.history.remote.sync_active_request_id != Some(request_id) {
                        continue;
                    }
                    self.history.remote.sync_rx = None;
                    self.history.remote.sync_cancel = None;
                    self.history.remote.sync_active_request_id = None;
                    if auth_expired {
                        self.auth.youtube_web_session = None;
                        self.auth.youtube_web_session_checked = true;
                        self.history.remote.needs_connect = true;
                    }
                    match result {
                        Ok(_) => {
                            self.history.remote.loaded = true;
                            self.history.remote.stale = false;
                            self.history.remote.needs_connect = false;
                            self.history.remote.error = None;
                            self.history.remote.synced_at = Some(Instant::now());
                            if self.history.dirty
                                && !self.history.read_failed
                                && self.history.path.is_some()
                            {
                                self.save_history_to_disk(Instant::now());
                            }
                        }
                        Err(err) => {
                            self.history.remote.error = Some(err);
                        }
                    }
                }
            }
        }
        if sync_disconnected && self.history.remote.sync_rx.is_some() {
            self.history.remote.sync_rx = None;
            self.history.remote.sync_cancel = None;
            self.history.remote.sync_active_request_id = None;
        }

        let mut chip_msgs = Vec::new();
        let mut chip_disconnected = false;
        if let Some(rx) = &self.history.remote.chip_rx {
            loop {
                match rx.try_recv() {
                    Ok(msg) => chip_msgs.push(msg),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        chip_disconnected = true;
                        break;
                    }
                }
            }
        }
        for msg in chip_msgs {
            match msg {
                history_sync::HistorySyncMessage::SessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                history_sync::HistorySyncMessage::Page {
                    request_id,
                    filter,
                    page_index,
                    page,
                } => {
                    if self.history.remote.chip_active_request_id != Some(request_id) {
                        continue;
                    }
                    self.apply_remote_history_page(filter, page_index, page);
                }
                history_sync::HistorySyncMessage::CategoryComplete {
                    request_id,
                    filter,
                    complete,
                } => {
                    if self.history.remote.chip_active_request_id != Some(request_id) {
                        continue;
                    }
                    if let Some(feed) = self.history.remote.feeds.get_mut(&filter) {
                        feed.complete = complete;
                        feed.loaded = true;
                    }
                    let member_ids = self
                        .history
                        .remote
                        .feeds
                        .get(&filter)
                        .map(|f| f.seen_ids.clone())
                        .unwrap_or_default();
                    self.history.data.apply_remote_category_membership(
                        filter,
                        &member_ids,
                        complete,
                    );
                    self.history.dirty = true;
                    if !self.history.read_failed && self.history.path.is_some() {
                        self.save_history_to_disk(Instant::now());
                    }
                    self.invalidate_feed_cache();
                }
                history_sync::HistorySyncMessage::Finished {
                    request_id,
                    filter,
                    auth_expired,
                    result,
                } => {
                    if self.history.remote.chip_active_request_id != Some(request_id) {
                        continue;
                    }
                    self.history.remote.chip_rx = None;
                    self.history.remote.chip_cancel = None;
                    self.history.remote.chip_active_request_id = None;
                    self.history.remote.chip_active_filter = None;
                    if let Some(feed) = self.history.remote.feeds.get_mut(&filter) {
                        feed.loading_more = false;
                    }
                    if auth_expired {
                        self.auth.youtube_web_session = None;
                        self.auth.youtube_web_session_checked = true;
                        self.history.remote.needs_connect = true;
                    }
                    if let Err(err) = result {
                        self.history.remote.error = Some(err);
                    }
                }
            }
        }
        if chip_disconnected && self.history.remote.chip_rx.is_some() {
            if let Some(filter) = self.history.remote.chip_active_filter
                && let Some(feed) = self.history.remote.feeds.get_mut(&filter)
            {
                feed.loading_more = false;
            }
            self.history.remote.chip_rx = None;
            self.history.remote.chip_cancel = None;
            self.history.remote.chip_active_request_id = None;
            self.history.remote.chip_active_filter = None;
        }

        if let Some(outcome) = self
            .history
            .remote
            .feedback_rx
            .as_ref()
            .map(Receiver::try_recv)
        {
            match outcome {
                Ok(ev) => {
                    self.history.remote.feedback_rx = None;
                    self.history.remote.feedback_cancel = None;
                    if ev.auth_expired {
                        self.auth.youtube_web_session = None;
                        self.history.remote.needs_connect = true;
                    } else if let Some(sess) = ev.web_session {
                        self.auth.youtube_web_session = Some(sess);
                        self.auth.youtube_web_session_checked = true;
                    }
                    if let Err(err) = ev.result {
                        self.history.remote.error = Some(err);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.history.remote.feedback_rx = None;
                    self.history.remote.feedback_cancel = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    pub(crate) fn record_video_accessed_in_history(
        &mut self,
        video: &Video,
        resume_target_ms: Option<u64>,
    ) {
        if self.settings.prefs.value.pause_watch_history || self.ui.incognito_mode {
            return;
        }
        self.ensure_history_synchronized();
        let Some(video_id) = history::youtube_video_id(&video.url) else {
            return;
        };
        let existing = self.history.data.get(&video_id).cloned();
        let meta = self.metadata_for(&video.url).cloned();
        let is_live = self.is_video_known_live(video);
        let duration_ms = if is_live {
            None
        } else {
            existing
                .as_ref()
                .and_then(|e| e.duration_ms)
                .or_else(|| parse_duration_ms(&video.duration))
                .filter(|&d| d > 0)
        };
        let mut position_ms = existing
            .as_ref()
            .map(|e| e.position_ms)
            .unwrap_or_else(|| resume_target_ms.unwrap_or(0));
        if let Some(dur) = duration_ms {
            position_ms = position_ms.min(dur);
        }
        let is_short = if video.url.contains("/shorts/")
            || meta
                .as_ref()
                .is_some_and(|m| m.kind == search::SearchResultKind::Short)
        {
            Some(true)
        } else {
            existing.as_ref().and_then(|e| e.is_short)
        };
        let is_podcast = existing.as_ref().and_then(|e| e.is_podcast);
        let is_music = existing.as_ref().and_then(|e| e.is_music);
        let view_count_text = existing
            .as_ref()
            .and_then(|e| e.view_count_text.clone())
            .or_else(|| {
                meta.as_ref().and_then(|m| {
                    m.view_count_text.clone().or_else(|| {
                        m.view_count
                            .map(recommendations::format_youtube_compact_views)
                    })
                })
            });
        let thumbnail_url = (!video.thumbnail.trim().is_empty())
            .then(|| video.thumbnail.clone())
            .or_else(|| existing.as_ref().and_then(|e| e.thumbnail_url.clone()));
        let channel_verified = existing
            .as_ref()
            .and_then(|e| e.channel_verified)
            .or_else(|| {
                meta.as_ref()
                    .and_then(|m| m.channel_verified.then_some(true))
            });
        let published_time_text = existing
            .as_ref()
            .and_then(|e| e.published_time_text.clone())
            .or_else(|| {
                meta.as_ref().and_then(|m| {
                    m.published_time_text.clone().or_else(|| {
                        m.upload_date
                            .as_deref()
                            .and_then(|d| recommendations::format_youtube_relative_time(d, None))
                    })
                })
            });
        let today_label = if self.ui_pt() { "Hoje" } else { "Today" }.to_owned();
        let entry = HistoryEntry {
            video_id: video_id.clone(),
            title: sanitize_history_title(&video.title, &video_id),
            channel: sanitize_history_channel(&video.channel),
            position_ms,
            duration_ms,
            watched_at_ms: Some(history::current_unix_epoch_ms()),
            is_short,
            is_podcast,
            is_music,
            date_label: Some(today_label.clone()),
            view_count_text: view_count_text.clone(),
            published_time_text: published_time_text.clone(),
            thumbnail_url: thumbnail_url.clone(),
            channel_verified,
        };
        if self.history.data.record(entry.clone()).is_ok() {
            self.history.dirty = true;
            self.invalidate_feed_cache();
            if !self.history.read_failed && self.history.path.is_some() {
                self.save_history_to_disk(Instant::now());
            }
        }

        let item = history_sync::RemoteHistoryItem {
            video_id: video_id.clone(),
            title: entry.title.clone(),
            channel: entry.channel.clone(),
            channel_id: meta.as_ref().and_then(|m| m.channel_id.clone()),
            channel_url: meta.as_ref().and_then(|m| m.channel_url.clone()),
            channel_verified: entry.channel_verified == Some(true),
            thumbnail_url: thumbnail_url.unwrap_or_default(),
            duration_text: video.duration.clone(),
            duration_ms,
            position_ms,
            view_count: meta.as_ref().and_then(|m| m.view_count),
            view_count_text,
            published_time_text,
            is_short: entry.is_short(),
            is_music: entry.is_music(),
            is_podcast: entry.is_podcast(),
            is_live,
            feedback_token: self.history.remote.feedback_tokens.get(&video_id).cloned(),
            date_label: today_label.clone(),
        };
        let mut target_filters = vec![history::HistoryFilter::All];
        if item.is_short {
            target_filters.push(history::HistoryFilter::Shorts);
        } else {
            target_filters.push(history::HistoryFilter::Videos);
        }
        if item.is_music {
            target_filters.push(history::HistoryFilter::Music);
        }
        if item.is_podcast {
            target_filters.push(history::HistoryFilter::Podcasts);
        }
        for filter in target_filters {
            if let Some(feed) = self.history.remote.feeds.get_mut(&filter) {
                for section in &mut feed.sections {
                    section.items.retain(|it| it.video_id != video_id);
                }
                feed.sections.retain(|section| !section.items.is_empty());
                feed.seen_ids.insert(video_id.clone());
                if let Some(first) = feed.sections.first_mut()
                    && (first.date_label.eq_ignore_ascii_case("Hoje")
                        || first.date_label.eq_ignore_ascii_case("Today"))
                {
                    first.items.insert(0, item.clone());
                } else {
                    feed.sections.insert(
                        0,
                        history_sync::RemoteHistorySection {
                            date_label: today_label.clone(),
                            items: vec![item.clone()],
                        },
                    );
                }
            }
        }
    }

    pub(crate) fn start_remote_watch_tracking(
        &mut self,
        video: &Video,
        resume_target_ms: Option<u64>,
        ctx: &egui::Context,
    ) {
        self.flush_remote_watch_tracking();
        if cfg!(test) || self.settings.prefs.value.pause_watch_history || self.ui.incognito_mode {
            return;
        }
        let Some(video_id) = history::youtube_video_id(&video.url) else {
            return;
        };
        if self.auth.youtube_web_session.is_none() && !self.auth.cached_has_saved_profile {
            return;
        }
        if let Some(cancel) = self.history.remote.watch_init_cancel.take() {
            cancel.cancel();
        }
        self.history.remote.watch_session_id = self.history.remote.watch_session_id.wrapping_add(1);
        let session_id = self.history.remote.watch_session_id;
        let start_secs = resume_target_ms.unwrap_or(0) as f64 / 1000.0;
        let (rx, cancel) = history_sync::start_remote_watch_init(
            session_id,
            video_id,
            start_secs,
            self.auth.youtube_web_session.clone(),
            ctx,
        );
        self.history.remote.watch_init_rx = Some(rx);
        self.history.remote.watch_init_cancel = Some(cancel);
        self.history.remote.stale = true;
    }

    pub(crate) fn maybe_send_remote_watchtime_heartbeat(
        &mut self,
        pos_secs: f64,
        paused: bool,
        force: bool,
        is_final: bool,
    ) {
        if cfg!(test) || self.settings.prefs.value.pause_watch_history || self.ui.incognito_mode {
            return;
        }
        let Some(session) = self.auth.youtube_web_session.clone() else {
            return;
        };
        let Some(tracker) = self.history.remote.watch_tracker.as_mut() else {
            return;
        };
        let active_matches = self
            .playback
            .active
            .as_ref()
            .and_then(|a| a.video_id.as_deref())
            == Some(tracker.video_id.as_str());
        if !active_matches {
            return;
        }
        if !force && !is_final && tracker.last_ping_at.elapsed() < Duration::from_secs(15) {
            return;
        }
        let clamped_pos = pos_secs.max(0.0);
        let snapshot = tracker.clone();
        tracker.last_ping_at = Instant::now();
        tracker.last_reported_pos_secs = clamped_pos;
        history_sync::send_watchtime_heartbeat_async(
            session,
            snapshot,
            clamped_pos,
            paused,
            is_final,
        );
    }

    pub(crate) fn flush_remote_watch_tracking(&mut self) {
        if let Some(cancel) = self.history.remote.watch_init_cancel.take() {
            cancel.cancel();
        }
        self.history.remote.watch_init_rx = None;
        let Some(tracker) = self.history.remote.watch_tracker.take() else {
            return;
        };
        if cfg!(test) || self.settings.prefs.value.pause_watch_history || self.ui.incognito_mode {
            return;
        }
        let Some(session) = self.auth.youtube_web_session.clone() else {
            return;
        };
        let pos_secs = self
            .playback
            .active
            .as_ref()
            .filter(|a| a.video_id.as_deref() == Some(tracker.video_id.as_str()))
            .map(|a| a.position_ms as f64 / 1000.0)
            .unwrap_or(tracker.last_reported_pos_secs);
        history_sync::send_watchtime_heartbeat_async(session, tracker, pos_secs, true, true);
    }

    pub(crate) fn poll_remote_watch_tracking(&mut self) {
        let Some(outcome) = self
            .history
            .remote
            .watch_init_rx
            .as_ref()
            .map(Receiver::try_recv)
        else {
            return;
        };
        match outcome {
            Ok(ev) => {
                self.history.remote.watch_init_rx = None;
                self.history.remote.watch_init_cancel = None;
                if ev.session_id != self.history.remote.watch_session_id {
                    return;
                }
                if ev.auth_expired {
                    self.auth.youtube_web_session = None;
                    self.history.remote.needs_connect = true;
                    return;
                }
                if let Some(sess) = ev.web_session {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                self.history.remote.watch_tracker = ev.tracker;
                if let Some(mut entry) = self.history.data.get(&ev.video_id).cloned() {
                    let mut changed = false;
                    if let Some(is_music) = ev.detected_music
                        && entry.is_music != Some(is_music)
                    {
                        entry.is_music = Some(is_music);
                        changed = true;
                    }
                    if let Some(is_short) = ev.detected_short
                        && entry.is_short != Some(is_short)
                    {
                        entry.is_short = Some(is_short);
                        changed = true;
                    }
                    if let Some(dur_ms) = ev.duration_ms.filter(|&d| d > 0)
                        && entry.duration_ms.is_none()
                    {
                        entry.duration_ms = Some(dur_ms);
                        entry.position_ms = entry.position_ms.min(dur_ms);
                        changed = true;
                    }
                    if changed && self.history.data.record(entry).is_ok() {
                        self.history.dirty = true;
                        self.invalidate_feed_cache();
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.history.remote.watch_init_rx = None;
                self.history.remote.watch_init_cancel = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    pub(crate) fn video_for_history_entry(&self, entry: &HistoryEntry) -> Option<Video> {
        let canonical_url = history::youtube_watch_url(&entry.video_id)?;
        if let Some(existing) = self.feed.videos.iter().find(|video| {
            history::youtube_video_id(&video.url).as_deref() == Some(entry.video_id.as_str())
        }) {
            return Some(existing.clone());
        }
        if let Some(channel_video) = self.channels.page.as_ref().and_then(|page| {
            page.videos
                .iter()
                .find(|item| {
                    history::youtube_video_id(&item.video.url).as_deref()
                        == Some(entry.video_id.as_str())
                })
                .map(|item| item.video.clone())
        }) {
            return Some(channel_video);
        }
        if let Some(playlist_video) = self
            .channels
            .playlist_detail
            .items
            .iter()
            .find(|item| {
                history::youtube_video_id(&item.video.url).as_deref()
                    == Some(entry.video_id.as_str())
            })
            .map(|item| item.video.clone())
        {
            return Some(playlist_video);
        }
        let duration = entry
            .duration_ms
            .map(|ms| format_time(ms as f64 / 1000.0))
            .unwrap_or_default();
        Some(Video {
            title: entry.title.clone(),
            channel: entry.channel.clone(),
            url: canonical_url,
            thumbnail: String::new(),
            duration,
        })
    }

    pub(crate) fn subscription_status_for(
        &self,
        target: Option<&ChannelTarget>,
        channel_id: Option<&str>,
    ) -> Option<&oauth::SubscriptionStatus> {
        if let Some(cid) = channel_id.map(str::trim).filter(|s| !s.is_empty())
            && let Some(status) = self.auth.subscriptions.get(cid)
        {
            return Some(status);
        }
        if let Some(t) = target {
            if let Some(status) = self.auth.subscriptions.get(&t.canonical_url()) {
                return Some(status);
            }
            if let ChannelTarget::ChannelId(cid) = t
                && let Some(status) = self.auth.subscriptions.get(cid)
            {
                return Some(status);
            }
        }
        None
    }

    pub(crate) fn start_check_subscription(
        &mut self,
        target: ChannelTarget,
        channel_id_hint: Option<String>,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        self.auth
            .sub_check_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| {
                        if let Some(cid) = channel_id_hint
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                        {
                            Ok(cid)
                        } else {
                            oauth::resolve_channel_id(&session.access_token, &target, &cancel)
                        }
                    })
                    .and_then(|cid| oauth::check_subscription(&session.access_token, &cid, &cancel))
                    .map(|status| (target.canonical_url(), status));
                (Some(session), result)
            });
    }

    pub(crate) fn start_toggle_subscription(
        &mut self,
        target: ChannelTarget,
        channel_id_hint: Option<String>,
        ctx: &egui::Context,
    ) {
        if self.auth.sub_toggle_task.is_running() {
            return;
        }
        let oauth_session = self.auth.session.clone();
        let web_session = Self::load_innertube_web_session_cached();
        if oauth_session.is_none() && web_session.is_none() {
            self.start_reauth(ctx);
            return;
        }
        let existing = self
            .subscription_status_for(Some(&target), channel_id_hint.as_deref())
            .cloned();
        let it_ctx = self.innertube_context();
        self.auth
            .sub_toggle_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let resolved_cid = existing
                    .as_ref()
                    .map(|s| s.channel_id.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        channel_id_hint
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                    })
                    .or_else(|| match &target {
                        ChannelTarget::ChannelId(id) => Some(id.clone()),
                        ChannelTarget::Handle(_) => None,
                    });
                let subscribe = existing
                    .as_ref()
                    .and_then(|s| s.subscription_id.as_ref())
                    .is_none();

                if let (Some(mut it_sess), Some(cid)) = (web_session, resolved_cid.as_deref()) {
                    match innertube::mutate_subscription(
                        cid,
                        subscribe,
                        &it_ctx,
                        &mut it_sess,
                        &cancel,
                    ) {
                        Ok(_) => {
                            let status = oauth::SubscriptionStatus {
                                channel_id: cid.to_owned(),
                                channel_title: existing
                                    .as_ref()
                                    .and_then(|s| s.channel_title.clone()),
                                subscription_id: subscribe.then(|| "innertube".to_owned()),
                            };
                            return (oauth_session, Ok((target.canonical_url(), status)));
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
                        Err("Não foi possível resolver o canal para alterar a inscrição.".into()),
                    );
                };
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| {
                        if let Some(cid) = resolved_cid {
                            Ok(cid)
                        } else {
                            oauth::resolve_channel_id(&session.access_token, &target, &cancel)
                        }
                    })
                    .and_then(|cid| {
                        if let Some(sub_id) = existing.and_then(|s| s.subscription_id) {
                            oauth::unsubscribe_channel(
                                &session.access_token,
                                &sub_id,
                                &cid,
                                &cancel,
                            )
                        } else {
                            oauth::subscribe_channel(&session.access_token, &cid, &cancel)
                        }
                    })
                    .map(|status| (target.canonical_url(), status));
                (Some(session), result)
            });
    }

    pub(crate) fn poll_subscriptions(&mut self, _ctx: &egui::Context) {
        if let Some((session, result)) = self.auth.sub_check_task.poll() {
            let _ = self.set_oauth_session(session);
            if let Ok((key, status)) = result {
                self.auth
                    .subscriptions
                    .insert(status.channel_id.clone(), status.clone());
                self.auth.subscriptions.insert(key, status);
            }
        }

        if let Some((session, result)) = self.auth.sub_toggle_task.poll() {
            let _ = self.set_oauth_session(session);
            match result {
                Ok((key, status)) => {
                    let subscribed = status.subscription_id.is_some();
                    self.auth
                        .subscriptions
                        .insert(status.channel_id.clone(), status.clone());
                    self.auth.subscriptions.insert(key, status);
                    self.feed.home.subscriptions_cached_at = None;
                    self.feed.home.subscription_channels_loaded = false;
                    self.message = if subscribed {
                        "Inscrição realizada com sucesso.".into()
                    } else {
                        "Inscrição cancelada.".into()
                    };
                }
                Err(error) => {
                    self.message = error;
                }
            }
        }
    }

    pub(crate) fn start_fetch_my_channel(&mut self, open_when_ready: bool, ctx: &egui::Context) {
        if self.auth.my_channel_task.is_running() {
            if open_when_ready {
                self.auth.my_channel_open_when_ready = true;
            }
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        self.auth.my_channel_open_when_ready = open_when_ready;
        self.auth.my_channel_fetch_attempted = true;
        self.auth
            .my_channel_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| oauth::my_channel_profile(&session.access_token, &cancel));
                (Some(session), result)
            });
    }

    pub(crate) fn poll_my_channel(&mut self, ctx: &egui::Context) {
        match self.auth.my_channel_task.poll_event() {
            async_task::AsyncTaskPoll::Ready((session, result)) => {
                let open_when_ready = std::mem::take(&mut self.auth.my_channel_open_when_ready);
                let _ = self.set_oauth_session(session);
                match result {
                    Ok(profile) => {
                        if let Some(title) = profile.title.as_deref() {
                            self.cache_channel_avatar(title, profile.avatar_url.as_deref());
                        }
                        self.auth.my_channel = Some(profile.target.clone());
                        self.auth.my_channel_title = profile.title;
                        self.auth.my_channel_avatar_url = profile.avatar_url;
                        self.auth.my_channel_subscribers = profile.subscriber_count;
                        if open_when_ready {
                            self.open_channel(profile.target, ctx);
                        }
                    }
                    Err(error) => {
                        if open_when_ready {
                            self.message = error;
                        }
                    }
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.auth.my_channel_open_when_ready = false;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_reauth(&mut self, ctx: &egui::Context) {
        if self.auth.reauth_task.is_running() {
            return;
        }
        self.auth
            .reauth_task
            .spawn_with_cancel(ctx.clone(), move |cancel| oauth::authorize(&cancel));
    }

    pub(crate) fn cancel_reauth(&mut self) {
        self.auth.reauth_task.cancel();
    }

    pub(crate) fn set_oauth_session(
        &mut self,
        session: Option<oauth::TokenSession>,
    ) -> Option<String> {
        if session.is_none() {
            self.auth.ratings.clear();
            self.auth.subscriptions.clear();
            self.auth.my_channel = None;
            self.auth.my_channel_title = None;
            self.auth.my_channel_avatar_url = None;
            self.auth.my_channel_subscribers = None;
            self.auth.my_channel_open_when_ready = false;
            self.auth.my_channel_fetch_attempted = false;
            self.auth.remote_playlists.clear();
            self.auth.youtube_web_session = None;
            self.auth.youtube_web_session_checked = false;
            #[cfg(not(test))]
            {
                self.auth.cached_has_saved_profile = session::has_saved_profile();
            }
            #[cfg(test)]
            {
                self.auth.cached_has_saved_profile = false;
            }
            self.library.liked_videos.reset();
            self.library.remote_playlists.reset();
            self.library.watch_later_remote.reset();
            self.library.active_playlist_detail.reset();
            self.history.remote.reset();
        }
        self.auth.session = session;
        #[cfg(test)]
        return None;
        #[cfg(not(test))]
        match self.auth.session.as_ref() {
            Some(session) => oauth::save_session(session).err(),
            None => oauth::delete_session().err(),
        }
    }

    #[cfg(test)]
    pub(crate) fn cancel_login(&mut self) {
        self.feed.home.stream_task.cancel();
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        self.feed.home.pending_items.clear();
        self.feed.home.title = None;
        self.feed.home.limit_reached = false;
        self.message = "Login cancelado. Você pode tentar novamente.".into();
    }

    pub(crate) fn poll_reauth(&mut self, ctx: &egui::Context) {
        match self.auth.reauth_task.poll_event() {
            async_task::AsyncTaskPoll::Ready(result) => match result {
                Ok(session) => {
                    self.auth.authenticated = true;
                    let persistence_error = self.set_oauth_session(Some(session));
                    self.auth.my_channel_fetch_attempted = false;
                    if self.auth.my_channel_avatar_url.is_none() {
                        self.start_fetch_my_channel(false, ctx);
                    }
                    self.start_subscription_channels_fetch(ctx);
                    self.watch.extras.reset_comments();
                    self.message = persistence_error
                        .map(|error| {
                            format!(
                                "Autorização concluída, mas não foi possível salvar a sessão: {error}"
                            )
                        })
                        .unwrap_or_else(|| "Comentários autorizados com sucesso.".into());
                    if self.watch.extras.tab == watch_extras::Tab::Comments
                        && self.watch.extras.video_url.is_some()
                    {
                        self.watch
                            .extras
                            .start_comments(self.auth.session.as_ref(), false, ctx);
                    }
                }
                Err(error) => {
                    self.message = error.clone();
                    if self.watch.extras.tab == watch_extras::Tab::Comments {
                        self.watch.extras.comments_error = Some(error);
                    }
                }
            },
            async_task::AsyncTaskPoll::Disconnected => {
                self.message = "Autoriza\u{00e7}\u{00e3}o interrompida.".into();
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn sign_out_and_reload_home(&mut self, ctx: &egui::Context) {
        self.close_video(ctx);
        let persistence_error = self.set_oauth_session(None);
        self.feed.home.stream_task.cancel();
        self.feed.notifications_task.cancel();
        self.feed.notifications.clear();
        self.feed.notifications_loading = false;
        self.feed.notifications_loading_more = false;
        self.feed.notifications_loaded = false;
        self.feed.notifications_initial_sync_attempted = false;
        self.feed.notifications_continuation = None;
        self.feed.notifications_error = None;
        self.feed.notifications_needs_connect = false;
        self.auth.authenticated = false;
        self.feed.home.title = None;
        self.feed.home.playlist_urls.clear();
        self.feed.home.playlist_total_results = None;
        self.library.active_remote_playlist_title = None;
        self.library.liked_videos.reset();
        self.library.remote_playlists.reset();
        self.library.watch_later_remote.reset();
        self.library.active_playlist_detail.reset();
        self.history.remote.reset();
        self.feed.home.pending_items.clear();
        self.feed.home.cached_items.clear();
        self.feed.home.cached_playlists.clear();
        self.feed.home.cached_loaded = false;
        self.feed.home.page = 0;
        self.feed.home.has_more = true;
        self.feed.home.channel_avatars.clear();
        self.feed.home.subscription_channels.clear();
        self.feed.home.subscription_channels_loaded = false;
        self.feed.home.subscription_channels_error = None;
        self.feed.home.subscriptions_cached_items.clear();
        self.feed.home.subscriptions_cached_loaded = false;
        self.feed.home.subscriptions_cached_at = None;
        self.feed.home.subscriptions_disk_cache_loaded = false;
        self.feed.home.subscriptions_rss_cache.clear();
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        if let Some(path) = self.effective_subscriptions_cache_path() {
            let _ = oauth::delete_subscriptions_cache_at_path(&path);
        }
        self.apply_search_results(ctx, Vec::new());
        self.ui.browse_section = BrowseSection::Home;
        self.ui.search_results_mode = false;
        self.feed.filter = FeedFilter::All;
        let logout_error = persistence_error.unwrap_or_default();
        self.start_fast_home_feed(false, ctx);
        if !logout_error.is_empty() {
            self.message = logout_error;
        }
    }

    pub(crate) fn load_innertube_web_session_cached() -> Option<innertube::YouTubeWebSession> {
        #[cfg(test)]
        {
            None
        }
        #[cfg(not(test))]
        {
            innertube::load_web_session().ok().flatten()
        }
    }

    pub(crate) fn innertube_context(&self) -> innertube::InnerTubeContext {
        innertube::InnerTubeContext::from_language(
            self.settings.prefs.value.language,
            self.settings.prefs.value.country_code.as_deref(),
        )
        .with_restricted_mode(self.settings.prefs.value.restricted_mode)
    }

    pub(crate) fn resolve_active_web_session(
        &mut self,
    ) -> Option<comment_actions::YouTubeWebSession> {
        self.auth.youtube_web_session.clone()
    }

    pub(crate) fn set_channel_notification_level(
        &mut self,
        channel_id: &str,
        level: settings::ChannelNotifyLevel,
        _ctx: &egui::Context,
    ) {
        let cid = channel_id.trim();
        if cid.is_empty() {
            return;
        }
        let mut prefs = self.settings.prefs.value.clone();
        prefs
            .per_channel_notifications
            .insert(cid.to_owned(), level);
        self.apply_app_preferences(prefs);
        let pt = self.ui_pt();
        let level_label = match (level, pt) {
            (settings::ChannelNotifyLevel::All, true) => "Todas",
            (settings::ChannelNotifyLevel::All, false) => "All",
            (settings::ChannelNotifyLevel::Personalized, true) => "Personalizadas",
            (settings::ChannelNotifyLevel::Personalized, false) => "Personalized",
            (settings::ChannelNotifyLevel::None, true) => "Nenhuma",
            (settings::ChannelNotifyLevel::None, false) => "None",
        };
        self.toasts.success(if pt {
            format!("Notifica\u{00e7}\u{00f5}es do canal: {level_label}")
        } else {
            format!("Channel notifications: {level_label}")
        });
    }

    pub(crate) fn unsubscribe_channel(&mut self, channel_id: &str, ctx: &egui::Context) {
        let cid = channel_id.trim();
        if cid.is_empty() {
            return;
        }
        let target = ChannelTarget::ChannelId(cid.to_owned());
        let removed_local = self
            .followed_channels
            .data
            .unfollow(Some(&target), Some(cid));
        if removed_local {
            if let Some(path) = self.followed_channels.path.as_deref() {
                let _ = followed_channels::save(path, &self.followed_channels.data);
            }
            self.followed_channels
                .feed_items
                .retain(|item| item.metadata.channel_id.as_deref() != Some(cid));
        }
        self.feed
            .home
            .subscription_channels
            .retain(|ch| ch.channel_id != cid);
        if self.auth.authenticated
            && self
                .auth
                .subscriptions
                .get(cid)
                .is_some_and(|st| st.subscription_id.is_some())
        {
            self.start_toggle_subscription(target, Some(cid.to_owned()), ctx);
        }
        let mut prefs = self.settings.prefs.value.clone();
        if prefs.per_channel_notifications.remove(cid).is_some() {
            self.apply_app_preferences(prefs);
        }
        let pt = self.ui_pt();
        self.toasts.info(if pt {
            "Inscri\u{00e7}\u{00e3}o cancelada."
        } else {
            "Unsubscribed from channel."
        });
    }
}

fn synthetic_watched_at_ms_for_label(date_label: &str, now_ms: u64, seq_offset: u64) -> u64 {
    const DAY_MS: u64 = 86_400_000;
    let lower = date_label.trim().to_lowercase();
    if lower.is_empty() || lower == "hoje" || lower == "today" {
        now_ms.saturating_sub(seq_offset)
    } else if lower == "ontem" || lower == "yesterday" {
        now_ms.saturating_sub(DAY_MS + seq_offset)
    } else if lower.contains("segunda")
        || lower.contains("terça")
        || lower.contains("terca")
        || lower.contains("quarta")
        || lower.contains("quinta")
        || lower.contains("sexta")
        || lower.contains("sábado")
        || lower.contains("sabado")
        || lower.contains("domingo")
        || lower.contains("monday")
        || lower.contains("tuesday")
        || lower.contains("wednesday")
        || lower.contains("thursday")
        || lower.contains("friday")
        || lower.contains("saturday")
        || lower.contains("sunday")
        || lower.contains("semana")
        || lower.contains("week")
    {
        now_ms.saturating_sub(3 * DAY_MS + seq_offset)
    } else {
        now_ms.saturating_sub(14 * DAY_MS + seq_offset)
    }
}
