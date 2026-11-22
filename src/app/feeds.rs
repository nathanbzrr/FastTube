use crate::*;

impl FastTube {
    pub(crate) fn prune_session_caches(&mut self) {
        if self.video_metadata.len() > 250 {
            let mut keep_urls = HashSet::new();
            if let Some(selected) = &self.selected {
                keep_urls.insert(selected.url.clone());
            }
            for video in &self.feed.videos {
                keep_urls.insert(video.url.clone());
            }
            for video in self.queue.data.videos() {
                keep_urls.insert(video.url.clone());
            }
            for video in &self.playback.video_back_stack {
                keep_urls.insert(video.url.clone());
            }
            for video in &self.playback.video_forward_stack {
                keep_urls.insert(video.url.clone());
            }
            if let Some(page) = &self.channels.page {
                for item in &page.videos {
                    keep_urls.insert(item.video.url.clone());
                }
                if let Some(featured) = &page.featured_video {
                    keep_urls.insert(featured.video.url.clone());
                }
                for shelf in &page.home_shelves {
                    for item in &shelf.items {
                        keep_urls.insert(item.video.url.clone());
                    }
                }
            }
            for item in &self.channels.videos.sorted_items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.shorts.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.streams.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.search.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.channels.playlist_detail.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.library.liked_videos.items {
                keep_urls.insert(item.video.url.clone());
            }
            for item in &self.followed_channels.feed_items {
                keep_urls.insert(item.video.url.clone());
            }
            self.video_metadata.retain(|url, _| keep_urls.contains(url));
            if self.video_metadata.len() > MAX_VIDEO_METADATA_ENTRIES {
                let selected_url = self.selected.as_ref().map(|v| v.url.clone());
                let mut overflow = self.video_metadata.len() - MAX_VIDEO_METADATA_ENTRIES;
                let victims: Vec<String> = self
                    .video_metadata
                    .keys()
                    .filter(|url| selected_url.as_deref() != Some(url.as_str()))
                    .take(overflow)
                    .cloned()
                    .collect();
                for url in victims {
                    self.video_metadata.remove(&url);
                    overflow = overflow.saturating_sub(1);
                    if overflow == 0 {
                        break;
                    }
                }
            }
            let min_cap = self.video_metadata.len().max(64);
            if self.video_metadata.capacity() > min_cap.saturating_mul(4) {
                self.video_metadata.shrink_to(min_cap);
            }
        }
        if self.feed.home.cached_items.len() > MAX_HOME_CACHED_ITEMS {
            self.feed.home.cached_items.truncate(MAX_HOME_CACHED_ITEMS);
        }
        if self.feed.home.channel_avatars.len() > 300 {
            let mut keep_channels = HashSet::new();
            for ch in &self.feed.home.subscription_channels {
                let key = recommendations::normalize_channel_key(&ch.title);
                if !key.is_empty() {
                    keep_channels.insert(key);
                }
            }
            for video in &self.feed.videos {
                let key = recommendations::normalize_channel_key(&video.channel);
                if !key.is_empty() {
                    keep_channels.insert(key);
                }
            }
            if let Some(selected) = &self.selected {
                let key = recommendations::normalize_channel_key(&selected.channel);
                if !key.is_empty() {
                    keep_channels.insert(key);
                }
            }
            if let Some(page) = &self.channels.page {
                if let Some(title) = page.title.as_deref() {
                    let key = recommendations::normalize_channel_key(title);
                    if !key.is_empty() {
                        keep_channels.insert(key);
                    }
                }
                for item in &page.videos {
                    let key = recommendations::normalize_channel_key(&item.video.channel);
                    if !key.is_empty() {
                        keep_channels.insert(key);
                    }
                }
            }
            self.feed
                .home
                .channel_avatars
                .retain(|k, _| keep_channels.contains(k));
            if self.feed.home.channel_avatars.len() > MAX_CHANNEL_AVATARS_ENTRIES {
                let selected_key = self
                    .selected
                    .as_ref()
                    .map(|v| recommendations::normalize_channel_key(&v.channel));
                let overflow = self.feed.home.channel_avatars.len() - MAX_CHANNEL_AVATARS_ENTRIES;
                let victims: Vec<String> = self
                    .feed
                    .home
                    .channel_avatars
                    .keys()
                    .filter(|key| selected_key.as_deref() != Some(key.as_str()))
                    .take(overflow)
                    .cloned()
                    .collect();
                for key in victims {
                    self.feed.home.channel_avatars.remove(&key);
                }
            }
            let min_cap = self.feed.home.channel_avatars.len().max(64);
            if self.feed.home.channel_avatars.capacity() > min_cap.saturating_mul(4) {
                self.feed.home.channel_avatars.shrink_to(min_cap);
            }
        }
    }

    pub(crate) fn metadata_for(&self, url: &str) -> Option<&VideoMetadata> {
        self.video_metadata
            .get(url)
            .or_else(|| {
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|selected| selected.url == url)
                {
                    self.selected_metadata.as_ref()
                } else {
                    None
                }
            })
            .or_else(|| {
                self.channels.page.as_ref().and_then(|page| {
                    page.videos
                        .iter()
                        .find(|item| item.video.url == url)
                        .map(|item| &item.metadata)
                })
            })
            .or_else(|| {
                self.channels
                    .playlist_detail
                    .items
                    .iter()
                    .find(|item| item.video.url == url)
                    .map(|item| &item.metadata)
            })
    }

    pub(crate) fn channel_target_for_video(&self, video: &Video) -> Option<ChannelTarget> {
        self.metadata_for(&video.url)
            .and_then(search::channel_target_for_metadata)
    }

    pub(crate) fn apply_search_results(&mut self, ctx: &egui::Context, results: Vec<SearchResult>) {
        self.message = if results.is_empty() {
            "Nenhum vídeo encontrado.".into()
        } else {
            String::new()
        };
        if self.channels.view_open {
            for previous in &self.feed.videos {
                let thumb = &previous.thumbnail;
                if !thumb.is_empty()
                    && !results.iter().any(|item| &item.video.thumbnail == thumb)
                    && !self.channels.page.as_ref().is_some_and(|page| {
                        page.videos
                            .iter()
                            .any(|item| &item.video.thumbnail == thumb)
                            || page
                                .featured_video
                                .as_ref()
                                .is_some_and(|item| &item.video.thumbnail == thumb)
                            || page.home_shelves.iter().any(|shelf| {
                                shelf
                                    .items
                                    .iter()
                                    .any(|item| &item.video.thumbnail == thumb)
                            })
                    })
                    && !self
                        .channels
                        .videos
                        .sorted_items
                        .iter()
                        .any(|item| &item.video.thumbnail == thumb)
                    && !self
                        .channels
                        .search
                        .items
                        .iter()
                        .any(|item| &item.video.thumbnail == thumb)
                    && !self
                        .channels
                        .playlist_detail
                        .items
                        .iter()
                        .any(|item| &item.video.thumbnail == thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
        }
        let channel_videos_len = self
            .channels
            .page
            .as_ref()
            .map_or(0, |page| page.videos.len());
        let mut videos = Vec::with_capacity(results.len());
        let mut metadata_map = HashMap::with_capacity(
            results.len()
                + self.queue.data.len()
                + channel_videos_len
                + self.channels.videos.sorted_items.len()
                + self.channels.search.items.len()
                + self.channels.playlist_detail.items.len()
                + self.library.liked_videos.items.len(),
        );
        for mut item in results {
            if self.ui.search_results_mode
                && self.feed.search_reported_urls.contains(&item.video.url)
            {
                continue;
            }
            if (item.metadata.kind == search::SearchResultKind::Playlist
                || item.video.url.contains("playlist?list="))
                && !self.feed.home.playlist_urls.contains(&item.video.url)
            {
                self.feed.home.playlist_urls.push(item.video.url.clone());
            }
            let avatar_key = recommendations::normalize_channel_key(&item.video.channel);
            if !avatar_key.is_empty()
                && let Some(avatar_url) = item
                    .metadata
                    .channel_avatar_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|url| !url.is_empty())
            {
                self.feed
                    .home
                    .channel_avatars
                    .insert(avatar_key, avatar_url.to_owned());
            }
            if let Some(prev) = self.video_metadata.get(&item.video.url) {
                item.metadata.merge_missing_from(prev);
            }
            metadata_map.insert(item.video.url.clone(), item.metadata);
            videos.push(item.video);
        }
        if !self.channels.view_open {
            self.thumbnails
                .retain_for_videos(ctx, &self.feed.videos, &videos);
        }
        if let Some(page) = &self.channels.page {
            for item in &page.videos {
                metadata_map
                    .entry(item.video.url.clone())
                    .or_insert_with(|| item.metadata.clone());
            }
            if let Some(featured) = &page.featured_video {
                metadata_map
                    .entry(featured.video.url.clone())
                    .or_insert_with(|| featured.metadata.clone());
            }
            for shelf in &page.home_shelves {
                for item in &shelf.items {
                    metadata_map
                        .entry(item.video.url.clone())
                        .or_insert_with(|| item.metadata.clone());
                }
            }
        }
        for item in &self.channels.videos.sorted_items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for item in &self.channels.search.items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for item in &self.channels.playlist_detail.items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for item in &self.library.liked_videos.items {
            metadata_map
                .entry(item.video.url.clone())
                .or_insert_with(|| item.metadata.clone());
        }
        for queued in self.queue.data.videos() {
            if !metadata_map.contains_key(&queued.url)
                && let Some(existing) = self.video_metadata.get(&queued.url)
            {
                metadata_map.insert(queued.url.clone(), existing.clone());
            }
        }
        if let Some(selected) = &self.selected {
            if let Some(found) = metadata_map.get_mut(&selected.url) {
                if let Some(existing) = &self.selected_metadata {
                    found.merge_missing_from(existing);
                }
                self.selected_metadata = Some(found.clone());
            } else if let Some(existing) = &self.selected_metadata {
                metadata_map.insert(selected.url.clone(), existing.clone());
            }
        }
        self.feed.videos = videos;
        self.feed.generation = self.feed.generation.wrapping_add(1);
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        self.video_metadata = metadata_map;
        self.feed.cache_dirty = true;
        self.maybe_auto_open_first_short_in_tab();
    }

    pub(crate) fn invalidate_feed_cache(&mut self) {
        self.feed.cache_dirty = true;
        self.feed.signal_revision = self.feed.signal_revision.wrapping_add(1);
    }

    pub(crate) fn ensure_feed_cache(&mut self) {
        let history_len = self.history.data.entries().len();
        let queue_len = self.queue.data.len();
        let selected_url = self.selected.as_ref().map(|video| video.url.clone());
        let pause_recommendations = self.settings.prefs.value.pause_recommendations;
        let muted_topics = &self.settings.prefs.value.muted_topics;
        let muted_channels = &self.settings.prefs.value.muted_channels;
        let pinned_topics = &self.settings.prefs.value.pinned_topics;
        let hide_shorts_in_home = self.settings.prefs.value.hide_shorts_in_home
            && self.ui.browse_section == BrowseSection::Home
            && !self.ui.search_results_mode
            && !matches!(self.feed.filter, FeedFilter::Shorts);
        let cache_is_current = self.feed.cache.generation == self.feed.generation
            && self.feed.cache.filter == self.feed.filter
            && self.feed.cache.history_len == history_len
            && self.feed.cache.queue_len == queue_len
            && self.feed.cache.selected_url == selected_url
            && self.feed.cache.pause_recommendations == pause_recommendations
            && self.feed.cache.hide_shorts_in_home == hide_shorts_in_home
            && self.feed.cache.muted_topics == *muted_topics
            && self.feed.cache.muted_channels == *muted_channels
            && self.feed.cache.pinned_topics == *pinned_topics;
        if !self.feed.cache_dirty && cache_is_current {
            return;
        }
        let mut profile = if pause_recommendations {
            UserProfile::default()
        } else {
            UserProfile::build(&self.history.data, &self.queue.data, self.selected.as_ref())
        };
        if !muted_topics.is_empty() {
            profile.apply_muted_topics(muted_topics);
        }
        if !muted_channels.is_empty() {
            profile.apply_muted_channels(muted_channels);
        }
        self.feed.cache.chips = RecommendationEngine::available_chips_with_pinned(
            &self.feed.videos,
            &self.video_metadata,
            &profile,
            pinned_topics,
        );
        self.feed.cache.ranked = RecommendationEngine::rank_videos(
            &self.feed.videos,
            &self.video_metadata,
            &profile,
            &self.feed.filter,
        );
        if hide_shorts_in_home {
            self.feed.cache.ranked.retain(|item| !item.is_short);
        }
        self.feed.cache.profile = profile;
        self.feed.cache.generation = self.feed.generation;
        self.feed.cache.filter = self.feed.filter.clone();
        self.feed.cache.history_len = history_len;
        self.feed.cache.queue_len = queue_len;
        self.feed.cache.selected_url = selected_url;
        self.feed.cache.pause_recommendations = pause_recommendations;
        self.feed.cache.hide_shorts_in_home = hide_shorts_in_home;
        self.feed.cache.muted_topics = muted_topics.clone();
        self.feed.cache.muted_channels = muted_channels.clone();
        self.feed.cache.pinned_topics = pinned_topics.clone();
        self.feed.cache_dirty = false;
    }

    pub(crate) fn cache_channel_avatar(&mut self, channel_name: &str, avatar_url: Option<&str>) {
        let key = recommendations::normalize_channel_key(channel_name);
        if key.is_empty() {
            return;
        }
        if let Some(url) = avatar_url.map(str::trim).filter(|url| !url.is_empty()) {
            self.feed.home.channel_avatars.insert(key, url.to_owned());
        }
    }

    pub(crate) fn populate_channel_avatars_from_subscriptions(&mut self) {
        let pairs: Vec<(String, String)> = self
            .feed
            .home
            .subscription_channels
            .iter()
            .filter_map(|ch| {
                let url = ch.avatar_url.as_deref()?.trim();
                if url.is_empty() {
                    return None;
                }
                let key = recommendations::normalize_channel_key(&ch.title);
                (!key.is_empty()).then(|| (key, url.to_owned()))
            })
            .collect();
        for (key, url) in pairs {
            self.feed.home.channel_avatars.insert(key, url);
        }
    }

    pub(crate) fn lookup_channel_avatar(&self, channel_name: &str) -> Option<&str> {
        let key = recommendations::normalize_channel_key(channel_name);
        if key.is_empty() {
            return None;
        }
        self.feed.home.channel_avatars.get(&key).map(String::as_str)
    }

    pub(crate) fn record_recent_search(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with(':')
        {
            return;
        }
        let lower = trimmed.to_lowercase();
        self.feed
            .recent_searches
            .retain(|existing| existing.to_lowercase() != lower);
        self.feed.recent_searches.insert(0, trimmed.to_owned());
        self.feed.recent_searches.truncate(12);
        self.feed.search_suggest.bump_recent_revision();
    }

    pub(crate) fn remove_recent_search(&mut self, query: &str) {
        let lower = query.trim().to_lowercase();
        let before = self.feed.recent_searches.len();
        self.feed
            .recent_searches
            .retain(|existing| existing.to_lowercase() != lower);
        if self.feed.recent_searches.len() != before {
            self.feed.search_suggest.bump_recent_revision();
        }
    }

    pub(crate) fn mute_channel_preference(&mut self, channel: &str) {
        let trimmed = channel.trim();
        if trimmed.is_empty() {
            return;
        }
        let key = recommendations::normalize_channel_key(trimmed);
        let mut prefs = self.settings.prefs.value.clone();
        if !prefs
            .muted_channels
            .iter()
            .any(|existing| recommendations::normalize_channel_key(existing) == key)
        {
            prefs.muted_channels.push(trimmed.to_owned());
            self.apply_app_preferences(prefs);
        }
        self.invalidate_feed_cache();
        let notice = format!("Menos vídeos de \"{trimmed}\" no feed.");
        self.message = notice.clone();
        self.toasts.info(notice);
    }

    pub(crate) fn mute_topic_preference(&mut self, topic: &str) {
        let trimmed = topic.trim();
        if trimmed.is_empty() {
            return;
        }
        let lower = trimmed.to_lowercase();
        let mut prefs = self.settings.prefs.value.clone();
        if !prefs
            .muted_topics
            .iter()
            .any(|existing| existing.trim().to_lowercase() == lower)
        {
            prefs.muted_topics.push(trimmed.to_owned());
            self.apply_app_preferences(prefs);
        }
        self.invalidate_feed_cache();
        let notice = format!("Tópico \"{trimmed}\" silenciado.");
        self.message = notice.clone();
        self.toasts.info(notice);
    }

    pub(crate) fn is_topic_pinned(&self, topic: &str) -> bool {
        let lower = topic.trim().to_lowercase();
        !lower.is_empty()
            && self
                .settings
                .prefs
                .value
                .pinned_topics
                .iter()
                .any(|existing| existing.trim().to_lowercase() == lower)
    }

    pub(crate) fn toggle_pinned_topic(&mut self, topic: &str) {
        let trimmed = topic.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            return;
        }
        let lower = trimmed.to_lowercase();
        let mut prefs = self.settings.prefs.value.clone();
        if let Some(pos) = prefs
            .pinned_topics
            .iter()
            .position(|existing| existing.trim().to_lowercase() == lower)
        {
            prefs.pinned_topics.remove(pos);
            self.apply_app_preferences(prefs);
            self.invalidate_feed_cache();
            let notice = format!("Tópico \"{trimmed}\" desafixado.");
            self.message = notice.clone();
            self.toasts.info(notice);
        } else if prefs.pinned_topics.len() < 24 {
            prefs.pinned_topics.push(trimmed.to_owned());
            self.apply_app_preferences(prefs);
            self.invalidate_feed_cache();
            let notice = format!("Tópico \"{trimmed}\" fixado nos filtros.");
            self.message = notice.clone();
            self.toasts.success(notice);
        } else {
            let warn = "Limite de 24 tópicos fixados atingido.".to_owned();
            self.message = warn.clone();
            self.toasts.warning(warn);
        }
    }

    pub(crate) fn start_search_suggestions_fetch(&mut self, query: &str, ctx: &egui::Context) {
        let trimmed = query.trim();
        if trimmed.len() < 2
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with(':')
        {
            self.feed.search_suggest.task.cancel();
            self.feed.search_suggest.clear_remote_suggestions();
            return;
        }
        self.feed.search_suggest.task.cancel();
        self.feed.search_suggest.last_fetched_query = trimmed.to_owned();
        if let Some(cached) = self.feed.search_suggest.get_cached_prefix(trimmed) {
            self.feed.search_suggest.set_remote_suggestions(cached);
            return;
        }
        let owned_query = trimmed.to_owned();
        self.feed.search_suggest.task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result = search::fetch_search_suggestions(&owned_query, &cancel);
                app_state::SearchSuggestFetchMessage {
                    request_id,
                    query: owned_query,
                    result,
                }
            },
        );
    }

    pub(crate) fn poll_search_suggestions(&mut self, ctx: &egui::Context) {
        let expected_id = self.feed.search_suggest.task.active_request_id();
        if let async_task::AsyncTaskPoll::Ready(msg) = self.feed.search_suggest.task.poll_event() {
            let is_current_id = expected_id == Some(msg.request_id);
            let is_current_query = msg
                .query
                .trim()
                .eq_ignore_ascii_case(self.feed.query.trim());
            if is_current_id && let Ok(items) = msg.result {
                self.feed
                    .search_suggest
                    .insert_cached_prefix(&msg.query, items.clone());
                if is_current_query {
                    self.feed.search_suggest.set_remote_suggestions(items);
                }
            }
        }

        if let Some(pending) = self.feed.search_suggest.pending_fetch_query.clone() {
            let elapsed_ready = self
                .feed
                .search_suggest
                .last_input_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(220));
            if elapsed_ready {
                self.feed.search_suggest.pending_fetch_query = None;
                if pending.trim().eq_ignore_ascii_case(self.feed.query.trim()) {
                    self.start_search_suggestions_fetch(&pending, ctx);
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }
    }

    pub(crate) fn begin_search_request(&mut self) -> Option<(u64, String)> {
        let query = self.feed.query.trim().to_owned();
        if query.is_empty() {
            return None;
        }
        self.feed.home.stream_task.cancel();
        self.feed.home.batch_task.cancel();
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        self.feed.home.title = None;
        self.feed.home.playlist_urls.clear();
        self.feed.home.playlist_total_results = None;
        self.library.active_remote_playlist_title = None;
        self.feed.home_innertube_task.cancel();
        self.feed.home_loading_more = false;
        self.feed.subs_innertube_task.cancel();
        self.feed.subscriptions_loading_more = false;
        self.feed.search_page_task.cancel();
        self.feed.search_continuation = None;
        self.feed.search_loading_more = false;
        self.feed.search_spelling = None;
        self.feed.search_page_last_query = query.clone();
        let request_id = self.feed.search.task.begin_reserved();
        self.channels.view_open = false;
        self.cancel_channel_subrequests();
        // Cancel any in-flight channel fetch when a new search replaces navigation.
        if self.channels.videos.task.is_active() {
            self.channels.videos.task.cancel();
            self.channels.loading_target = None;
        }
        self.message = "Pesquisando...".into();
        Some((request_id, query))
    }

    #[cfg(test)]
    pub(crate) fn complete_search_request(
        &mut self,
        request_id: u64,
        result: Result<Vec<SearchResult>, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.feed.search.task.active_request_id() != Some(request_id) {
            return false;
        }
        self.feed.search.task.cancel();
        self.apply_search_request(result, ctx);
        true
    }

    pub(crate) fn apply_search_request(
        &mut self,
        result: Result<Vec<SearchResult>, String>,
        ctx: &egui::Context,
    ) {
        match result {
            Ok(results) => self.apply_search_results(ctx, results),
            Err(error) => self.message = error,
        }
    }

    pub(crate) fn restore_cached_home_feed(&mut self, ctx: &egui::Context) -> bool {
        if !self.feed.home.cached_loaded || self.feed.home.cached_items.is_empty() {
            return false;
        }
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.playlist_urls = self.feed.home.cached_playlists.clone();
        self.feed.home.title = None;
        self.feed.home.limit_reached = false;
        let cached = self.feed.home.cached_items.clone();
        self.apply_search_results(ctx, cached);
        true
    }

    pub(crate) fn effective_subscriptions_cache_path(&self) -> Option<PathBuf> {
        if let Some(path) = &self.feed.home.subscriptions_disk_cache_path {
            return Some(path.clone());
        }
        #[cfg(not(test))]
        {
            oauth::subscriptions_cache_path().ok()
        }
        #[cfg(test)]
        {
            None
        }
    }

    pub(crate) fn ensure_subscriptions_disk_cache_loaded(&mut self) {
        if self.feed.home.subscriptions_disk_cache_loaded {
            return;
        }
        self.feed.home.subscriptions_disk_cache_loaded = true;
        let Some(path) = self.effective_subscriptions_cache_path() else {
            return;
        };
        let Ok(Some(disk_cache)) = oauth::load_subscriptions_cache_from_path(&path) else {
            return;
        };
        let (saved_at_unix_secs, channels, items, rss_cache) = disk_cache.into_parts();
        if !rss_cache.is_empty() && self.feed.home.subscriptions_rss_cache.is_empty() {
            self.feed.home.subscriptions_rss_cache = rss_cache;
        }
        if !self.feed.home.subscription_channels_loaded && !channels.is_empty() {
            self.feed.home.subscription_channels = channels;
            self.feed.home.subscription_channels_loaded = true;
            self.populate_channel_avatars_from_subscriptions();
        }
        if !self.feed.home.subscriptions_cached_loaded && !items.is_empty() {
            self.feed.home.subscriptions_cached_items = items;
            self.feed.home.subscriptions_cached_loaded = true;
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let age_secs = now_secs.saturating_sub(saved_at_unix_secs);
            self.feed.home.subscriptions_cached_at =
                Instant::now().checked_sub(Duration::from_secs(age_secs));
        }
    }

    pub(crate) fn persist_subscriptions_disk_cache(&self) {
        let Some(path) = self.effective_subscriptions_cache_path() else {
            return;
        };
        let saved_at_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let feed = search::HomeFeed {
            items: self.feed.home.subscriptions_cached_items.clone(),
            playlists: Vec::new(),
            channels: self.feed.home.subscription_channels.clone(),
        };
        let cache = oauth::SubscriptionsDiskCache::from_feed(
            &feed,
            &self.feed.home.subscriptions_rss_cache,
            saved_at_unix_secs,
        );
        let _ = oauth::save_subscriptions_cache_to_path(&path, &cache);
    }

    pub(crate) fn prioritized_subscription_channel_ids(&self) -> Vec<String> {
        let profile =
            UserProfile::build(&self.history.data, &self.queue.data, self.selected.as_ref());
        if profile.channel_weights.is_empty() || self.feed.home.subscription_channels.is_empty() {
            return Vec::new();
        }
        let mut ranked: Vec<(f64, usize, String)> = self
            .feed
            .home
            .subscription_channels
            .iter()
            .enumerate()
            .filter_map(|(idx, ch)| {
                let weight = profile
                    .channel_weights
                    .get(&recommendations::normalize_channel_key(&ch.title))
                    .copied()
                    .unwrap_or(0.0);
                (weight > 0.0).then(|| (weight, idx, ch.channel_id.clone()))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        ranked.into_iter().map(|(_, _, id)| id).collect()
    }

    pub(crate) fn start_fast_subscriptions_feed(
        &mut self,
        force_refresh_channels: bool,
        progressive_streaming: bool,
        ctx: &egui::Context,
    ) {
        let Some(mut session) = self.auth.session.clone() else {
            self.start_browser_feed(":ytrec".into(), true, ctx);
            return;
        };
        self.feed.home.stream_task.cancel();
        self.feed.home.batch_task.cancel();
        self.feed.search.task.cancel();

        self.feed.home.subscriptions_progressive_streaming = progressive_streaming;
        self.feed.home.subscriptions_refreshing_in_background = !progressive_streaming;
        self.feed.home.limit_reached = false;
        self.feed.home.pending_items.clear();
        self.feed.home.subscription_channels_error = None;
        self.feed.home.title = Some("Inscri\u{00e7}\u{00f5}es: envios recentes".into());
        self.feed.home.playlist_urls.clear();
        self.message = if progressive_streaming {
            "Consultando suas inscri\u{00e7}\u{00f5}es...".into()
        } else {
            "Atualizando inscri\u{00e7}\u{00f5}es em segundo plano...".into()
        };

        let known_channels = (!force_refresh_channels
            && self.feed.home.subscription_channels_loaded
            && !self.feed.home.subscription_channels.is_empty())
        .then(|| self.feed.home.subscription_channels.clone());
        let prioritized_channel_ids = self.prioritized_subscription_channel_ids();
        let rss_cache = self.feed.home.subscriptions_rss_cache.clone();

        self.feed
            .home
            .stream_task
            .spawn_stream(ctx.clone(), move |cancel, sender| {
                let request_id = sender.request_id();
                let result = if let Err(error) = session.refresh_if_needed(&cancel) {
                    (Some(session), Err(error))
                } else {
                    let progress_sender = sender.clone();
                    let options = oauth::SubscriptionFeedOptions {
                        known_channels,
                        prioritized_channel_ids,
                        rss_cache,
                    };
                    let feed = oauth::subscriptions_feed_fast(
                        &session.access_token,
                        options,
                        &cancel,
                        move |progress| {
                            let _ = progress_sender.send(HomeFetchMessage::SubscriptionProgress {
                                request_id,
                                progress,
                            });
                        },
                    );
                    (Some(session), feed)
                };
                let _ = sender.send(HomeFetchMessage::SubscriptionFinished {
                    request_id,
                    token: result.0,
                    result: result.1,
                });
            });
    }

    pub(crate) fn open_subscriptions_section(&mut self, force_refresh: bool, ctx: &egui::Context) {
        self.ui.settings_open = false;
        self.ui.search_results_mode = false;
        self.ui.browse_section = BrowseSection::Subscriptions;
        self.close_channel_view();
        if self.selected.is_some() {
            if !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
        } else {
            self.sync_player_window_visibility();
        }
        self.feed.home.title = Some(
            self.pick(
                "Inscrições: envios recentes",
                "Subscriptions: recent uploads",
            )
            .into(),
        );
        self.feed.home.playlist_urls.clear();
        self.feed.subscriptions_continuation = None;
        self.feed.subscriptions_loading_more = false;

        if Self::load_innertube_web_session_cached().is_some() {
            self.ensure_subscriptions_disk_cache_loaded();
            let has_cached = self.feed.home.subscriptions_cached_loaded
                && !self.feed.home.subscriptions_cached_items.is_empty();
            if has_cached {
                let cached = self.feed.home.subscriptions_cached_items.clone();
                self.apply_search_results(ctx, cached);
            }
            let cache_fresh = !force_refresh
                && has_cached
                && self
                    .feed
                    .home
                    .subscriptions_cached_at
                    .is_some_and(|at| at.elapsed() < SUBSCRIPTIONS_CACHE_TTL);
            if !cache_fresh && self.start_innertube_subscriptions_feed(None, false, ctx) {
                return;
            }
            if cache_fresh {
                return;
            }
        }

        if !self.auth.authenticated {
            if self.followed_channels.data.is_empty() {
                self.followed_channels.feed_items.clear();
                self.followed_channels.new_video_urls.clear();
                self.apply_search_results(ctx, Vec::new());
                self.message = self
                    .pick(
                        "Siga canais localmente sem login para acompanhar os envios recentes aqui.",
                        "Follow channels locally without signing in to track recent uploads here.",
                    )
                    .into();
                return;
            }
            let cache_fresh = !force_refresh
                && !self.followed_channels.feed_items.is_empty()
                && self
                    .followed_channels
                    .loaded_at
                    .is_some_and(|at| at.elapsed() < SUBSCRIPTIONS_CACHE_TTL);
            if cache_fresh {
                let items = self.followed_channels.feed_items.clone();
                self.apply_search_results(ctx, items);
                self.message = format!(
                    "{} {}",
                    self.followed_channels.feed_items.len(),
                    self.pick(
                        "envios recentes dos canais seguidos localmente.",
                        "recent uploads from locally followed channels.",
                    )
                );
                return;
            }
            self.start_local_followed_feed(force_refresh, ctx);
            return;
        }

        self.ensure_subscriptions_disk_cache_loaded();

        let has_cached_items = self.feed.home.subscriptions_cached_loaded
            && !self.feed.home.subscriptions_cached_items.is_empty();
        if has_cached_items {
            let cached = self.feed.home.subscriptions_cached_items.clone();
            self.apply_search_results(ctx, cached);
            self.feed.home.subscription_channels_error = None;
        } else if !force_refresh && self.feed.home.stream_task.active_request_id().is_none() {
            self.apply_search_results(ctx, Vec::new());
        }

        let cache_is_fresh = !force_refresh
            && self.feed.home.subscriptions_cached_loaded
            && self
                .feed
                .home
                .subscriptions_cached_at
                .is_some_and(|at| at.elapsed() < SUBSCRIPTIONS_CACHE_TTL);

        if cache_is_fresh {
            self.feed.home.subscriptions_refreshing_in_background = false;
            self.feed.home.subscriptions_progressive_streaming = false;
            self.message = if self.feed.home.subscriptions_cached_items.is_empty() {
                "Nenhum envio encontrado nas inscri\u{00e7}\u{00f5}es consultadas.".into()
            } else {
                format!(
                    "{} envios em cache dos canais que voc\u{00ea} acompanha.",
                    self.feed.home.subscriptions_cached_items.len()
                )
            };
            return;
        }

        if !force_refresh && self.feed.home.stream_task.active_request_id().is_some() {
            if !has_cached_items {
                self.feed.home.subscriptions_progressive_streaming = true;
            }
            return;
        }

        let progressive_streaming = !has_cached_items;
        self.start_fast_subscriptions_feed(force_refresh, progressive_streaming, ctx);
    }

    pub(crate) fn save_followed_channels(&mut self) {
        #[cfg(test)]
        {
            if let Some(path) = self.followed_channels.path.as_deref() {
                if let Err(err) = followed_channels::save(path, &self.followed_channels.data) {
                    self.followed_channels.error = Some(err.to_string());
                } else {
                    self.followed_channels.error = None;
                }
            }
        }
        #[cfg(not(test))]
        {
            let target = self
                .followed_channels
                .path
                .clone()
                .or_else(|| followed_channels::path().ok());
            if let Some(path) = target {
                if let Err(err) = followed_channels::save(&path, &self.followed_channels.data) {
                    self.followed_channels.error = Some(err.to_string());
                } else {
                    self.followed_channels.path = Some(path);
                    self.followed_channels.error = None;
                }
            }
        }
    }

    pub(crate) fn mark_followed_video_seen(&mut self, channel_id: Option<&str>, video_url: &str) {
        let Some(video_id) = history::youtube_video_id(video_url) else {
            return;
        };
        let mut changed = self.followed_channels.new_video_urls.remove(video_url);
        if let Some(cid) = channel_id.map(str::trim).filter(|s| !s.is_empty())
            && self.followed_channels.data.mark_seen(cid, &video_id)
        {
            changed = true;
        }
        if changed {
            self.save_followed_channels();
        }
    }

    pub(crate) fn toggle_local_follow_channel(
        &mut self,
        channel_id: String,
        title: String,
        handle: Option<String>,
        url: String,
        avatar_url: Option<String>,
    ) {
        let target = if channel_id.starts_with("UC") {
            Some(ChannelTarget::ChannelId(channel_id.clone()))
        } else {
            handle.clone().map(ChannelTarget::Handle)
        };
        let is_following = self
            .followed_channels
            .data
            .is_following_target(target.as_ref(), Some(&channel_id));
        if is_following {
            if self
                .followed_channels
                .data
                .unfollow(target.as_ref(), Some(&channel_id))
            {
                self.save_followed_channels();
                let msg = format!(
                    "{}: {title}",
                    self.pick("Deixou de seguir localmente", "Unfollowed locally")
                );
                self.message = msg.clone();
                self.toasts.info(msg);
            }
        } else {
            let entry = followed_channels::FollowedChannel {
                channel_id,
                title: title.clone(),
                handle,
                url,
                avatar_url,
                followed_at_ms: followed_channels::now_ms(),
                last_seen_video_id: None,
            };
            match self.followed_channels.data.follow(entry) {
                Ok(_) => {
                    self.followed_channels.loaded_at = None;
                    self.save_followed_channels();
                    let msg = format!(
                        "{}: {title}",
                        self.pick("Seguindo canal localmente", "Following channel locally")
                    );
                    self.message = msg.clone();
                    self.toasts.success(msg);
                }
                Err(err) => {
                    self.message = err.to_owned();
                    self.toasts.warning(err);
                }
            }
        }
    }

    pub(crate) fn start_local_followed_feed(&mut self, _force_refresh: bool, ctx: &egui::Context) {
        self.followed_channels.feed_task.cancel();
        if self.followed_channels.data.is_empty() {
            self.followed_channels.feed_items.clear();
            self.followed_channels.new_video_urls.clear();
            self.apply_search_results(ctx, Vec::new());
            return;
        }
        let channels = self.followed_channels.data.channels.clone();
        self.message = self
            .pick(
                "Atualizando envios dos canais seguidos...",
                "Refreshing uploads from followed channels...",
            )
            .into();
        self.followed_channels
            .feed_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                followed_channels::fetch_local_followed_feed(&channels, &cancel)
            });
    }

    pub(crate) fn poll_local_followed_feed(&mut self, ctx: &egui::Context) {
        if let async_task::AsyncTaskPoll::Ready(result) =
            self.followed_channels.feed_task.poll_event()
        {
            match result {
                Ok(output) => {
                    let had_prev_load = self.followed_channels.loaded_at.is_some();
                    let prev_new_urls = self.followed_channels.new_video_urls.clone();
                    self.followed_channels.loaded_at = Some(Instant::now());
                    self.followed_channels.feed_items = output.items.clone();
                    self.followed_channels.new_video_urls = output.new_video_urls;
                    if self.settings.prefs.value.windows_toast_notifications && !cfg!(test) {
                        let newly_arrived: Vec<&SearchResult> = self
                            .followed_channels
                            .feed_items
                            .iter()
                            .filter(|item| {
                                self.followed_channels
                                    .new_video_urls
                                    .contains(&item.video.url)
                                    && (!had_prev_load || !prev_new_urls.contains(&item.video.url))
                            })
                            .collect();
                        if let Some(first) = newly_arrived.first() {
                            let count = newly_arrived.len();
                            let title = if count == 1 {
                                if first.metadata.is_live {
                                    self.pick("Canal ao vivo agora", "Channel is live now")
                                        .to_owned()
                                } else {
                                    self.pick("Novo vídeo publicado", "New video uploaded")
                                        .to_owned()
                                }
                            } else if self.ui_pt() {
                                format!("{count} novos vídeos de canais seguidos")
                            } else {
                                format!("{count} new videos from followed channels")
                            };
                            let body = if first.video.channel.trim().is_empty() {
                                first.video.title.clone()
                            } else {
                                format!("{} — {}", first.video.channel, first.video.title)
                            };
                            let _ =
                                win11::show_windows_toast(&title, &body, Some(&first.video.url));
                        }
                    }
                    if self.ui.browse_section == BrowseSection::Subscriptions
                        && !self.auth.authenticated
                        && !self.ui.search_results_mode
                        && !self.channels.view_open
                    {
                        let count = output.items.len();
                        self.apply_search_results(ctx, output.items);
                        self.message = if count == 0 {
                            self.pick(
                                "Nenhum envio recente encontrado nos canais seguidos.",
                                "No recent uploads found in followed channels.",
                            )
                            .into()
                        } else if output.errors > 0 {
                            format!(
                                "{count} {} ({} {}).",
                                self.pick("envios carregados", "uploads loaded"),
                                output.errors,
                                self.pick("falha(s) de canal", "channel failure(s)")
                            )
                        } else {
                            format!(
                                "{count} {}",
                                self.pick(
                                    "envios recentes dos canais seguidos.",
                                    "recent uploads from followed channels.",
                                )
                            )
                        };
                    }
                }
                Err(error) => {
                    self.followed_channels.error = Some(error.clone());
                    if self.ui.browse_section == BrowseSection::Subscriptions
                        && !self.auth.authenticated
                    {
                        self.message = error;
                    }
                }
            }
        }
    }

    pub(crate) fn start_subscription_channels_fetch(&mut self, ctx: &egui::Context) {
        if self.feed.home.channels_task.is_running() || !self.auth.authenticated {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        self.feed
            .home
            .channels_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| oauth::list_subscriptions(&session.access_token, &cancel));
                (Some(session), result)
            });
    }

    pub(crate) fn build_fast_home_query_plan(&self, page: usize) -> recommendations::HomeQueryPlan {
        let prefs = &self.settings.prefs.value;
        let mut profile = if prefs.pause_recommendations {
            UserProfile::default()
        } else {
            UserProfile::build(&self.history.data, &self.queue.data, self.selected.as_ref())
        };
        if !prefs.muted_topics.is_empty() {
            profile.apply_muted_topics(&prefs.muted_topics);
        }
        if !prefs.muted_channels.is_empty() {
            profile.apply_muted_channels(&prefs.muted_channels);
        }

        let mut seen_channel_ids = HashSet::new();
        let mut seed_channels = Vec::with_capacity(
            self.feed.home.subscription_channels.len() + self.followed_channels.data.channels.len(),
        );
        let is_muted_channel = |title: &str| {
            let key = recommendations::normalize_channel_key(title);
            !key.is_empty() && profile.muted_channels.contains(&key)
        };
        for ch in &self.feed.home.subscription_channels {
            if is_muted_channel(&ch.title) {
                continue;
            }
            if !ch.channel_id.is_empty() && !seen_channel_ids.insert(ch.channel_id.clone()) {
                continue;
            }
            seed_channels.push(ch.clone());
        }
        for ch in &self.followed_channels.data.channels {
            if is_muted_channel(&ch.title) {
                continue;
            }
            if !ch.channel_id.is_empty() && !seen_channel_ids.insert(ch.channel_id.clone()) {
                continue;
            }
            seed_channels.push(search::HomeChannel {
                title: ch.title.clone(),
                channel_id: ch.channel_id.clone(),
                avatar_url: ch.avatar_url.clone(),
            });
        }

        RecommendationEngine::build_home_query_plan(
            &profile,
            &seed_channels,
            &self.feed.filter,
            page,
        )
    }

    pub(crate) fn start_fast_home_feed(&mut self, append: bool, ctx: &egui::Context) {
        if append && (self.feed.home.batch_task.is_running() || !self.feed.home.has_more) {
            return;
        }
        let page = if append { self.feed.home.page } else { 0 };
        self.feed.home.title = None;
        self.feed.home.limit_reached = false;
        if !append && self.feed.videos.is_empty() {
            self.feed.home.has_more = true;
        }

        let plan = self.build_fast_home_query_plan(page);
        let request = search::HomeBatchRequest {
            video_queries: plan.video_queries,
            channel_targets: plan.channel_targets,
            playlist_query: plan.playlist_query,
            shorts_query: plan.shorts_query,
        };
        self.feed.home.batch_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result = search::fetch_home_batch(&request, &cancel);
                HomeBatchFetchMessage {
                    request_id,
                    page,
                    result,
                }
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn start_public_home(&mut self, ctx: &egui::Context) {
        if self.feed.search.task.is_running()
            || self.feed.home.stream_task.is_running()
            || self.feed.home.batch_task.is_running()
        {
            return;
        }
        self.ui.browse_section = BrowseSection::Home;
        self.ui.search_results_mode = false;
        self.feed.filter = FeedFilter::All;
        if self.restore_cached_home_feed(ctx) {
            return;
        }
        self.start_fast_home_feed(false, ctx);
    }

    pub(crate) fn clear_visual_caches(&mut self, ctx: &egui::Context) {
        let current_videos = std::mem::take(&mut self.feed.videos);
        self.thumbnails.reset(ctx, &current_videos);
        self.thumbnails.clear_storyboards(ctx);
        self.avatars.evict_all_gpu_textures(ctx);
        self.feed.videos = current_videos;
        self.playback.qualities.clear();
        search::clear_in_memory_caches();
        self.prune_session_caches();
        self.feed.cache_dirty = true;
        self.settings.buffer.notice = Some(
            "Miniaturas e resoluções em cache foram liberadas. O feed continua disponível e os dados locais não foram alterados."
                .into(),
        );
    }

    pub(crate) fn start_search(&mut self, ctx: &egui::Context) {
        let Some((_request_id, query)) = self.begin_search_request() else {
            return;
        };
        let filters = search::effective_search_filters(
            self.feed.search_filters.clone(),
            self.feed.search_chip,
        );
        let language = self.settings.prefs.value.language;
        let web_session = Self::load_innertube_web_session_cached();
        self.feed.search_page_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |page_req_id, cancel| {
                let result = search::search_videos_innertube(
                    &query,
                    &filters,
                    None,
                    language,
                    None,
                    web_session.as_ref(),
                    &cancel,
                );
                app_state::SearchPageFetchMessage {
                    request_id: page_req_id,
                    append: false,
                    result,
                }
            },
        );
    }

    pub(crate) fn start_browser_feed(
        &mut self,
        target: String,
        interactive: bool,
        ctx: &egui::Context,
    ) {
        if target == ":google-liked" && !interactive && self.auth.authenticated {
            self.start_liked_videos_fetch(false, ctx);
            return;
        }
        if target == ":ytrec" && self.feed.home.stream_task.is_running() {
            return;
        }
        self.feed.home.stream_task.cancel();
        self.feed.home.batch_task.cancel();
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        self.feed.search.task.cancel();
        self.ui.search_results_mode = false;
        self.close_channel_view();
        self.feed.home.playlist_urls.clear();
        self.feed.home.playlist_total_results = None;
        self.feed.home.limit_reached = false;
        self.feed.home.pending_items.clear();
        if target == ":ytrec" {
            self.feed.home.subscription_channels_error = None;
        }
        self.apply_search_results(ctx, Vec::new());
        let title = if target == ":ytrec" {
            "Inscrições: envios recentes"
        } else if target == ":google-playlists" {
            "Playlists da conta Google"
        } else if target == ":google-liked" {
            "Vídeos com Gostei"
        } else if target.starts_with("fasttube-google-playlist:") {
            "Vídeos da playlist"
        } else {
            "Playlist do YouTube"
        };
        self.feed.home.title = Some(title.into());
        self.message = if target == ":ytrec" {
            "Consultando suas inscrições..."
        } else if target == ":google-playlists" || target.starts_with("fasttube-google-playlist:") {
            "Consultando suas playlists Google..."
        } else if target == ":google-liked" {
            "Consultando vídeos com Gostei..."
        } else {
            "Consultando a playlist..."
        }
        .into();
        let saved_session = self.auth.session.clone();
        self.feed
            .home
            .stream_task
            .spawn_stream(ctx.clone(), move |cancel, sender| {
                let is_oauth_target = target == ":ytrec"
                    || target == ":google-playlists"
                    || target == ":google-liked"
                    || target.starts_with("fasttube-google-playlist:");
                let result = if is_oauth_target {
                    let (token_opt, ready_session) = match saved_session {
                        Some(mut session) => match session.refresh_if_needed(&cancel) {
                            Ok(()) => (Some(session.clone()), Ok(session)),
                            Err(err) => (Some(session), Err(err)),
                        },
                        None if interactive => match oauth::authorize(&cancel) {
                            Ok(session) => (Some(session.clone()), Ok(session)),
                            Err(err) => (None, Err(err)),
                        },
                        None => (
                            None,
                            Err("Entre na conta para consultar suas inscrições.".into()),
                        ),
                    };
                    match ready_session {
                        Ok(session) => {
                            if let Some(id) = target.strip_prefix("fasttube-google-playlist:") {
                                let progress_sender = sender.clone();
                                let outcome = oauth::playlist_videos_feed_progressive(
                                    &session.access_token,
                                    id,
                                    &cancel,
                                    move |progress| {
                                        let _ = progress_sender.send(
                                            HomeFetchMessage::PlaylistProgress {
                                                batch: progress.batch,
                                                total_results: progress.total_results,
                                            },
                                        );
                                    },
                                );
                                let (total_results, res) = match outcome {
                                    Ok(out) => (out.total_results, Ok(())),
                                    Err(err) => (None, Err(err)),
                                };
                                let _ = sender.send(HomeFetchMessage::PlaylistFinished {
                                    token: token_opt,
                                    total_results,
                                    result: res,
                                });
                                return;
                            }
                            let feed = if target == ":google-playlists" {
                                oauth::playlists_feed(&session.access_token, &cancel)
                            } else if target == ":google-liked" {
                                oauth::liked_videos_feed(&session.access_token, &cancel)
                            } else {
                                let progress_sender = sender.clone();
                                oauth::subscriptions_feed(
                                    &session.access_token,
                                    &cancel,
                                    move |progress| {
                                        let _ = progress_sender
                                            .send(HomeFetchMessage::Progress(progress));
                                    },
                                )
                            };
                            (token_opt, feed)
                        }
                        Err(error) => {
                            if target.starts_with("fasttube-google-playlist:") {
                                let _ = sender.send(HomeFetchMessage::PlaylistFinished {
                                    token: token_opt,
                                    total_results: None,
                                    result: Err(error),
                                });
                                return;
                            }
                            (token_opt, Err(error))
                        }
                    }
                } else {
                    let feed = session::youtube_cookies(false, &cancel)
                        .and_then(|cookies| search::session_feed(&cookies, &target, &cancel));
                    (saved_session, feed)
                };
                let _ = sender.send(HomeFetchMessage::Finished(result.0, result.1));
            });
    }

    #[cfg(test)]
    pub(crate) fn cancel_home_feed(&mut self, ctx: &egui::Context) {
        self.feed.home.stream_task.cancel();
        self.feed.home.subscriptions_progressive_streaming = false;
        self.feed.home.subscriptions_refreshing_in_background = false;
        if !self.feed.home.pending_items.is_empty() {
            let partial = std::mem::take(&mut self.feed.home.pending_items);
            self.apply_search_results(ctx, partial);
        }
        self.message =
            "Carregamento cancelado. Os resultados já recebidos permanecem disponíveis.".into();
    }

    pub(crate) fn poll_subscription_channels(&mut self) {
        if let Some((updated_session, result)) = self.feed.home.channels_task.poll() {
            if let Some(session) = updated_session
                && self.auth.session.is_some()
            {
                let _ = self.set_oauth_session(Some(session));
            }
            match result {
                Ok(channels) => {
                    self.feed.home.subscription_channels = channels;
                    self.feed.home.subscription_channels_loaded = true;
                    self.feed.home.subscription_channels_error = None;
                    self.populate_channel_avatars_from_subscriptions();
                }
                Err(error) => {
                    self.feed.home.subscription_channels_error = Some(error);
                }
            }
        }
    }

    pub(crate) fn poll_home_batch(&mut self, ctx: &egui::Context) {
        if let async_task::AsyncTaskPoll::Ready(msg) = self
            .feed
            .home
            .batch_task
            .poll_matching_id(|msg| msg.request_id)
        {
            match msg.result {
                Ok(feed) => {
                    let fetched_count = feed.items.len();
                    let mut existing_keys: std::collections::HashSet<String> = self
                        .feed
                        .home
                        .cached_items
                        .iter()
                        .map(|item| {
                            history::youtube_video_id(&item.video.url)
                                .unwrap_or_else(|| item.video.url.clone())
                        })
                        .collect();
                    let mut existing_playlists: std::collections::HashSet<String> =
                        self.feed.home.cached_playlists.iter().cloned().collect();
                    for playlist in feed.playlists {
                        if existing_playlists.insert(playlist.clone()) {
                            self.feed.home.cached_playlists.push(playlist);
                        }
                    }
                    let mut added_count = 0usize;
                    for item in feed.items {
                        if self.feed.home.cached_items.len() >= MAX_HOME_CACHED_ITEMS {
                            break;
                        }
                        let key = history::youtube_video_id(&item.video.url)
                            .unwrap_or_else(|| item.video.url.clone());
                        if existing_keys.insert(key) {
                            self.feed.home.cached_items.push(item);
                            added_count += 1;
                        }
                    }
                    if self.feed.home.cached_items.len() > MAX_HOME_CACHED_ITEMS {
                        self.feed.home.cached_items.truncate(MAX_HOME_CACHED_ITEMS);
                    }
                    self.feed.home.cached_loaded = true;
                    self.feed.home.page = msg.page.saturating_add(1);
                    self.feed.home.has_more = fetched_count > 0
                        && added_count > 0
                        && self.feed.home.cached_items.len() < MAX_HOME_CACHED_ITEMS;
                    let should_apply = (self.ui.browse_section == BrowseSection::Home
                        || self.ui.browse_section == BrowseSection::Shorts)
                        && !self.ui.search_results_mode
                        && !self.channels.view_open
                        && self.feed.home.title.is_none();
                    if should_apply {
                        self.feed.home.playlist_urls = self.feed.home.cached_playlists.clone();
                        let cached = self.feed.home.cached_items.clone();
                        self.apply_search_results(ctx, cached);
                    }
                }
                Err(error) => {
                    self.feed.home.has_more = false;
                    if self.feed.videos.is_empty()
                        && self.ui.browse_section == BrowseSection::Home
                        && !self.ui.search_results_mode
                    {
                        self.message = error;
                    }
                }
            }
        }
    }

    pub(crate) fn poll_home(&mut self, ctx: &egui::Context) {
        let active_stream_id = self.feed.home.stream_task.active_request_id();
        let poll = self.feed.home.stream_task.poll_batch();
        let home_messages = poll.items;
        let home_disconnected = poll.finished;
        let mut stream_completed_cleanly = false;
        for home_message in home_messages {
            match home_message {
                HomeFetchMessage::Progress(progress) => {
                    if self.ui.browse_section == BrowseSection::Subscriptions {
                        self.feed.home.subscription_channels = progress.channels.clone();
                        self.feed.home.subscription_channels_loaded = true;
                        self.feed.home.subscription_channels_error = None;
                        self.populate_channel_avatars_from_subscriptions();
                    }
                    self.feed.home.limit_reached |= progress.reached_limit;
                    self.feed.home.pending_items = progress.items;
                    self.message = if self.feed.home.limit_reached {
                        format!(
                            "Carregados {} canais; limite de 100 canais atingido. Algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.",
                            progress.completed_channels
                        )
                    } else {
                        format!(
                            "Carregando envios: {} de {} canais.",
                            progress.completed_channels, progress.total_channels
                        )
                    };
                }
                HomeFetchMessage::Finished(token, Ok(feed)) => {
                    self.feed.home.pending_items.clear();
                    let was_authenticated = self.auth.authenticated;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    let is_subscription_feed =
                        self.ui.browse_section == BrowseSection::Subscriptions;
                    let is_playlists_feed =
                        self.feed.home.title.as_deref() == Some("Playlists da conta Google");
                    self.feed.home.stream_task.cancel();
                    stream_completed_cleanly = true;
                    if is_subscription_feed {
                        self.feed.home.subscription_channels = feed.channels.clone();
                        self.feed.home.subscription_channels_loaded = true;
                        self.feed.home.subscription_channels_error = None;
                        self.populate_channel_avatars_from_subscriptions();
                        self.feed.home.subscriptions_cached_items = feed.items.clone();
                        self.feed.home.subscriptions_cached_loaded = true;
                        self.feed.home.subscriptions_cached_at = Some(Instant::now());
                        self.persist_subscriptions_disk_cache();
                    }
                    if !was_authenticated && self.auth.authenticated {
                        if self.auth.my_channel_avatar_url.is_none()
                            && !self.auth.my_channel_fetch_attempted
                        {
                            self.start_fetch_my_channel(false, ctx);
                        }
                        if !self.feed.home.subscription_channels_loaded {
                            self.start_subscription_channels_fetch(ctx);
                        }
                    }
                    if is_playlists_feed {
                        self.auth.remote_playlists = feed
                            .items
                            .iter()
                            .filter_map(|item| {
                                item.video
                                    .url
                                    .strip_prefix("fasttube-google-playlist:")
                                    .map(|id| (id.to_owned(), item.video.title.clone()))
                            })
                            .collect();
                    }
                    self.feed.home.playlist_urls = feed.playlists;
                    self.apply_search_results(ctx, feed.items);
                    if self.feed.home.limit_reached {
                        self.message = "Limite de 100 canais por atualiza\u{e7}\u{e3}o atingido; algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.".into();
                    } else if self.feed.videos.is_empty() {
                        self.message = if self.feed.home.title.as_deref()
                            == Some("Playlists da conta Google")
                        {
                            "Nenhuma playlist encontrada nesta conta.".into()
                        } else if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
                            "Nenhum vídeo encontrado nesta playlist.".into()
                        } else if self.feed.home.title.as_deref() == Some("Vídeos com Gostei") {
                            "Nenhum vídeo com Gostei encontrado.".into()
                        } else {
                            "Nenhum envio encontrado nas inscri\u{e7}\u{f5}es consultadas.".into()
                        };
                    }
                    if let Some(error) = persistence_error {
                        self.message = format!("{} (sessao local: {error})", self.message);
                    }
                }
                HomeFetchMessage::Finished(token, Err(error)) => {
                    let is_subscription_feed =
                        self.ui.browse_section == BrowseSection::Subscriptions;
                    if is_subscription_feed {
                        self.feed.home.subscription_channels_error = Some(error.clone());
                    }
                    if !self.feed.home.pending_items.is_empty() {
                        let partial = std::mem::take(&mut self.feed.home.pending_items);
                        self.apply_search_results(ctx, partial);
                    }
                    self.feed.home.stream_task.cancel();
                    stream_completed_cleanly = true;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    self.message = persistence_error
                        .map(|save_error| format!("{error} ({save_error})"))
                        .unwrap_or(error);
                }
                HomeFetchMessage::SubscriptionProgress {
                    request_id,
                    progress,
                } => {
                    if active_stream_id != Some(request_id) {
                        continue;
                    }
                    if !progress.channels.is_empty() {
                        self.feed.home.subscription_channels = progress.channels.clone();
                        self.feed.home.subscription_channels_loaded = true;
                        self.feed.home.subscription_channels_error = None;
                        self.populate_channel_avatars_from_subscriptions();
                    }
                    self.feed.home.limit_reached |= progress.reached_limit;
                    let viewing_subscriptions = self.ui.browse_section
                        == BrowseSection::Subscriptions
                        && !self.ui.search_results_mode
                        && !self.channels.view_open;
                    if self.feed.home.subscriptions_progressive_streaming && viewing_subscriptions {
                        if !progress.items.is_empty() {
                            self.apply_search_results(ctx, progress.items.clone());
                        }
                        self.feed.home.pending_items = progress.items;
                    } else {
                        self.feed.home.pending_items = progress.items;
                    }
                    if viewing_subscriptions {
                        self.message = if self.feed.home.limit_reached {
                            format!(
                                "Carregados {} canais; limite de 100 canais atingido. Algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.",
                                progress.completed_channels
                            )
                        } else if self.feed.home.subscriptions_refreshing_in_background {
                            format!(
                                "Atualizando inscri\u{00e7}\u{f5}es em segundo plano ({} de {} canais)...",
                                progress.completed_channels, progress.total_channels
                            )
                        } else {
                            format!(
                                "Carregando envios: {} de {} canais.",
                                progress.completed_channels, progress.total_channels
                            )
                        };
                    }
                }
                HomeFetchMessage::SubscriptionFinished {
                    request_id,
                    token,
                    result,
                } => {
                    if active_stream_id != Some(request_id) {
                        continue;
                    }
                    self.feed.home.stream_task.cancel();
                    stream_completed_cleanly = true;
                    let was_progressive =
                        std::mem::take(&mut self.feed.home.subscriptions_progressive_streaming);
                    self.feed.home.subscriptions_refreshing_in_background = false;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    let viewing_subscriptions = self.ui.browse_section
                        == BrowseSection::Subscriptions
                        && !self.ui.search_results_mode
                        && !self.channels.view_open;
                    match result {
                        Ok(output) => {
                            self.feed.home.pending_items.clear();
                            self.feed.home.subscription_channels = output.feed.channels.clone();
                            self.feed.home.subscription_channels_loaded = true;
                            self.feed.home.subscription_channels_error = None;
                            self.populate_channel_avatars_from_subscriptions();
                            self.feed.home.subscriptions_cached_items = output.feed.items.clone();
                            self.feed.home.subscriptions_cached_loaded = true;
                            self.feed.home.subscriptions_cached_at = Some(Instant::now());
                            self.feed.home.subscriptions_rss_cache = output.rss_cache;
                            self.persist_subscriptions_disk_cache();
                            if viewing_subscriptions {
                                self.feed.home.playlist_urls = output.feed.playlists;
                                self.apply_search_results(ctx, output.feed.items);
                                if self.feed.home.limit_reached {
                                    self.message = "Limite de 100 canais por atualiza\u{e7}\u{e3}o atingido; algumas inscri\u{e7}\u{f5}es podem n\u{e3}o aparecer.".into();
                                } else if self.feed.videos.is_empty() {
                                    self.message = "Nenhum envio encontrado nas inscri\u{e7}\u{f5}es consultadas.".into();
                                }
                                if let Some(error) = persistence_error {
                                    self.message =
                                        format!("{} (sessao local: {error})", self.message);
                                }
                            }
                        }
                        Err(error) => {
                            self.feed.home.subscription_channels_error = Some(error.clone());
                            let partial = std::mem::take(&mut self.feed.home.pending_items);
                            if viewing_subscriptions {
                                if !partial.is_empty()
                                    && (was_progressive || self.feed.videos.is_empty())
                                {
                                    self.apply_search_results(ctx, partial);
                                }
                                self.message = persistence_error
                                    .map(|save_error| format!("{error} ({save_error})"))
                                    .unwrap_or(error);
                            }
                        }
                    }
                }
                HomeFetchMessage::PlaylistProgress {
                    batch,
                    total_results,
                } => {
                    if self.feed.home.title.as_deref() != Some("Vídeos da playlist") {
                        continue;
                    }
                    if total_results.is_some() {
                        self.feed.home.playlist_total_results = total_results;
                    }
                    if !batch.is_empty() {
                        for item in batch {
                            self.video_metadata
                                .insert(item.video.url.clone(), item.metadata);
                            self.feed.videos.push(item.video);
                        }
                        self.feed.generation = self.feed.generation.wrapping_add(1);
                        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                        self.feed.cache_dirty = true;
                        self.message.clear();
                    }
                }
                HomeFetchMessage::PlaylistFinished {
                    token,
                    total_results,
                    result,
                } => {
                    self.feed.home.stream_task.cancel();
                    stream_completed_cleanly = true;
                    self.auth.authenticated = token.is_some();
                    let persistence_error = self.set_oauth_session(token);
                    if total_results.is_some() {
                        self.feed.home.playlist_total_results = total_results;
                    }
                    match result {
                        Ok(()) => {
                            if self.feed.videos.is_empty() {
                                self.message = "Nenhum vídeo encontrado nesta playlist.".into();
                            } else {
                                self.message.clear();
                            }
                            if let Some(error) = persistence_error {
                                self.message = format!("{} (sessao local: {error})", self.message);
                            }
                        }
                        Err(error) => {
                            self.message = persistence_error
                                .map(|save_error| format!("{error} ({save_error})"))
                                .unwrap_or(error);
                        }
                    }
                }
            }
        }
        if home_disconnected && !stream_completed_cleanly {
            self.feed.home.subscriptions_progressive_streaming = false;
            self.feed.home.subscriptions_refreshing_in_background = false;
            if !self.feed.home.pending_items.is_empty() {
                let partial = std::mem::take(&mut self.feed.home.pending_items);
                self.apply_search_results(ctx, partial);
            }
            self.message = "A consulta foi interrompida.".into();
        }
    }

    pub(crate) fn poll_search(&mut self, ctx: &egui::Context) {
        let had_active = self.feed.search.task.is_active();
        match self.feed.search.task.poll_matching_id(|msg| msg.request_id) {
            async_task::AsyncTaskPoll::Ready(msg) => {
                self.apply_search_request(msg.result, ctx);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if had_active {
                    self.message = "A pesquisa foi interrompida.".into();
                }
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn select_explore_category(
        &mut self,
        category: innertube::ExploreCategory,
        ctx: &egui::Context,
    ) {
        self.feed.explore_task.cancel();
        self.feed.explore_category = category;
        self.feed.explore_loaded_category = Some(category);
        self.feed.explore_continuation = None;
        self.feed.explore_loading_more = false;
        if cfg!(test) {
            self.feed.explore_loading = false;
            return;
        }
        let it_ctx = self.innertube_context();
        let mut session = Self::load_innertube_web_session_cached();
        self.feed.explore_loading = true;
        self.feed.videos.clear();
        self.invalidate_feed_cache();
        self.feed
            .explore_task
            .spawn_with_id_and_cancel(ctx.clone(), move |request_id, cancel| {
                let result = innertube::fetch_explore_hub(
                    category,
                    None,
                    &it_ctx,
                    session.as_mut(),
                    &cancel,
                )
                .map_err(|e| e.into_message());
                app_state::ExploreHubFetchMessage {
                    request_id,
                    append: false,
                    result,
                }
            });
    }

    pub(crate) fn start_load_more_explore(&mut self, ctx: &egui::Context) {
        if self.feed.explore_loading || self.feed.explore_loading_more {
            return;
        }
        let Some(token) = self.feed.explore_continuation.clone() else {
            return;
        };
        if cfg!(test) {
            return;
        }
        let category = self.feed.explore_category;
        let it_ctx = self.innertube_context();
        let mut session = Self::load_innertube_web_session_cached();
        self.feed.explore_loading_more = true;
        self.feed
            .explore_task
            .spawn_with_id_and_cancel(ctx.clone(), move |request_id, cancel| {
                let result = innertube::fetch_explore_hub(
                    category,
                    Some(&token),
                    &it_ctx,
                    session.as_mut(),
                    &cancel,
                )
                .map_err(|e| e.into_message());
                app_state::ExploreHubFetchMessage {
                    request_id,
                    append: true,
                    result,
                }
            });
    }

    pub(crate) fn poll_explore_hub(&mut self, ctx: &egui::Context) {
        match self
            .feed
            .explore_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                self.feed.explore_loading = false;
                self.feed.explore_loading_more = false;
                match msg.result {
                    Ok(page) => {
                        self.feed.explore_continuation = page.continuation;
                        let results: Vec<SearchResult> = page
                            .items
                            .into_iter()
                            .zip(page.metadata)
                            .map(|(video, metadata)| SearchResult { video, metadata })
                            .collect();
                        if msg.append {
                            let mut seen: HashSet<String> = self
                                .feed
                                .videos
                                .iter()
                                .map(|v| {
                                    history::youtube_video_id(&v.url)
                                        .unwrap_or_else(|| v.url.clone())
                                })
                                .collect();
                            for item in results {
                                let key = history::youtube_video_id(&item.video.url)
                                    .unwrap_or_else(|| item.video.url.clone());
                                if !seen.insert(key) {
                                    continue;
                                }
                                if let Some(ts) = item.metadata.upcoming_start_unix {
                                    self.channels
                                        .upcoming_start_unix_by_url
                                        .insert(item.video.url.clone(), ts);
                                }
                                self.video_metadata
                                    .insert(item.video.url.clone(), item.metadata);
                                self.feed.videos.push(item.video);
                            }
                            self.feed.generation = self.feed.generation.wrapping_add(1);
                            self.invalidate_feed_cache();
                        } else {
                            for item in &results {
                                if let Some(ts) = item.metadata.upcoming_start_unix {
                                    self.channels
                                        .upcoming_start_unix_by_url
                                        .insert(item.video.url.clone(), ts);
                                }
                            }
                            self.apply_search_results(ctx, results);
                        }
                    }
                    Err(err) => {
                        self.message = err;
                    }
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.feed.explore_loading = false;
                self.feed.explore_loading_more = false;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn upcoming_start_unix_for(&self, video_url: &str) -> Option<i64> {
        self.channels
            .upcoming_start_unix_by_url
            .get(video_url)
            .copied()
            .or_else(|| {
                self.video_metadata
                    .get(video_url)
                    .and_then(|m| m.upcoming_start_unix)
            })
    }

    pub(crate) fn toggle_upcoming_video_notify(
        &mut self,
        video_url: &str,
        title: &str,
        _start_unix: Option<i64>,
    ) {
        let pt = self.ui_pt();
        if self.channels.upcoming_notify_urls.contains(video_url) {
            self.channels.upcoming_notify_urls.remove(video_url);
            self.toasts.info(if pt {
                format!("Lembrete removido: {title}")
            } else {
                format!("Reminder removed: {title}")
            });
        } else {
            self.channels
                .upcoming_notify_urls
                .insert(video_url.to_owned());
            self.toasts.success(if pt {
                format!("Lembrete ativado: {title}")
            } else {
                format!("Reminder set: {title}")
            });
        }
    }

    pub(crate) fn prefetch_next_short(&mut self, ctx: &egui::Context) {
        if let Some(next_short) = self.next_or_prev_short(true)
            && let Some(video_id) = history::youtube_video_id(&next_short.url)
        {
            self.playback
                .stream_prefetcher
                .prefetch(&video_id, &next_short.url);
        }
        let near_end = match self.short_index_and_total() {
            Some((idx, total)) => total.saturating_sub(idx) <= 2,
            None => self.shorts_feed_candidates().len() <= 2,
        };
        if near_end && !cfg!(test) && !self.feed.shorts_task.is_running() {
            self.start_shorts_sequence_fetch(true, ctx);
        }
    }

    pub(crate) fn start_shorts_sequence_fetch(&mut self, append: bool, ctx: &egui::Context) {
        if cfg!(test) {
            return;
        }
        let continuation = append
            .then(|| self.feed.shorts_continuation.clone())
            .flatten();
        let it_ctx = self.innertube_context();
        let mut session = Self::load_innertube_web_session_cached();
        self.feed
            .shorts_task
            .spawn_with_id_and_cancel(ctx.clone(), move |request_id, cancel| {
                let result = innertube::fetch_shorts_sequence(
                    continuation.as_deref(),
                    &it_ctx,
                    session.as_mut(),
                    &cancel,
                )
                .map_err(|e| e.into_message());
                app_state::ShortsSequenceFetchMessage { request_id, result }
            });
    }

    pub(crate) fn poll_shorts_sequence(&mut self, _ctx: &egui::Context) {
        if let async_task::AsyncTaskPoll::Ready(msg) =
            self.feed.shorts_task.poll_matching_id(|msg| msg.request_id)
            && let Ok(page) = msg.result
        {
            self.feed.shorts_continuation = page.continuation;
            let mut seen: HashSet<String> = self
                .feed
                .videos
                .iter()
                .map(|v| history::youtube_video_id(&v.url).unwrap_or_else(|| v.url.clone()))
                .collect();
            let mut added = 0usize;
            for (video, metadata) in page.items.into_iter().zip(page.metadata) {
                let key =
                    history::youtube_video_id(&video.url).unwrap_or_else(|| video.url.clone());
                if !seen.insert(key) {
                    continue;
                }
                self.video_metadata.insert(video.url.clone(), metadata);
                self.feed.videos.push(video);
                added += 1;
            }
            if added > 0 {
                self.feed.generation = self.feed.generation.wrapping_add(1);
                self.invalidate_feed_cache();
                self.maybe_auto_open_first_short_in_tab();
            }
        }
    }

    pub(crate) fn maybe_auto_open_first_short_in_tab(&mut self) {
        if self.ui.browse_section == BrowseSection::Shorts
            && !self.ui.search_results_mode
            && !self.channels.view_open
            && !self.ui.settings_open
            && !self.is_watch_page_active()
            && self.playback.pending_open_video.is_none()
            && let Some(first_short) = self.shorts_feed_candidates().first().map(|v| (*v).clone())
        {
            self.playback.pending_open_keep_mini_player = false;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(first_short);
        }
    }

    pub(crate) fn open_shorts_tab(&mut self, ctx: &egui::Context) {
        self.ui.settings_open = false;
        self.ui.search_results_mode = false;
        self.queue.remote_playlist_id = None;
        self.ui.browse_section = BrowseSection::Shorts;
        self.feed.filter = FeedFilter::Shorts;
        self.close_channel_view();
        if self.selected.is_some() && self.is_watch_short_mode() {
            self.exit_in_app_mini_player(ctx);
            if !cfg!(test)
                && !self.feed.shorts_task.is_running()
                && self.shorts_feed_candidates().len() < 4
            {
                self.start_shorts_sequence_fetch(false, ctx);
            }
            return;
        }
        if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
            let _ = self.restore_cached_home_feed(ctx);
        }
        self.invalidate_feed_cache();
        self.ensure_feed_cache();
        if let Some(first_short) = self.shorts_feed_candidates().first().map(|v| (*v).clone()) {
            self.playback.pending_open_keep_mini_player = false;
            self.playback.pending_open_suppress_back_stack = true;
            self.playback.pending_open_video = Some(first_short);
        } else if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
            self.close_video(ctx);
        } else if self.selected.is_none() {
            self.sync_player_window_visibility();
        }
        if self.shorts_feed_candidates().len() < 4
            && !self.feed.home.batch_task.is_running()
            && !self.feed.home.stream_task.is_running()
            && !self.feed.home_innertube_task.is_running()
        {
            self.start_fast_home_feed(false, ctx);
        }
        if !cfg!(test)
            && !self.feed.shorts_task.is_running()
            && self.shorts_feed_candidates().len() < 4
        {
            self.start_shorts_sequence_fetch(false, ctx);
        }
    }

    #[cfg(not(test))]
    pub(crate) fn hydrate_home_from_swr_cache(&mut self, ctx: &egui::Context) -> bool {
        let Ok(path) = innertube::HomeSwrCache::home_cache_path() else {
            return false;
        };
        self.hydrate_home_from_swr_cache_at(&path, ctx)
    }

    pub(crate) fn hydrate_home_from_swr_cache_at(
        &mut self,
        path: &Path,
        ctx: &egui::Context,
    ) -> bool {
        let Ok(Some(cache)) = innertube::HomeSwrCache::load_from(path) else {
            return false;
        };
        if cache.items.is_empty() || !cache.matches_language(self.settings.prefs.value.language) {
            return false;
        }
        self.apply_innertube_home_page(cache.into_page(), false, true, ctx);
        true
    }

    pub(crate) fn apply_innertube_home_page(
        &mut self,
        page: innertube::InnerTubeHomePage,
        append: bool,
        update_chips: bool,
        ctx: &egui::Context,
    ) {
        let results: Vec<SearchResult> = page
            .items
            .into_iter()
            .zip(page.metadata)
            .map(|(video, metadata)| SearchResult { video, metadata })
            .collect();
        self.feed.home_continuation = page.continuation;
        if update_chips && !page.chips.is_empty() {
            self.feed.home_chips = page.chips;
        }
        if append {
            let mut seen: HashSet<String> = self
                .feed
                .videos
                .iter()
                .map(|v| history::youtube_video_id(&v.url).unwrap_or_else(|| v.url.clone()))
                .collect();
            let mut added = 0usize;
            for item in results {
                let key = history::youtube_video_id(&item.video.url)
                    .unwrap_or_else(|| item.video.url.clone());
                if !seen.insert(key) {
                    continue;
                }
                self.video_metadata
                    .insert(item.video.url.clone(), item.metadata.clone());
                if self.feed.home.cached_items.len() < MAX_HOME_CACHED_ITEMS {
                    self.feed.home.cached_items.push(item.clone());
                }
                self.feed.videos.push(item.video);
                added += 1;
            }
            if added > 0 {
                self.feed.generation = self.feed.generation.wrapping_add(1);
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                self.feed.cache_dirty = true;
                self.maybe_auto_open_first_short_in_tab();
            }
        } else if !results.is_empty() {
            self.feed.home.title = None;
            self.feed.home.cached_items = results
                .iter()
                .take(MAX_HOME_CACHED_ITEMS)
                .cloned()
                .collect();
            self.feed.home.cached_loaded = true;
            self.apply_search_results(ctx, results);
        }
    }

    pub(crate) fn start_innertube_home_feed(
        &mut self,
        continuation: Option<String>,
        append: bool,
        is_default_home: bool,
        ctx: &egui::Context,
    ) {
        if append {
            self.feed.home_loading_more = true;
        }
        let it_ctx = self.innertube_context();
        let language = self.settings.prefs.value.language;
        let mut web_session = Self::load_innertube_web_session_cached();
        self.feed.home_innertube_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let mut outcome = innertube::fetch_home_feed(
                    &it_ctx,
                    web_session.as_mut(),
                    continuation.as_deref(),
                    &cancel,
                );
                if let Err(ref err) = outcome
                    && err.is_auth_expired()
                    && web_session.is_some()
                {
                    #[cfg(not(test))]
                    let _ = innertube::invalidate_web_session();
                    outcome =
                        innertube::fetch_home_feed(&it_ctx, None, continuation.as_deref(), &cancel);
                }
                if let Ok(ref page) = outcome
                    && is_default_home
                    && !append
                    && !page.items.is_empty()
                {
                    #[cfg(not(test))]
                    {
                        let now_unix = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let cache = innertube::HomeSwrCache::from_page(page, language, now_unix);
                        let _ = cache.save();
                    }
                    #[cfg(test)]
                    let _ = language;
                }
                app_state::InnerTubeHomeFetchMessage {
                    request_id,
                    append,
                    is_default_home,
                    result: outcome.map_err(innertube::InnerTubeError::into_message),
                }
            },
        );
    }

    pub(crate) fn poll_innertube_home(&mut self, ctx: &egui::Context) {
        match self
            .feed
            .home_innertube_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                self.feed.home_loading_more = false;
                let viewing_home = (self.ui.browse_section == BrowseSection::Home
                    || self.ui.browse_section == BrowseSection::Shorts)
                    && !self.ui.search_results_mode
                    && !self.channels.view_open
                    && self.feed.home.title.is_none();
                match msg.result {
                    Ok(page) => {
                        if viewing_home {
                            let is_empty = page.items.is_empty();
                            self.apply_innertube_home_page(
                                page,
                                msg.append,
                                !msg.append && msg.is_default_home,
                                ctx,
                            );
                            if !msg.append && is_empty && self.feed.videos.is_empty() {
                                self.start_fast_home_feed(false, ctx);
                            }
                        }
                    }
                    Err(_) => {
                        if !msg.append && viewing_home && self.feed.videos.is_empty() {
                            self.start_fast_home_feed(false, ctx);
                        }
                    }
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.feed.home_loading_more = false;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_load_more_home(&mut self, ctx: &egui::Context) {
        if self.feed.home_loading_more || self.feed.home_innertube_task.is_running() {
            return;
        }
        if let Some(token) = self.feed.home_continuation.clone() {
            self.start_innertube_home_feed(Some(token), true, false, ctx);
        } else if self.feed.home.has_more {
            self.start_fast_home_feed(true, ctx);
        }
    }

    pub(crate) fn select_home_innertube_chip(
        &mut self,
        token: Option<String>,
        ctx: &egui::Context,
    ) {
        for chip in &mut self.feed.home_chips {
            chip.selected = chip.token == token;
        }
        self.feed.home_continuation = None;
        self.feed.home_loading_more = false;
        self.feed.home.title = None;
        let is_default = token.is_none();
        self.start_innertube_home_feed(token, false, is_default, ctx);
    }

    pub(crate) fn start_innertube_subscriptions_feed(
        &mut self,
        continuation: Option<String>,
        append: bool,
        ctx: &egui::Context,
    ) -> bool {
        let Some(mut web_session) = Self::load_innertube_web_session_cached() else {
            return false;
        };
        if append {
            self.feed.subscriptions_loading_more = true;
        }
        let it_ctx = self.innertube_context();
        self.feed.subs_innertube_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let outcome = innertube::fetch_subscriptions_feed(
                    &it_ctx,
                    &mut web_session,
                    continuation.as_deref(),
                    &cancel,
                );
                if let Err(ref err) = outcome
                    && err.is_auth_expired()
                {
                    #[cfg(not(test))]
                    let _ = innertube::invalidate_web_session();
                }
                app_state::InnerTubeSubscriptionsFetchMessage {
                    request_id,
                    append,
                    result: outcome.map_err(innertube::InnerTubeError::into_message),
                }
            },
        );
        true
    }

    pub(crate) fn apply_innertube_subscriptions_page(
        &mut self,
        page: innertube::InnerTubeSubscriptionsPage,
        append: bool,
        ctx: &egui::Context,
    ) {
        let results: Vec<SearchResult> = page
            .items
            .into_iter()
            .zip(page.metadata)
            .map(|(video, metadata)| SearchResult { video, metadata })
            .collect();
        self.feed.subscriptions_continuation = page.continuation;
        let viewing_subscriptions = self.ui.browse_section == BrowseSection::Subscriptions
            && !self.ui.search_results_mode
            && !self.channels.view_open;
        if append {
            let mut seen: HashSet<String> = self
                .feed
                .videos
                .iter()
                .map(|v| history::youtube_video_id(&v.url).unwrap_or_else(|| v.url.clone()))
                .collect();
            let mut added = 0usize;
            for item in results {
                let key = history::youtube_video_id(&item.video.url)
                    .unwrap_or_else(|| item.video.url.clone());
                if !seen.insert(key) {
                    continue;
                }
                self.video_metadata
                    .insert(item.video.url.clone(), item.metadata.clone());
                self.feed.home.subscriptions_cached_items.push(item.clone());
                if viewing_subscriptions {
                    self.feed.videos.push(item.video);
                }
                added += 1;
            }
            if added > 0 && viewing_subscriptions {
                self.feed.generation = self.feed.generation.wrapping_add(1);
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                self.feed.cache_dirty = true;
            }
        } else {
            self.feed.home.subscriptions_cached_items = results.clone();
            self.feed.home.subscriptions_cached_loaded = true;
            self.feed.home.subscriptions_cached_at = Some(Instant::now());
            if viewing_subscriptions {
                self.apply_search_results(ctx, results);
            }
        }
    }

    pub(crate) fn poll_innertube_subscriptions(&mut self, ctx: &egui::Context) {
        match self
            .feed
            .subs_innertube_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                self.feed.subscriptions_loading_more = false;
                let viewing_subscriptions = self.ui.browse_section == BrowseSection::Subscriptions
                    && !self.ui.search_results_mode
                    && !self.channels.view_open;
                match msg.result {
                    Ok(page) => {
                        self.apply_innertube_subscriptions_page(page, msg.append, ctx);
                    }
                    Err(_) => {
                        if !msg.append && viewing_subscriptions {
                            if self.auth.authenticated {
                                self.start_fast_subscriptions_feed(
                                    false,
                                    self.feed.videos.is_empty(),
                                    ctx,
                                );
                            } else if !self.followed_channels.data.is_empty() {
                                self.start_local_followed_feed(false, ctx);
                            }
                        }
                    }
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.feed.subscriptions_loading_more = false;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_load_more_subscriptions(&mut self, ctx: &egui::Context) {
        if self.feed.subscriptions_loading_more || self.feed.subs_innertube_task.is_running() {
            return;
        }
        let Some(token) = self.feed.subscriptions_continuation.clone() else {
            return;
        };
        if !self.start_innertube_subscriptions_feed(Some(token), true, ctx) {
            self.feed.subscriptions_continuation = None;
        }
    }

    pub(crate) fn start_load_more_search(&mut self, ctx: &egui::Context) {
        if self.feed.search_loading_more || self.feed.search_page_task.is_running() {
            return;
        }
        let Some(token) = self.feed.search_continuation.clone() else {
            return;
        };
        let query = if !self.feed.search_page_last_query.trim().is_empty() {
            self.feed.search_page_last_query.clone()
        } else {
            self.feed.query.trim().to_owned()
        };
        self.feed.search_loading_more = true;
        let filters = search::effective_search_filters(
            self.feed.search_filters.clone(),
            self.feed.search_chip,
        );
        let language = self.settings.prefs.value.language;
        let web_session = Self::load_innertube_web_session_cached();
        self.feed.search_page_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result = search::search_videos_innertube(
                    &query,
                    &filters,
                    Some(&token),
                    language,
                    None,
                    web_session.as_ref(),
                    &cancel,
                );
                app_state::SearchPageFetchMessage {
                    request_id,
                    append: true,
                    result,
                }
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn complete_search_page_request(
        &mut self,
        request_id: u64,
        append: bool,
        result: Result<search::SearchPageOutput, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.feed.search_page_task.active_request_id() != Some(request_id) {
            return false;
        }
        self.feed.search_page_task.cancel();
        self.apply_search_page_request(append, result, ctx);
        true
    }

    pub(crate) fn apply_search_page_request(
        &mut self,
        append: bool,
        result: Result<search::SearchPageOutput, String>,
        ctx: &egui::Context,
    ) {
        self.feed.search_loading_more = false;
        if !append {
            self.feed.search.task.cancel();
        }
        match result {
            Ok(page) => {
                self.feed.search_continuation = page.continuation.clone();
                if !append {
                    self.feed.search_spelling = page.spelling.clone();
                    let results = page.into_search_results();
                    self.apply_search_results(ctx, results);
                } else {
                    let results = page.into_search_results();
                    let mut seen: HashSet<String> = self
                        .feed
                        .videos
                        .iter()
                        .map(|v| history::youtube_video_id(&v.url).unwrap_or_else(|| v.url.clone()))
                        .collect();
                    let mut added = 0usize;
                    for item in results {
                        if self.feed.search_reported_urls.contains(&item.video.url) {
                            continue;
                        }
                        let key = history::youtube_video_id(&item.video.url)
                            .unwrap_or_else(|| item.video.url.clone());
                        if !seen.insert(key) {
                            continue;
                        }
                        self.video_metadata
                            .insert(item.video.url.clone(), item.metadata);
                        self.feed.videos.push(item.video);
                        added += 1;
                    }
                    if added > 0 {
                        self.feed.generation = self.feed.generation.wrapping_add(1);
                        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                        self.feed.cache_dirty = true;
                    }
                }
            }
            Err(error) => {
                self.message = error;
                if append {
                    self.feed.search_continuation = None;
                }
            }
        }
    }

    pub(crate) fn poll_search_page(&mut self, ctx: &egui::Context) {
        let had_active = self.feed.search_page_task.is_active();
        match self
            .feed
            .search_page_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                self.apply_search_page_request(msg.append, msg.result, ctx);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if had_active {
                    self.message = "A pesquisa foi interrompida.".into();
                }
                self.feed.search_loading_more = false;
                self.feed.search.task.cancel();
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }
}
