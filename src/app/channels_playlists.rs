use crate::*;

impl FastTube {
    pub(crate) fn is_channel_loading(&self) -> bool {
        self.channels.is_loading()
    }

    pub(crate) fn is_channel_more_videos_loading(&self) -> bool {
        self.channels.is_more_videos_loading()
    }

    pub(crate) fn is_channel_playlists_loading(&self) -> bool {
        self.channels.is_playlists_loading()
    }

    pub(crate) fn is_channel_playlist_videos_loading(&self) -> bool {
        self.channels.is_playlist_videos_loading()
    }

    pub(crate) fn is_channel_about_loading(&self) -> bool {
        self.channels.is_about_loading()
    }

    pub(crate) fn cancel_channel_subrequests(&mut self) {
        self.channels.cancel_subrequests();
    }

    pub(crate) fn reset_channel_tabs_state(&mut self, ctx: &egui::Context) {
        self.cancel_channel_subrequests();
        for item in &self.channels.shorts.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.streams.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.videos.sorted_items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.search.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for pl in &self.channels.playlists.items {
            let thumb = &pl.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        for item in &self.channels.playlist_detail.items {
            let thumb = &item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channels.tab = ChannelTab::Home;
        self.channels.about_modal_open = false;
        self.channels.videos.more_error = None;
        self.channels.videos.has_more = false;
        self.channels.videos.reached_max = false;
        self.channels.videos.sort = search::ChannelVideosSort::Latest;
        self.channels.videos.sorted_items.clear();
        self.channels.videos.sorted_error = None;
        self.channels.shorts.items.clear();
        self.channels.shorts.loaded = false;
        self.channels.shorts.error = None;
        self.channels.streams.items.clear();
        self.channels.streams.loaded = false;
        self.channels.streams.error = None;
        self.channels.playlists.items.clear();
        self.channels.playlists.loaded = false;
        self.channels.playlists.error = None;
        self.channels.playlist_detail.selected = None;
        self.channels.playlist_detail.items.clear();
        self.channels.playlist_detail.loaded = false;
        self.channels.playlist_detail.error = None;
        self.channels.about.info = None;
        self.channels.about.loaded = false;
        self.channels.about.error = None;
        self.channels.search = app_state::ChannelSearchState::default();
    }

    pub(crate) fn cancel_channel_loading(&mut self) {
        self.channels.cancel_loading();
    }

    pub(crate) fn is_own_channel_target(&self, target: &ChannelTarget) -> bool {
        if self.auth.my_channel.as_ref() == Some(target) {
            return true;
        }
        if matches!(target, ChannelTarget::Handle(h) if h.eq_ignore_ascii_case("@voce")) {
            return true;
        }
        if let (Some(my_target), Some(page)) = (&self.auth.my_channel, &self.channels.page)
            && page.target == *target
        {
            return match my_target {
                ChannelTarget::ChannelId(my_id) => {
                    page.channel_id.as_deref() == Some(my_id.as_str())
                }
                ChannelTarget::Handle(my_handle) => page
                    .handle
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(my_handle)),
            };
        }
        false
    }

    pub(crate) fn is_own_channel_page(&self, page: &ChannelPage) -> bool {
        if self.is_own_channel_target(&page.target) {
            return true;
        }
        match &self.auth.my_channel {
            Some(ChannelTarget::ChannelId(my_id)) => {
                page.channel_id.as_deref() == Some(my_id.as_str())
            }
            Some(ChannelTarget::Handle(my_handle)) => page
                .handle
                .as_deref()
                .is_some_and(|h| h.eq_ignore_ascii_case(my_handle)),
            None => false,
        }
    }

    pub(crate) fn resolve_innertube_web_session(&self) -> Option<innertube::YouTubeWebSession> {
        #[cfg(test)]
        {
            let _ = self;
            None
        }
        #[cfg(not(test))]
        {
            if let Some(sess) = Self::load_innertube_web_session_cached() {
                return Some(sess);
            }
            let web = self.auth.youtube_web_session.as_ref()?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            playlist_sync::to_innertube_web_session(web, now).ok()
        }
    }

    pub(crate) fn default_available_channel_tab_for_page(&self, page: &ChannelPage) -> ChannelTab {
        let is_own_channel = self.is_own_channel_page(page);
        let has_videos = channel_ui::channel_page_has_videos(page)
            || !self.channels.videos.sorted_items.is_empty();
        let has_home = channel_ui::channel_page_has_home(page, has_videos);
        let has_playlists = page.has_playlists_tab
            || page
                .tab_endpoints
                .iter()
                .any(|ep| ep.kind == innertube::ChannelTabKind::Playlists)
            || !page.initial_playlists.is_empty()
            || !self.channels.playlists.items.is_empty()
            || (is_own_channel
                && (self.auth.authenticated || self.auth.youtube_web_session.is_some()));
        let has_shorts = page.has_shorts_tab
            || page
                .tab_endpoints
                .iter()
                .any(|ep| ep.kind == innertube::ChannelTabKind::Shorts);
        let has_streams = page.has_streams_tab
            || page
                .tab_endpoints
                .iter()
                .any(|ep| ep.kind == innertube::ChannelTabKind::Live);

        if has_home {
            ChannelTab::Home
        } else if has_videos {
            ChannelTab::Videos
        } else if has_playlists {
            ChannelTab::Playlists
        } else if has_shorts {
            ChannelTab::Shorts
        } else if has_streams {
            ChannelTab::Streams
        } else {
            ChannelTab::Home
        }
    }

    pub(crate) fn current_channel_target(&self) -> Option<ChannelTarget> {
        self.channels.current_target()
    }

    pub(crate) fn apply_channel_page(&mut self, ctx: &egui::Context, page: ChannelPage) {
        if let Some(title) = page.title.as_deref() {
            self.cache_channel_avatar(title, page.avatar_url.as_deref());
        }
        if let Some(previous_page) = &self.channels.page {
            let page_keeps_thumb = |thumb: &str| {
                page.videos.iter().any(|v| v.video.thumbnail == thumb)
                    || page
                        .featured_video
                        .as_ref()
                        .is_some_and(|v| v.video.thumbnail == thumb)
                    || page
                        .home_shelves
                        .iter()
                        .any(|s| s.items.iter().any(|v| v.video.thumbnail == thumb))
            };
            for old_item in &previous_page.videos {
                let thumb = &old_item.video.thumbnail;
                if !thumb.is_empty()
                    && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    && !page_keeps_thumb(thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
            if let Some(old_featured) = &previous_page.featured_video {
                let thumb = &old_featured.video.thumbnail;
                if !thumb.is_empty()
                    && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    && !page_keeps_thumb(thumb)
                {
                    ctx.forget_image(thumb);
                }
            }
            for shelf in &previous_page.home_shelves {
                for old_item in &shelf.items {
                    let thumb = &old_item.video.thumbnail;
                    if !thumb.is_empty()
                        && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                        && !page_keeps_thumb(thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
            }
        }
        for item in &page.videos {
            if let Some(ts) = item.metadata.upcoming_start_unix {
                self.channels
                    .upcoming_start_unix_by_url
                    .insert(item.video.url.clone(), ts);
            }
            self.video_metadata
                .insert(item.video.url.clone(), item.metadata.clone());
        }
        if let Some(featured) = &page.featured_video {
            if let Some(ts) = featured.metadata.upcoming_start_unix {
                self.channels
                    .upcoming_start_unix_by_url
                    .insert(featured.video.url.clone(), ts);
            }
            self.video_metadata
                .insert(featured.video.url.clone(), featured.metadata.clone());
        }
        for shelf in &page.home_shelves {
            for item in &shelf.items {
                if let Some(ts) = item.metadata.upcoming_start_unix {
                    self.channels
                        .upcoming_start_unix_by_url
                        .insert(item.video.url.clone(), ts);
                }
                self.video_metadata
                    .insert(item.video.url.clone(), item.metadata.clone());
            }
        }
        self.channels.community_posts = page.community_posts.clone();
        if let Some(selected) = &self.selected
            && let Some(found) = self.video_metadata.get(&selected.url)
        {
            self.selected_metadata = Some(found.clone());
        }
        self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        let effective_limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        let loaded_count = page.videos.len();
        self.channels.videos.reached_max = loaded_count >= search::MAX_CHANNEL_VIDEOS_LIMIT;
        self.channels.videos.has_more =
            !self.channels.videos.reached_max && loaded_count >= effective_limit;
        self.channels.videos.more_error = None;
        self.channels.videos.sort = search::ChannelVideosSort::Latest;
        self.channels.videos.sorted_items.clear();
        self.channels.videos.sorted_error = None;
        if page.country.is_some()
            || page.joined_date_text.is_some()
            || !page.links.is_empty()
            || page.view_count.is_some()
            || page.view_count_text.is_some()
        {
            let info = search::ChannelAboutInfo {
                channel_id: page.channel_id.clone(),
                title: page.title.clone(),
                handle: page.handle.clone(),
                canonical_url: Some(page.canonical_url.clone()),
                description: page.description.clone(),
                subscriber_count: page.subscriber_count,
                view_count: page.view_count,
                subscriber_count_text: page.subscriber_count_text.clone(),
                video_count_text: page.video_count_text.clone(),
                view_count_text: page.view_count_text.clone(),
                country: page.country.clone(),
                joined_date_text: page.joined_date_text.clone(),
                links: page.links.clone(),
                avatar_url: page.avatar_url.clone(),
                banner_url: page.banner_url.clone(),
                verified: page.verified,
                tags: page.tags.clone(),
            };
            self.channels.about.info = Some(info);
            if page.country.is_some() || page.joined_date_text.is_some() {
                self.channels.about.loaded = true;
            }
        }
        let is_own_channel = self.is_own_channel_page(&page);
        if !page.initial_playlists.is_empty() {
            if is_own_channel {
                if self.channels.playlists.items.is_empty() {
                    self.channels.playlists.items = page.initial_playlists.clone();
                } else {
                    let existing = std::mem::take(&mut self.channels.playlists.items);
                    self.channels.playlists.items = search::merge_channel_playlists(
                        page.initial_playlists.clone(),
                        existing,
                        search::MAX_CHANNEL_PLAYLISTS_LIMIT,
                    );
                }
            } else {
                self.channels.playlists.items = page.initial_playlists.clone();
                self.channels.playlists.loaded = true;
                self.channels.playlists.error = None;
            }
        }
        let has_videos = channel_ui::channel_page_has_videos(&page);
        let has_home = channel_ui::channel_page_has_home(&page, has_videos);
        if (self.channels.tab == ChannelTab::Home && !has_home)
            || (self.channels.tab == ChannelTab::Videos && !has_videos)
        {
            self.channels.tab = self.default_available_channel_tab_for_page(&page);
        }

        let target = page.target.clone();
        let channel_id = page.channel_id.clone();
        self.channels.last_target = Some(target.clone());
        self.channels.error = None;
        self.channels.page = Some(page);
        if !cfg!(test) {
            match self.channels.tab {
                ChannelTab::Playlists
                    if !self.channels.playlists.loaded
                        && !self.is_channel_playlists_loading()
                        && self.channels.playlists.error.is_none() =>
                {
                    self.start_channel_playlists(ctx);
                }
                ChannelTab::Shorts
                    if !self.channels.shorts.loaded
                        && !self.channels.is_shorts_loading()
                        && self.channels.shorts.error.is_none() =>
                {
                    self.start_channel_shorts(ctx);
                }
                ChannelTab::Streams
                    if !self.channels.streams.loaded
                        && !self.channels.is_streams_loading()
                        && self.channels.streams.error.is_none() =>
                {
                    self.start_channel_streams(ctx);
                }
                _ => {}
            }
        }
        if self.auth.authenticated {
            self.start_check_subscription(target, channel_id, ctx);
        }
    }

    pub(crate) fn begin_channel_request(
        &mut self,
        target: ChannelTarget,
        ctx: &egui::Context,
    ) -> Option<(u64, ChannelTarget)> {
        let Some(valid_target) = search::parse_channel_target(&target.canonical_url()) else {
            self.channels.error = Some(
                "Não foi possível abrir o canal: identificador ou URL do canal inválido ou não resolvido com segurança."
                    .into(),
            );
            return None;
        };
        let is_different_channel = self.channels.page.as_ref().is_some_and(|prev| {
            prev.target != valid_target && prev.canonical_url != valid_target.canonical_url()
        }) || (self.channels.page.is_none()
            && self.channels.last_target.as_ref() != Some(&valid_target));
        self.cancel_channel_subrequests();
        let request_id = self.channels.videos.task.begin_reserved();
        self.channels.loading_target = Some(valid_target.clone());
        self.channels.last_target = Some(valid_target.clone());
        self.channels.error = None;
        self.channels.videos.more_error = None;
        self.channels.view_open = true;
        self.ui.settings_open = false;
        self.sync_player_window_visibility();
        if is_different_channel {
            if let Some(previous_page) = &self.channels.page {
                for item in &previous_page.videos {
                    let thumb = &item.video.thumbnail;
                    if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
                if let Some(featured) = &previous_page.featured_video {
                    let thumb = &featured.video.thumbnail;
                    if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
                for shelf in &previous_page.home_shelves {
                    for item in &shelf.items {
                        let thumb = &item.video.thumbnail;
                        if !thumb.is_empty()
                            && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                        {
                            ctx.forget_image(thumb);
                        }
                    }
                }
                self.channels.page = None;
            }
            self.reset_channel_tabs_state(ctx);
        }
        Some((request_id, valid_target))
    }

    pub(crate) fn open_channel(&mut self, target: ChannelTarget, ctx: &egui::Context) -> bool {
        self.open_channel_with_options(target, ctx, false)
    }

    pub(crate) fn open_channel_with_options(
        &mut self,
        target: ChannelTarget,
        ctx: &egui::Context,
        force_refresh: bool,
    ) -> bool {
        self.feed.home.stream_task.cancel();
        let Some((request_id, valid_target)) = self.begin_channel_request(target, ctx) else {
            return false;
        };
        if force_refresh {
            self.channels.playlists.loaded = false;
            self.channels.playlists.error = None;
            self.channels.about.loaded = false;
            self.channels.about.error = None;
        }
        let input = valid_target.canonical_url();
        let language = self.settings.prefs.value.language;
        let web_session = self.resolve_innertube_web_session();
        self.channels
            .videos
            .task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let result = search::fetch_channel_page_innertube(
                    &input,
                    None,
                    force_refresh,
                    language,
                    None,
                    web_session.as_ref(),
                    &cancel,
                );
                ChannelFetchMessage {
                    request_id,
                    target: valid_target,
                    result,
                }
            });
        true
    }

    pub(crate) fn begin_channel_more_videos_request(
        &mut self,
    ) -> Option<(u64, ChannelTarget, usize, usize)> {
        self.channels.begin_more_videos_request()
    }

    pub(crate) fn load_more_channel_videos(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target, start, count)) = self.begin_channel_more_videos_request()
        else {
            return false;
        };
        let input = target.canonical_url();
        self.channels
            .videos
            .more_task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let result =
                    search::fetch_channel_videos_batch_with_cancel(&input, start, count, &cancel);
                ChannelMoreVideosFetchMessage {
                    request_id,
                    target,
                    result,
                }
            });
        true
    }

    pub(crate) fn cancel_channel_more_videos(&mut self) {
        self.channels.cancel_more_videos();
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_more_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelVideosBatch, String>,
    ) -> bool {
        if self.channels.videos.more_task.active_request_id() != Some(request_id) {
            return false;
        }
        self.channels.videos.more_task.cancel();
        self.apply_channel_more_videos(target, result)
    }

    pub(crate) fn apply_channel_more_videos(
        &mut self,
        target: &ChannelTarget,
        result: Result<search::ChannelVideosBatch, String>,
    ) -> bool {
        if self
            .channels
            .page
            .as_ref()
            .is_none_or(|page| &page.target != target)
        {
            return false;
        }
        match result {
            Ok(batch) => {
                self.channels.videos.more_error = None;
                self.channels.videos.has_more = batch.has_more;
                self.channels.videos.reached_max = batch.reached_max_limit;
                if let Some(page) = &mut self.channels.page {
                    for item in batch.videos {
                        if page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT {
                            self.channels.videos.reached_max = true;
                            self.channels.videos.has_more = false;
                            break;
                        }
                        if !page
                            .videos
                            .iter()
                            .any(|existing| existing.video.url == item.video.url)
                        {
                            self.video_metadata
                                .insert(item.video.url.clone(), item.metadata.clone());
                            page.videos.push(item);
                        }
                    }
                    if page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT {
                        self.channels.videos.reached_max = true;
                        self.channels.videos.has_more = false;
                    }
                    let effective_limit = self
                        .channels
                        .video_limit
                        .max(page.videos.len())
                        .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
                    search::update_cached_channel_videos(
                        &page.target,
                        effective_limit,
                        &page.videos,
                    );
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            }
            Err(error) => {
                self.channels.videos.more_error = Some(error);
            }
        }
        true
    }

    pub(crate) fn begin_channel_playlists_request(&mut self) -> Option<(u64, ChannelTarget)> {
        self.channels.begin_playlists_request()
    }

    pub(crate) fn start_channel_playlists(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.begin_channel_playlists_request() else {
            return false;
        };
        let is_own = self.is_own_channel_target(&target)
            || self
                .channels
                .page
                .as_ref()
                .is_some_and(|p| self.is_own_channel_page(p));
        let (playlists_browse_id, playlists_params) = self
            .channels
            .page
            .as_ref()
            .map(|page| {
                let ep = page
                    .tab_endpoints
                    .iter()
                    .find(|t| t.kind == innertube::ChannelTabKind::Playlists);
                let bid = ep
                    .map(|t| t.browse_id.clone())
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| page.channel_id.clone());
                let params = ep.and_then(|t| t.params.clone());
                (bid, params)
            })
            .unwrap_or((None, None));
        let input = target.canonical_url();
        let language = self.settings.prefs.value.language;
        let web_session = self.resolve_innertube_web_session();
        let oauth_session = if is_own && self.auth.authenticated {
            self.auth.session.clone()
        } else {
            None
        };
        self.channels
            .playlists
            .task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let web_res = search::fetch_channel_playlists_innertube(
                    &input,
                    playlists_browse_id.as_deref(),
                    playlists_params.as_deref(),
                    search::DEFAULT_CHANNEL_PLAYLISTS_LIMIT,
                    language,
                    web_session.as_ref(),
                    &cancel,
                );
                let mut returned_oauth = None;
                let result = if let Some(mut sess) = oauth_session {
                    let oauth_res = sess.refresh_if_needed(&cancel).and_then(|()| {
                        oauth::list_my_playlists_detailed(&sess.access_token, &cancel)
                    });
                    returned_oauth = Some(sess);
                    match (web_res, oauth_res) {
                        (Ok(web_pls), Ok(cards)) => {
                            let oauth_pls = oauth_cards_to_channel_playlists(cards);
                            Ok(search::merge_channel_playlists(
                                web_pls,
                                oauth_pls,
                                search::MAX_CHANNEL_PLAYLISTS_LIMIT,
                            ))
                        }
                        (Err(_), Ok(cards)) => Ok(oauth_cards_to_channel_playlists(cards)),
                        (Ok(web_pls), Err(_)) => Ok(web_pls),
                        (Err(web_err), Err(_)) => Err(web_err),
                    }
                } else {
                    web_res
                };
                ChannelPlaylistsFetchMessage {
                    request_id,
                    target,
                    oauth_session: returned_oauth,
                    result,
                }
            });
        true
    }

    pub(crate) fn cancel_channel_playlists(&mut self) {
        self.channels.cancel_playlists();
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_playlists(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<Vec<search::ChannelPlaylist>, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.playlists.task.active_request_id() != Some(request_id) {
            return false;
        }
        self.channels.playlists.task.cancel();
        self.apply_channel_playlists(target, result, ctx)
    }

    pub(crate) fn apply_channel_playlists(
        &mut self,
        target: &ChannelTarget,
        result: Result<Vec<search::ChannelPlaylist>, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.current_channel_target().as_ref() != Some(target) {
            return false;
        }
        match result {
            Ok(playlists) => {
                for old_pl in &self.channels.playlists.items {
                    let thumb = &old_pl.thumbnail;
                    if !thumb.is_empty()
                        && !playlists.iter().any(|p| p.thumbnail == *thumb)
                        && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb)
                    {
                        ctx.forget_image(thumb);
                    }
                }
                self.channels.playlists.items = playlists;
                self.channels.playlists.loaded = true;
                self.channels.playlists.error = None;
            }
            Err(error) => {
                self.channels.playlists.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn begin_channel_playlist_videos_request(
        &mut self,
        playlist: search::ChannelPlaylist,
        ctx: &egui::Context,
    ) -> Option<(u64, ChannelTarget, String)> {
        if !search::is_valid_playlist_id(&playlist.id) {
            self.channels.playlist_detail.error =
                Some("Não foi possível abrir a playlist: identificador inválido.".into());
            return None;
        }
        let target = self
            .channels
            .page
            .as_ref()
            .map(|page| page.target.clone())
            .or_else(|| self.current_channel_target())?;
        self.channels.playlist_detail.task.cancel();
        for old_item in &self.channels.playlist_detail.items {
            let thumb = &old_item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channels.playlist_detail.items.clear();
        self.channels.playlist_detail.loaded = false;
        self.channels.playlist_detail.error = None;
        let playlist_id = playlist.id.clone();
        self.channels.playlist_detail.selected = Some(playlist);
        let request_id = self.channels.playlist_detail.task.begin_reserved();
        Some((request_id, target, playlist_id))
    }

    pub(crate) fn open_channel_public_playlist(
        &mut self,
        playlist: search::ChannelPlaylist,
        ctx: &egui::Context,
    ) -> bool {
        let Some((request_id, target, playlist_id)) =
            self.begin_channel_playlist_videos_request(playlist, ctx)
        else {
            return false;
        };
        let pid_clone = playlist_id.clone();
        let language = self.settings.prefs.value.language;
        let web_session = self.resolve_innertube_web_session();
        let oauth_session = if self.auth.authenticated {
            self.auth.session.clone()
        } else {
            None
        };
        self.channels
            .playlist_detail
            .task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let mut returned_oauth = None;
                let session_res = search::fetch_playlist_videos_with_session(
                    &pid_clone,
                    search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT,
                    language,
                    web_session.as_ref(),
                    &cancel,
                );
                let result = match session_res {
                    Ok(videos) if !videos.is_empty() => Ok(videos),
                    other_res => {
                        if let Some(mut sess) = oauth_session {
                            let oauth_res = sess.refresh_if_needed(&cancel).and_then(|()| {
                                oauth::playlist_videos_feed_progressive(
                                    &sess.access_token,
                                    &pid_clone,
                                    &cancel,
                                    |_| {},
                                )
                            });
                            returned_oauth = Some(sess);
                            match oauth_res {
                                Ok(mut feed) => {
                                    feed.items
                                        .truncate(search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT);
                                    Ok(feed.items)
                                }
                                Err(oauth_err) => match other_res {
                                    Ok(videos) => Ok(videos),
                                    Err(_) => search::fetch_public_playlist_videos_with_cancel(
                                        &pid_clone,
                                        search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT,
                                        &cancel,
                                    )
                                    .or(Err(oauth_err)),
                                },
                            }
                        } else if let Ok(videos) = other_res
                            && web_session.is_some()
                        {
                            Ok(videos)
                        } else {
                            search::fetch_public_playlist_videos_with_cancel(
                                &pid_clone,
                                search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT,
                                &cancel,
                            )
                        }
                    }
                };
                ChannelPlaylistVideosFetchMessage {
                    request_id,
                    target,
                    playlist_id: pid_clone,
                    oauth_session: returned_oauth,
                    result,
                }
            });
        true
    }

    pub(crate) fn close_channel_public_playlist(&mut self, ctx: &egui::Context) {
        self.channels.playlist_detail.task.cancel();
        self.channels.playlist_detail.error = None;
        self.channels.playlist_detail.loaded = false;
        self.channels.playlist_detail.selected = None;
        for old_item in &self.channels.playlist_detail.items {
            let thumb = &old_item.video.thumbnail;
            if !thumb.is_empty() && !self.feed.videos.iter().any(|v| &v.thumbnail == thumb) {
                ctx.forget_image(thumb);
            }
        }
        self.channels.playlist_detail.items.clear();
    }

    pub(crate) fn cancel_channel_playlist_videos(&mut self) {
        self.channels.cancel_playlist_videos();
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_playlist_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        playlist_id: &str,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.channels.playlist_detail.task.active_request_id() != Some(request_id) {
            return false;
        }
        self.channels.playlist_detail.task.cancel();
        self.apply_channel_playlist_videos(target, playlist_id, result)
    }

    pub(crate) fn apply_channel_playlist_videos(
        &mut self,
        target: &ChannelTarget,
        playlist_id: &str,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.current_channel_target().as_ref() != Some(target)
            || self
                .channels
                .playlist_detail
                .selected
                .as_ref()
                .is_none_or(|pl| pl.id != playlist_id)
        {
            return false;
        }
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                self.channels.playlist_detail.items = videos;
                self.channels.playlist_detail.loaded = true;
                self.channels.playlist_detail.error = None;
            }
            Err(error) => {
                self.channels.playlist_detail.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn begin_channel_about_request(&mut self) -> Option<(u64, ChannelTarget)> {
        self.channels.begin_about_request()
    }

    pub(crate) fn start_channel_about(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.begin_channel_about_request() else {
            return false;
        };
        let input = target.canonical_url();
        self.channels
            .about
            .task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let result = search::fetch_channel_about_with_cancel(&input, &cancel);
                ChannelAboutFetchMessage {
                    request_id,
                    target,
                    result,
                }
            });
        true
    }

    pub(crate) fn cancel_channel_about(&mut self) {
        self.channels.cancel_about();
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_about(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelAboutInfo, String>,
    ) -> bool {
        if self.channels.about.task.active_request_id() != Some(request_id) {
            return false;
        }
        self.channels.about.task.cancel();
        self.apply_channel_about(target, result)
    }

    pub(crate) fn apply_channel_about(
        &mut self,
        target: &ChannelTarget,
        result: Result<search::ChannelAboutInfo, String>,
    ) -> bool {
        if self.current_channel_target().as_ref() != Some(target) {
            return false;
        }
        match result {
            Ok(about) => {
                if let Some(page) = &mut self.channels.page {
                    if page.channel_id.is_none() {
                        page.channel_id = about.channel_id.clone();
                    }
                    if page.title.is_none() {
                        page.title = about.title.clone();
                    }
                    if page.handle.is_none() {
                        page.handle = about.handle.clone();
                    }
                    if let Some(desc) = &about.description {
                        let current_len = page.description.as_ref().map_or(0, String::len);
                        if desc.len() >= current_len {
                            page.description = Some(desc.clone());
                        }
                    }
                    if about.subscriber_count.is_some() {
                        page.subscriber_count = about.subscriber_count;
                    }
                    if about.view_count.is_some() {
                        page.view_count = about.view_count;
                    }
                    if page.subscriber_count_text.is_none() {
                        page.subscriber_count_text = about.subscriber_count_text.clone();
                    }
                    if page.video_count_text.is_none() {
                        page.video_count_text = about.video_count_text.clone();
                    }
                    if page.view_count_text.is_none() {
                        page.view_count_text = about.view_count_text.clone();
                    }
                    if page.country.is_none() {
                        page.country = about.country.clone();
                    }
                    if page.joined_date_text.is_none() {
                        page.joined_date_text = about.joined_date_text.clone();
                    }
                    if page.links.is_empty() && !about.links.is_empty() {
                        page.links = about.links.clone();
                    }
                    if page.avatar_url.is_none() {
                        page.avatar_url = about.avatar_url.clone();
                    }
                    if page.banner_url.is_none() {
                        page.banner_url = about.banner_url.clone();
                    }
                    if page.verified.is_none() {
                        page.verified = about.verified;
                    }
                    if !about.tags.is_empty() {
                        page.tags = about.tags.clone();
                    }
                }
                self.channels.about.info = Some(about);
                self.channels.about.loaded = true;
                self.channels.about.error = None;
            }
            Err(error) => {
                self.channels.about.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn start_channel_shorts(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.channels.begin_shorts_request() else {
            return false;
        };
        let input = target.canonical_url();
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        self.channels
            .shorts
            .task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let result = search::fetch_channel_tab_videos_with_cancel(
                    &input,
                    search::ChannelFeedTab::Shorts,
                    limit,
                    &cancel,
                );
                ChannelTabVideosFetchMessage {
                    request_id,
                    target,
                    tab: search::ChannelFeedTab::Shorts,
                    result,
                }
            });
        true
    }

    pub(crate) fn cancel_channel_shorts(&mut self) {
        self.channels.cancel_shorts();
    }

    pub(crate) fn start_channel_streams(&mut self, ctx: &egui::Context) -> bool {
        let Some((request_id, target)) = self.channels.begin_streams_request() else {
            return false;
        };
        let input = target.canonical_url();
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        self.channels
            .streams
            .task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let result = search::fetch_channel_tab_videos_with_cancel(
                    &input,
                    search::ChannelFeedTab::Streams,
                    limit,
                    &cancel,
                );
                ChannelTabVideosFetchMessage {
                    request_id,
                    target,
                    tab: search::ChannelFeedTab::Streams,
                    result,
                }
            });
        true
    }

    pub(crate) fn cancel_channel_streams(&mut self) {
        self.channels.cancel_streams();
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_tab_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        tab: search::ChannelFeedTab,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        let state = match tab {
            search::ChannelFeedTab::Shorts => &mut self.channels.shorts,
            search::ChannelFeedTab::Streams => &mut self.channels.streams,
            search::ChannelFeedTab::Videos => return false,
        };
        if state.task.active_request_id() != Some(request_id) {
            return false;
        }
        state.task.cancel();
        self.apply_channel_tab_videos(target, tab, result)
    }

    pub(crate) fn apply_channel_tab_videos(
        &mut self,
        target: &ChannelTarget,
        tab: search::ChannelFeedTab,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        let current_target = self.channels.current_target();
        if current_target.as_ref() != Some(target) {
            return false;
        }
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                let state = match tab {
                    search::ChannelFeedTab::Shorts => &mut self.channels.shorts,
                    search::ChannelFeedTab::Streams => &mut self.channels.streams,
                    search::ChannelFeedTab::Videos => return false,
                };
                state.items = videos;
                state.loaded = true;
                state.error = None;
            }
            Err(error) => {
                let state = match tab {
                    search::ChannelFeedTab::Shorts => &mut self.channels.shorts,
                    search::ChannelFeedTab::Streams => &mut self.channels.streams,
                    search::ChannelFeedTab::Videos => return false,
                };
                state.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_channel_tab_feeds(&mut self) {
        let shorts_expected = self.channels.shorts.task.active_request_id();
        match self.channels.shorts.task.poll_event() {
            async_task::AsyncTaskPoll::Ready(msg) => {
                if shorts_expected == Some(msg.request_id) {
                    self.apply_channel_tab_videos(&msg.target, msg.tab, msg.result);
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.channels.shorts.error =
                    Some("A consulta de Shorts do canal foi encerrada inesperadamente.".into());
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }

        let streams_expected = self.channels.streams.task.active_request_id();
        match self.channels.streams.task.poll_event() {
            async_task::AsyncTaskPoll::Ready(msg) => {
                if streams_expected == Some(msg.request_id) {
                    self.apply_channel_tab_videos(&msg.target, msg.tab, msg.result);
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.channels.streams.error = Some(
                    "A consulta de transmissões ao vivo do canal foi encerrada inesperadamente."
                        .into(),
                );
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_channel_sorted_videos(
        &mut self,
        sort: search::ChannelVideosSort,
        ctx: &egui::Context,
    ) -> bool {
        if sort == search::ChannelVideosSort::Latest {
            self.channels.cancel_sorted_videos();
            self.channels.videos.sort = sort;
            self.channels.videos.sorted_error = None;
            return false;
        }
        let Some(page) = self.channels.page.as_ref() else {
            return false;
        };
        let continuation = match sort {
            search::ChannelVideosSort::Latest => None,
            search::ChannelVideosSort::Popular => page.videos_popular_continuation.clone(),
            search::ChannelVideosSort::Oldest => page.videos_oldest_continuation.clone(),
        };
        let mut fallback = page.videos.clone();
        match sort {
            search::ChannelVideosSort::Latest => {}
            search::ChannelVideosSort::Popular => {
                fallback.sort_by(|a, b| {
                    b.metadata
                        .view_count
                        .unwrap_or(0)
                        .cmp(&a.metadata.view_count.unwrap_or(0))
                });
            }
            search::ChannelVideosSort::Oldest => {
                fallback.reverse();
            }
        }
        let channel_title = page.title.clone();
        let channel_id = page.channel_id.clone();
        let canonical_url = page.canonical_url.clone();
        let verified = page.verified == Some(true);

        self.channels.videos.sort = sort;
        self.channels.videos.sorted_error = None;
        let Some(token) = continuation.filter(|t| !t.trim().is_empty()) else {
            self.channels.cancel_sorted_videos();
            self.channels.videos.sorted_items = fallback;
            return false;
        };
        let Some((request_id, target)) = self.channels.begin_sorted_videos_request() else {
            return false;
        };
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        self.channels
            .videos
            .sorted_task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let result = search::fetch_channel_sorted_videos_with_cancel(
                    &token,
                    channel_title.as_deref(),
                    channel_id.as_deref(),
                    &canonical_url,
                    verified,
                    limit,
                    &cancel,
                );
                ChannelSortedVideosFetchMessage {
                    request_id,
                    target,
                    sort,
                    result,
                }
            });
        true
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_sorted_videos(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        sort: search::ChannelVideosSort,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.channels.videos.sorted_task.active_request_id() != Some(request_id) {
            return false;
        }
        self.channels.videos.sorted_task.cancel();
        self.apply_channel_sorted_videos(target, sort, result)
    }

    pub(crate) fn apply_channel_sorted_videos(
        &mut self,
        target: &ChannelTarget,
        sort: search::ChannelVideosSort,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.current_channel_target().as_ref() != Some(target) {
            return false;
        }
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                if self.channels.videos.sort == sort {
                    self.channels.videos.sorted_items = videos;
                }
                self.channels.videos.sorted_error = None;
            }
            Err(error) => {
                self.channels.videos.sorted_error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_channel_sorted_videos(&mut self) {
        let expected_id = self.channels.videos.sorted_task.active_request_id();
        match self.channels.videos.sorted_task.poll_event() {
            async_task::AsyncTaskPoll::Ready(msg) => {
                if expected_id == Some(msg.request_id) {
                    self.apply_channel_sorted_videos(&msg.target, msg.sort, msg.result);
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.channels.videos.sorted_error =
                    Some("A ordenação de vídeos do canal foi encerrada inesperadamente.".into());
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_channel_search(&mut self, query: &str, ctx: &egui::Context) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            self.channels.cancel_channel_search();
            self.channels.search.items.clear();
            self.channels.search.loaded = false;
            self.channels.search.error = None;
            return false;
        }
        let query_owned = trimmed.to_owned();
        self.channels.search.query = query_owned.clone();
        self.channels.search.input_open = true;
        self.channels.tab = ChannelTab::Search;
        if let Some(page) = &mut self.channels.page {
            for ep in &mut page.tab_endpoints {
                ep.selected = false;
            }
        }
        let Some((request_id, target)) = self.channels.begin_channel_search_request() else {
            return false;
        };
        let input = target.canonical_url();
        let limit = self
            .channels
            .video_limit
            .clamp(1, search::MAX_CHANNEL_VIDEOS_LIMIT);
        self.channels
            .search
            .task
            .spawn_reserved(request_id, ctx.clone(), move |cancel| {
                let result =
                    search::fetch_channel_search_with_cancel(&input, &query_owned, limit, &cancel);
                ChannelSearchFetchMessage {
                    request_id,
                    target,
                    query: query_owned,
                    result,
                }
            });
        true
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_search(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        query: String,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.channels.search.task.active_request_id() != Some(request_id) {
            return false;
        }
        self.channels.search.task.cancel();
        self.apply_channel_search(target, query, result)
    }

    pub(crate) fn apply_channel_search(
        &mut self,
        target: &ChannelTarget,
        query: String,
        result: Result<Vec<SearchResult>, String>,
    ) -> bool {
        if self.current_channel_target().as_ref() != Some(target) {
            return false;
        }
        match result {
            Ok(videos) => {
                for item in &videos {
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if let Some(selected) = &self.selected
                    && let Some(found) = self.video_metadata.get(&selected.url)
                {
                    self.selected_metadata = Some(found.clone());
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
                self.channels.search.query = query;
                self.channels.search.items = videos;
                self.channels.search.loaded = true;
                self.channels.search.error = None;
            }
            Err(error) => {
                self.channels.search.error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_channel_search(&mut self) {
        let expected_id = self.channels.search.task.active_request_id();
        match self.channels.search.task.poll_event() {
            async_task::AsyncTaskPoll::Ready(msg) => {
                if expected_id == Some(msg.request_id) {
                    self.apply_channel_search(&msg.target, msg.query, msg.result);
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.channels.search.error =
                    Some("A busca interna no canal foi encerrada inesperadamente.".into());
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn start_fetch_remote_playlists(&mut self, ctx: &egui::Context) {
        if self.auth.remote_playlists_task.is_running() {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            return;
        };
        self.auth
            .remote_playlists_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = session.refresh_if_needed(&cancel).and_then(|()| {
                    oauth::list_my_playlists_detailed(&session.access_token, &cancel)
                });
                (Some(session), result)
            });
    }

    pub(crate) fn poll_remote_playlists(&mut self, _ctx: &egui::Context) {
        if let Some((session, result)) = self.auth.remote_playlists_task.poll() {
            let _ = self.set_oauth_session(session);
            match result {
                Ok(cards) => {
                    self.auth.remote_playlists = cards
                        .iter()
                        .map(|c| (c.id.clone(), c.title.clone()))
                        .collect();
                    self.library.remote_playlists.oauth_items = cards;
                    self.rebuild_remote_playlists_view(None);
                }
                Err(error) => {
                    self.message = error;
                }
            }
        }
    }

    pub(crate) fn start_create_remote_playlist(
        &mut self,
        title: String,
        video_id: Option<String>,
        ctx: &egui::Context,
    ) {
        if self.auth.playlist_action_task.is_running() {
            return;
        }
        let Some(mut session) = self.auth.session.clone() else {
            self.start_reauth(ctx);
            return;
        };
        self.auth
            .playlist_action_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| {
                        oauth::create_playlist(&session.access_token, &title, "private", &cancel)
                    })
                    .and_then(|(pl_id, pl_title)| {
                        if let Some(vid) = video_id.as_deref() {
                            oauth::add_video_to_playlist(
                                &session.access_token,
                                &pl_id,
                                vid,
                                &cancel,
                            )?;
                            Ok((
                                Some((pl_id, pl_title.clone())),
                                format!("Playlist '{pl_title}' criada e vídeo salvo."),
                            ))
                        } else {
                            Ok((
                                Some((pl_id, pl_title.clone())),
                                format!("Playlist '{pl_title}' criada."),
                            ))
                        }
                    });
                (Some(session), result)
            });
    }

    pub(crate) fn start_add_to_remote_playlist(
        &mut self,
        playlist_id: String,
        playlist_title: String,
        video_id: String,
        ctx: &egui::Context,
    ) {
        if self.auth.playlist_action_task.is_running() {
            return;
        }
        let oauth_session = self.auth.session.clone();
        let web_session = Self::load_innertube_web_session_cached();
        if oauth_session.is_none() && web_session.is_none() {
            self.start_reauth(ctx);
            return;
        }
        self.auth
            .playlist_action_task
            .spawn_with_cancel(ctx.clone(), move |cancel| {
                if let Some(mut it_sess) = web_session {
                    match playlist_sync::add_video_to_playlist_innertube(
                        &playlist_id,
                        &video_id,
                        &mut it_sess,
                        &cancel,
                    ) {
                        Ok(_) => {
                            return (
                                oauth_session,
                                Ok((None, format!("Vídeo salvo na playlist '{playlist_title}'."))),
                            );
                        }
                        Err(err) if oauth_session.is_none() => {
                            return (None, Err(err));
                        }
                        Err(_) => {}
                    }
                }
                let Some(mut session) = oauth_session else {
                    return (
                        None,
                        Err("Sessão indisponível para salvar na playlist.".into()),
                    );
                };
                let result = session
                    .refresh_if_needed(&cancel)
                    .and_then(|()| {
                        oauth::add_video_to_playlist(
                            &session.access_token,
                            &playlist_id,
                            &video_id,
                            &cancel,
                        )
                    })
                    .map(|_| (None, format!("Vídeo salvo na playlist '{playlist_title}'.")));
                (Some(session), result)
            });
    }

    pub(crate) fn poll_playlist_action(&mut self, _ctx: &egui::Context) {
        if let Some((session, result)) = self.auth.playlist_action_task.poll() {
            let _ = self.set_oauth_session(session);
            match result {
                Ok((created, notice)) => {
                    if let Some((pl_id, pl_title)) = created
                        && !self
                            .auth
                            .remote_playlists
                            .iter()
                            .any(|(id, _)| id == &pl_id)
                    {
                        self.auth
                            .remote_playlists
                            .push((pl_id.clone(), pl_title.clone()));
                        let owner = self
                            .auth
                            .my_channel_title
                            .clone()
                            .unwrap_or_else(|| self.pick("Privado", "Private").to_owned());
                        let new_card = playlist_sync::SyncedPlaylistCard {
                            id: pl_id.clone(),
                            title: pl_title,
                            thumbnail_url: String::new(),
                            video_count: Some(0),
                            badge_text: self.pick("0 vídeos", "0 videos").to_owned(),
                            owner_label: owner.clone(),
                            subtitle_line: format!("{owner} · Playlist"),
                            updated_text: None,
                            privacy: playlist_sync::PlaylistPrivacy::Private,
                            kind: playlist_sync::PlaylistKind::Playlist,
                            is_mix: false,
                            is_music: false,
                            is_owned: true,
                            is_saved: false,
                            seed_video_id: None,
                            browse_id: Some(format!("VL{pl_id}")),
                            has_full_playlist_link: true,
                            stack_color_dark: None,
                            stack_color_light: None,
                        };
                        self.library
                            .remote_playlists
                            .oauth_items
                            .insert(0, new_card.clone());
                        if !self.library.remote_playlists.all_items.is_empty()
                            && !self
                                .library
                                .remote_playlists
                                .all_items
                                .iter()
                                .any(|c| c.id == pl_id)
                        {
                            self.library.remote_playlists.all_items.insert(0, new_card);
                        }
                        self.rebuild_remote_playlists_view(None);
                    }
                    self.message = notice;
                }
                Err(error) => {
                    self.message = error;
                }
            }
        }
    }

    pub(crate) fn rebuild_remote_playlists_view(
        &mut self,
        server_filtered: Option<Vec<playlist_sync::SyncedPlaylistCard>>,
    ) {
        let state = &mut self.library.remote_playlists;
        let filter = state.active_filter;
        let sort = state.active_sort;
        let mut combined = if let Some(cards) = server_filtered {
            cards
        } else if !state.all_items.is_empty() {
            state
                .all_items
                .iter()
                .filter(|c| c.matches_filter(filter))
                .cloned()
                .collect()
        } else {
            state
                .oauth_items
                .iter()
                .filter(|c| c.matches_filter(filter))
                .cloned()
                .collect()
        };
        if sort == playlist_sync::PlaylistSort::AlphabeticalAZ
            && state.server_sorted_for != Some(playlist_sync::PlaylistSort::AlphabeticalAZ)
        {
            combined.sort_by_key(|c| c.title.to_lowercase());
        }
        state.items = combined;
        state.revision = state.revision.wrapping_add(1);
    }

    pub(crate) fn on_enter_playlists_section(&mut self, ctx: &egui::Context) {
        if cfg!(test) {
            return;
        }
        if self.auth.authenticated
            && self.auth.remote_playlists.is_empty()
            && !self.auth.remote_playlists_task.is_running()
        {
            self.start_fetch_remote_playlists(ctx);
        }
        let has_session = self.auth.youtube_web_session.is_some()
            || (!self.auth.youtube_web_session_checked && self.auth.cached_has_saved_profile);
        if has_session
            && !self.library.remote_playlists.loaded
            && self.library.remote_playlists.rx.is_none()
        {
            self.start_playlists_sync(false, false, ctx);
        } else if !has_session && !self.library.remote_playlists.loaded {
            self.library.remote_playlists.needs_connect = true;
        }
    }

    pub(crate) fn start_playlists_sync(
        &mut self,
        force_refresh: bool,
        allow_interactive: bool,
        ctx: &egui::Context,
    ) {
        if cfg!(test) {
            return;
        }
        if self.auth.authenticated
            && (force_refresh || self.auth.remote_playlists.is_empty())
            && !self.auth.remote_playlists_task.is_running()
        {
            self.start_fetch_remote_playlists(ctx);
        }
        if !force_refresh
            && (self.library.remote_playlists.rx.is_some()
                || (self.library.remote_playlists.loaded
                    && self.library.remote_playlists.error.is_none()))
        {
            return;
        }
        if let Some(cancel) = self.library.remote_playlists.cancel.take() {
            cancel.cancel();
        }
        self.library.remote_playlists.next_request_id = self
            .library
            .remote_playlists
            .next_request_id
            .wrapping_add(1);
        let request_id = self.library.remote_playlists.next_request_id;
        self.library.remote_playlists.active_request_id = Some(request_id);
        self.library.remote_playlists.active_filter = playlist_sync::PlaylistCategoryFilter::All;
        self.library.remote_playlists.active_sort = playlist_sync::PlaylistSort::RecentlyAdded;
        self.library.remote_playlists.server_sorted_for = None;
        self.library.remote_playlists.error = None;
        self.library.remote_playlists.needs_connect = false;
        self.library.remote_playlists.initial_sync_attempted = true;
        self.library.remote_playlists.revision =
            self.library.remote_playlists.revision.wrapping_add(1);

        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let (rx, cancel) = playlist_sync::start_playlists_feed_sync(
            request_id,
            playlist_sync::PlaylistsFetchMode::Initial,
            self.auth.youtube_web_session.clone(),
            allow_interactive,
            hl,
            ctx,
        );
        self.library.remote_playlists.rx = Some(rx);
        self.library.remote_playlists.cancel = Some(cancel);
    }

    pub(crate) fn start_playlists_filtered_sync(
        &mut self,
        filter: playlist_sync::PlaylistCategoryFilter,
        sort: playlist_sync::PlaylistSort,
        ctx: &egui::Context,
    ) {
        let prev_filter = self.library.remote_playlists.active_filter;
        let prev_sort = self.library.remote_playlists.active_sort;
        self.library.remote_playlists.active_filter = filter;
        self.library.remote_playlists.active_sort = sort;
        if sort != prev_sort {
            self.library.remote_playlists.server_sorted_for = None;
        }
        self.rebuild_remote_playlists_view(None);
        if cfg!(test) {
            return;
        }
        let has_session = self.auth.youtube_web_session.is_some()
            || (!self.auth.youtube_web_session_checked && self.auth.cached_has_saved_profile);
        if !has_session {
            return;
        }
        let mode = if filter != prev_filter {
            if filter == playlist_sync::PlaylistCategoryFilter::All {
                self.library
                    .remote_playlists
                    .chips
                    .get(&filter)
                    .cloned()
                    .map(|act| playlist_sync::PlaylistsFetchMode::Filter(filter, sort, act))
                    .or_else(|| {
                        self.library
                            .remote_playlists
                            .all_items
                            .is_empty()
                            .then_some(playlist_sync::PlaylistsFetchMode::Initial)
                    })
            } else {
                self.library
                    .remote_playlists
                    .chips
                    .get(&filter)
                    .cloned()
                    .map(|act| playlist_sync::PlaylistsFetchMode::Filter(filter, sort, act))
            }
        } else if sort != prev_sort {
            self.library
                .remote_playlists
                .sort_options
                .iter()
                .find(|opt| opt.sort == sort)
                .map(|opt| {
                    playlist_sync::PlaylistsFetchMode::Sort(filter, sort, opt.action.clone())
                })
        } else {
            None
        };
        let Some(mode) = mode else {
            return;
        };
        if let Some(cancel) = self.library.remote_playlists.cancel.take() {
            cancel.cancel();
        }
        self.library.remote_playlists.next_request_id = self
            .library
            .remote_playlists
            .next_request_id
            .wrapping_add(1);
        let request_id = self.library.remote_playlists.next_request_id;
        self.library.remote_playlists.active_request_id = Some(request_id);
        self.library.remote_playlists.error = None;
        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let (rx, cancel) = playlist_sync::start_playlists_feed_sync(
            request_id,
            mode,
            self.auth.youtube_web_session.clone(),
            false,
            hl,
            ctx,
        );
        self.library.remote_playlists.rx = Some(rx);
        self.library.remote_playlists.cancel = Some(cancel);
    }

    pub(crate) fn poll_playlists_sync(&mut self, _ctx: &egui::Context) {
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.library.remote_playlists.rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for msg in msgs {
            match msg {
                playlist_sync::PlaylistsSyncMessage::SessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                playlist_sync::PlaylistsSyncMessage::Page {
                    request_id,
                    filter,
                    sort,
                    page_index,
                    page,
                } => {
                    if self.library.remote_playlists.active_request_id != Some(request_id) {
                        continue;
                    }
                    if page.signed_out {
                        self.library.remote_playlists.needs_connect = true;
                    } else if !page.cards.is_empty() || !page.chips.is_empty() {
                        self.library.remote_playlists.needs_connect = false;
                    }
                    if !page.chips.is_empty() {
                        for (k, v) in page.chips {
                            self.library.remote_playlists.chips.insert(k, v);
                        }
                    }
                    if !page.sort_options.is_empty() {
                        self.library.remote_playlists.sort_options = page.sort_options;
                    }
                    self.library.remote_playlists.next_continuation = page.next_continuation;

                    if page_index == 0 {
                        if let Some(detected_sort) = page.selected_sort {
                            if filter == playlist_sync::PlaylistCategoryFilter::All {
                                self.library.remote_playlists.active_sort = detected_sort;
                            }
                            self.library.remote_playlists.server_sorted_for = Some(detected_sort);
                        } else if filter == self.library.remote_playlists.active_filter
                            && sort == self.library.remote_playlists.active_sort
                        {
                            self.library.remote_playlists.server_sorted_for = Some(sort);
                        }
                    }

                    let is_base_feed = filter == playlist_sync::PlaylistCategoryFilter::All;
                    if is_base_feed {
                        if page_index == 0 {
                            self.library.remote_playlists.all_items = page.cards;
                        } else {
                            for card in page.cards {
                                if !self
                                    .library
                                    .remote_playlists
                                    .all_items
                                    .iter()
                                    .any(|c| c.id == card.id)
                                {
                                    self.library.remote_playlists.all_items.push(card);
                                }
                            }
                        }
                        if self.auth.remote_playlists.is_empty() {
                            self.auth.remote_playlists = self
                                .library
                                .remote_playlists
                                .all_items
                                .iter()
                                .filter(|c| {
                                    c.is_owned
                                        && !c.is_mix
                                        && c.id != "WL"
                                        && c.id != "LL"
                                        && !c.id.starts_with("local:")
                                })
                                .map(|c| (c.id.clone(), c.title.clone()))
                                .collect();
                        }
                        self.library.remote_playlists.loaded = true;
                        self.rebuild_remote_playlists_view(None);
                    } else if filter == self.library.remote_playlists.active_filter {
                        if page_index == 0 {
                            self.rebuild_remote_playlists_view(Some(page.cards));
                        } else {
                            let mut current = self.library.remote_playlists.items.clone();
                            for card in page.cards {
                                if !current.iter().any(|c| c.id == card.id) {
                                    current.push(card);
                                }
                            }
                            self.rebuild_remote_playlists_view(Some(current));
                        }
                        self.library.remote_playlists.loaded = true;
                    }
                }
                playlist_sync::PlaylistsSyncMessage::Finished {
                    request_id,
                    filter,
                    sort,
                    auth_expired,
                    result,
                } => {
                    if self.library.remote_playlists.active_request_id != Some(request_id) {
                        continue;
                    }
                    self.library.remote_playlists.rx = None;
                    self.library.remote_playlists.cancel = None;
                    self.library.remote_playlists.active_request_id = None;
                    if auth_expired {
                        self.auth.youtube_web_session = None;
                        self.auth.youtube_web_session_checked = true;
                        self.library.remote_playlists.needs_connect = true;
                    }
                    match result {
                        Ok(_) => {
                            self.library.remote_playlists.loaded = true;
                            if !auth_expired {
                                self.library.remote_playlists.needs_connect = false;
                            }
                            self.library.remote_playlists.error = None;
                            if (filter != self.library.remote_playlists.active_filter
                                || (sort != self.library.remote_playlists.active_sort
                                    && self.library.remote_playlists.server_sorted_for
                                        != Some(self.library.remote_playlists.active_sort)))
                                && !self.library.remote_playlists.all_items.is_empty()
                            {
                                self.rebuild_remote_playlists_view(None);
                            }
                        }
                        Err(err) => {
                            if self.library.remote_playlists.all_items.is_empty()
                                && (!auth_expired
                                    || self.library.remote_playlists.oauth_items.is_empty())
                            {
                                self.library.remote_playlists.error = Some(err);
                            }
                        }
                    }
                    self.library.remote_playlists.revision =
                        self.library.remote_playlists.revision.wrapping_add(1);
                }
            }
        }
        if disconnected && self.library.remote_playlists.rx.is_some() {
            self.library.remote_playlists.rx = None;
            self.library.remote_playlists.cancel = None;
            self.library.remote_playlists.active_request_id = None;
            self.library.remote_playlists.revision =
                self.library.remote_playlists.revision.wrapping_add(1);
        }
    }

    pub(crate) fn on_enter_watch_later_section(&mut self, ctx: &egui::Context) {
        if cfg!(test) {
            return;
        }
        let has_session = self.auth.youtube_web_session.is_some()
            || (!self.auth.youtube_web_session_checked && self.auth.cached_has_saved_profile);
        if has_session
            && !self.library.watch_later_remote.loaded
            && self.library.watch_later_remote.rx.is_none()
        {
            self.start_watch_later_sync(false, false, ctx);
        } else if !has_session && !self.library.watch_later_remote.loaded {
            self.library.watch_later_remote.needs_connect = true;
        }
    }

    pub(crate) fn start_watch_later_sync(
        &mut self,
        force_refresh: bool,
        allow_interactive: bool,
        ctx: &egui::Context,
    ) {
        if cfg!(test) {
            return;
        }
        if !force_refresh
            && (self.library.watch_later_remote.rx.is_some()
                || (self.library.watch_later_remote.loaded
                    && self.library.watch_later_remote.error.is_none()))
        {
            return;
        }
        if let Some(cancel) = self.library.watch_later_remote.cancel.take() {
            cancel.cancel();
        }
        self.library.watch_later_remote.next_request_id = self
            .library
            .watch_later_remote
            .next_request_id
            .wrapping_add(1);
        let request_id = self.library.watch_later_remote.next_request_id;
        self.library.watch_later_remote.active_request_id = Some(request_id);
        self.library.watch_later_remote.playlist_id = Some("WL".into());
        self.library.watch_later_remote.items.clear();
        self.library.watch_later_remote.seen_keys.clear();
        self.library.watch_later_remote.meta = None;
        self.library.watch_later_remote.total_results = None;
        self.library.watch_later_remote.loaded = false;
        self.library.watch_later_remote.needs_connect = false;
        self.library.watch_later_remote.error = None;
        self.library.watch_later_remote.merged_cache_key = None;
        self.library.watch_later_remote.revision =
            self.library.watch_later_remote.revision.wrapping_add(1);

        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let oauth_token = self.auth.session.as_ref().map(|s| s.access_token.clone());
        let (rx, cancel) = playlist_sync::start_playlist_detail_sync(
            request_id,
            "WL".into(),
            None,
            self.auth.youtube_web_session.clone(),
            oauth_token,
            allow_interactive,
            hl,
            ctx,
        );
        self.library.watch_later_remote.rx = Some(rx);
        self.library.watch_later_remote.cancel = Some(cancel);
    }

    pub(crate) fn poll_watch_later_sync(&mut self, _ctx: &egui::Context) {
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.library.watch_later_remote.rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut metadata_changed = false;
        let mut changed = false;
        for msg in msgs {
            match msg {
                playlist_sync::PlaylistDetailSyncMessage::SessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                playlist_sync::PlaylistDetailSyncMessage::Page {
                    request_id,
                    playlist_id,
                    page_index: _,
                    page,
                } => {
                    if self.library.watch_later_remote.active_request_id != Some(request_id) {
                        continue;
                    }
                    if page.signed_out {
                        self.library.watch_later_remote.needs_connect = true;
                    }
                    if let Some(meta) = page.meta {
                        if let Some(total) = meta.total_videos {
                            self.library.watch_later_remote.total_results = Some(total);
                        }
                        self.library.watch_later_remote.meta = Some(meta);
                        changed = true;
                    }
                    let mut batch_videos = Vec::with_capacity(page.items.len());
                    for item in page.items {
                        let key = history::youtube_video_id(&item.result.video.url)
                            .unwrap_or_else(|| item.result.video.url.clone());
                        self.video_metadata
                            .insert(item.result.video.url.clone(), item.result.metadata.clone());
                        metadata_changed = true;
                        batch_videos.push(item.result.video.clone());
                        if self.library.watch_later_remote.seen_keys.insert(key) {
                            self.library.watch_later_remote.items.push(item);
                            changed = true;
                        }
                    }
                    let loading_more = page.next_continuation.is_some();
                    if self
                        .watch
                        .active_playlist
                        .as_ref()
                        .is_some_and(|pl| pl.playlist_id == playlist_id)
                        && let Some(pl) = &mut self.watch.active_playlist
                    {
                        let total_opt = self
                            .library
                            .watch_later_remote
                            .total_results
                            .map(|t| t as usize);
                        pl.append_videos(&batch_videos, total_opt, loading_more);
                        if let Some(meta) = &self.library.watch_later_remote.meta {
                            pl.update_metadata(
                                Some(meta.title.clone()),
                                Some(meta.owner.clone()),
                                Some(meta.privacy),
                                total_opt,
                                loading_more,
                            );
                        }
                    }
                }
                playlist_sync::PlaylistDetailSyncMessage::Finished {
                    request_id,
                    playlist_id,
                    auth_expired,
                    result,
                } => {
                    if self.library.watch_later_remote.active_request_id != Some(request_id) {
                        continue;
                    }
                    self.library.watch_later_remote.rx = None;
                    self.library.watch_later_remote.cancel = None;
                    self.library.watch_later_remote.active_request_id = None;
                    if auth_expired {
                        self.auth.youtube_web_session = None;
                        self.auth.youtube_web_session_checked = true;
                        self.library.watch_later_remote.needs_connect = true;
                    }
                    match result {
                        Ok(_) => {
                            self.library.watch_later_remote.loaded = true;
                            if !auth_expired {
                                self.library.watch_later_remote.needs_connect = false;
                            }
                            self.library.watch_later_remote.error = None;
                        }
                        Err(err) => {
                            self.library.watch_later_remote.error = Some(err);
                        }
                    }
                    if let Some(pl) = &mut self.watch.active_playlist
                        && pl.playlist_id == playlist_id
                    {
                        pl.loading_more = false;
                    }
                    changed = true;
                }
            }
        }
        if disconnected && self.library.watch_later_remote.rx.is_some() {
            self.library.watch_later_remote.rx = None;
            self.library.watch_later_remote.cancel = None;
            self.library.watch_later_remote.active_request_id = None;
            changed = true;
        }
        if metadata_changed {
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        }
        if changed {
            self.library.watch_later_remote.merged_cache_key = None;
            self.library.watch_later_remote.revision =
                self.library.watch_later_remote.revision.wrapping_add(1);
        }
    }

    pub(crate) fn open_remote_playlist_detail(
        &mut self,
        playlist_id: String,
        title_hint: Option<String>,
        seed_video_id: Option<String>,
        ctx: &egui::Context,
    ) {
        self.library.selected_collection_idx = None;
        self.library.detail_chip_filter = 0;
        if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
            self.feed.home.title = None;
            self.feed.home.playlist_total_results = None;
        }
        self.ui.browse_section = BrowseSection::Playlists;
        self.library.active_remote_playlist_title = title_hint.clone();

        if let Some(cancel) = self.library.active_playlist_detail.cancel.take() {
            cancel.cancel();
        }
        self.library.active_playlist_detail.next_request_id = self
            .library
            .active_playlist_detail
            .next_request_id
            .wrapping_add(1);
        let request_id = self.library.active_playlist_detail.next_request_id;
        self.library.active_playlist_detail.active_request_id = Some(request_id);
        self.library.active_playlist_detail.playlist_id = Some(playlist_id.clone());
        self.library.active_playlist_detail.items.clear();
        self.library.active_playlist_detail.seen_keys.clear();
        self.library.active_playlist_detail.total_results = None;
        self.library.active_playlist_detail.loaded = false;
        self.library.active_playlist_detail.needs_connect = false;
        self.library.active_playlist_detail.error = None;
        self.library.active_playlist_detail.merged_cache_key = None;
        self.library.active_playlist_detail.meta =
            title_hint.map(|t| playlist_sync::RemotePlaylistDetailMetadata {
                playlist_id: playlist_id.clone(),
                title: t,
                ..Default::default()
            });
        self.library.active_playlist_detail.revision =
            self.library.active_playlist_detail.revision.wrapping_add(1);

        if cfg!(test) {
            return;
        }
        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let oauth_token = self.auth.session.as_ref().map(|s| s.access_token.clone());
        let (rx, cancel) = playlist_sync::start_playlist_detail_sync(
            request_id,
            playlist_id,
            seed_video_id,
            self.auth.youtube_web_session.clone(),
            oauth_token,
            false,
            hl,
            ctx,
        );
        self.library.active_playlist_detail.rx = Some(rx);
        self.library.active_playlist_detail.cancel = Some(cancel);
    }

    pub(crate) fn poll_active_playlist_detail_sync(&mut self, _ctx: &egui::Context) {
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.library.active_playlist_detail.rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut metadata_changed = false;
        let mut changed = false;
        for msg in msgs {
            match msg {
                playlist_sync::PlaylistDetailSyncMessage::SessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                playlist_sync::PlaylistDetailSyncMessage::Page {
                    request_id,
                    playlist_id,
                    page_index: _,
                    page,
                } => {
                    if self.library.active_playlist_detail.active_request_id != Some(request_id) {
                        continue;
                    }
                    if page.signed_out {
                        self.library.active_playlist_detail.needs_connect = true;
                    }
                    if let Some(meta) = page.meta {
                        if let Some(total) = meta.total_videos {
                            self.library.active_playlist_detail.total_results = Some(total);
                        }
                        self.library.active_playlist_detail.meta = Some(meta);
                        changed = true;
                    }
                    let mut batch_videos = Vec::with_capacity(page.items.len());
                    for item in page.items {
                        let key = history::youtube_video_id(&item.result.video.url)
                            .unwrap_or_else(|| item.result.video.url.clone());
                        self.video_metadata
                            .insert(item.result.video.url.clone(), item.result.metadata.clone());
                        metadata_changed = true;
                        batch_videos.push(item.result.video.clone());
                        if self.library.active_playlist_detail.seen_keys.insert(key) {
                            self.library.active_playlist_detail.items.push(item);
                            changed = true;
                        }
                    }
                    let loading_more = page.next_continuation.is_some();
                    if self
                        .watch
                        .active_playlist
                        .as_ref()
                        .is_some_and(|pl| pl.playlist_id == playlist_id)
                        && let Some(pl) = &mut self.watch.active_playlist
                    {
                        let total_opt = self
                            .library
                            .active_playlist_detail
                            .total_results
                            .map(|t| t as usize);
                        pl.append_videos(&batch_videos, total_opt, loading_more);
                        if let Some(meta) = &self.library.active_playlist_detail.meta {
                            pl.update_metadata(
                                Some(meta.title.clone()),
                                Some(meta.owner.clone()),
                                Some(meta.privacy),
                                total_opt,
                                loading_more,
                            );
                        }
                    }
                }
                playlist_sync::PlaylistDetailSyncMessage::Finished {
                    request_id,
                    playlist_id,
                    auth_expired,
                    result,
                } => {
                    if self.library.active_playlist_detail.active_request_id != Some(request_id) {
                        continue;
                    }
                    self.library.active_playlist_detail.rx = None;
                    self.library.active_playlist_detail.cancel = None;
                    self.library.active_playlist_detail.active_request_id = None;
                    if auth_expired {
                        self.auth.youtube_web_session = None;
                        self.auth.youtube_web_session_checked = true;
                        self.library.active_playlist_detail.needs_connect = true;
                    }
                    match result {
                        Ok(_) => {
                            self.library.active_playlist_detail.loaded = true;
                            if !auth_expired {
                                self.library.active_playlist_detail.needs_connect = false;
                            }
                            self.library.active_playlist_detail.error = None;
                        }
                        Err(err) => {
                            self.library.active_playlist_detail.error = Some(err);
                        }
                    }
                    if let Some(pl) = &mut self.watch.active_playlist
                        && pl.playlist_id == playlist_id
                    {
                        pl.loading_more = false;
                    }
                    changed = true;
                }
            }
        }
        if disconnected && self.library.active_playlist_detail.rx.is_some() {
            self.library.active_playlist_detail.rx = None;
            self.library.active_playlist_detail.cancel = None;
            self.library.active_playlist_detail.active_request_id = None;
            changed = true;
        }
        if metadata_changed {
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        }
        if changed {
            self.library.active_playlist_detail.merged_cache_key = None;
            self.library.active_playlist_detail.revision =
                self.library.active_playlist_detail.revision.wrapping_add(1);
        }
    }

    pub(crate) fn start_watch_mix_or_playlist(
        &mut self,
        card: &playlist_sync::SyncedPlaylistCard,
        ctx: &egui::Context,
    ) {
        if let Some(cancel) = self.watch.mix_cancel.take() {
            cancel.cancel();
        }
        self.watch.mix_rx = None;
        self.watch.mix_active_request_id = None;
        self.watch.mix_next_request_id = self.watch.mix_next_request_id.wrapping_add(1);
        let request_id = self.watch.mix_next_request_id;
        self.watch.mix_active_request_id = Some(request_id);

        if let Some(vid_id) = card
            .seed_video_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            let seed_video = Video {
                title: card.title.clone(),
                channel: card.owner_label.clone(),
                url: format!("https://www.youtube.com/watch?v={vid_id}"),
                thumbnail: card.thumbnail_url.clone(),
                duration: String::new(),
            };
            let pl = app_state::ActivePlaylistPlayback::new(
                card.id.clone(),
                card.title.clone(),
                card.owner_label.clone(),
                card.privacy,
                card.is_mix,
                vec![seed_video],
                card.video_count.map(|c| c as usize),
                0,
                false,
                1,
                true,
                None,
            );
            self.start_playlist_playback(pl);
        } else {
            self.message = self.pick("Carregando Mix...", "Loading Mix...").to_owned();
        }

        if cfg!(test) {
            return;
        }
        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let oauth_token = self.auth.session.as_ref().map(|s| s.access_token.clone());
        let (rx, cancel) = playlist_sync::start_playlist_detail_sync(
            request_id,
            card.id.clone(),
            card.seed_video_id.clone(),
            self.auth.youtube_web_session.clone(),
            oauth_token,
            false,
            hl,
            ctx,
        );
        self.watch.mix_rx = Some(rx);
        self.watch.mix_cancel = Some(cancel);
    }

    pub(crate) fn poll_watch_mix_sync(&mut self, _ctx: &egui::Context) {
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.watch.mix_rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut metadata_changed = false;
        for msg in msgs {
            match msg {
                playlist_sync::PlaylistDetailSyncMessage::SessionUpdated(sess) => {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                playlist_sync::PlaylistDetailSyncMessage::Page {
                    request_id,
                    playlist_id,
                    page_index,
                    page,
                } => {
                    if self.watch.mix_active_request_id != Some(request_id) {
                        continue;
                    }
                    let mut videos = Vec::with_capacity(page.items.len());
                    for item in page.items {
                        self.video_metadata
                            .insert(item.result.video.url.clone(), item.result.metadata.clone());
                        metadata_changed = true;
                        videos.push(item.result.video);
                    }
                    let loading_more = page.next_continuation.is_some();
                    let meta_title = page.meta.as_ref().map(|m| m.title.clone());
                    let meta_owner = page.meta.as_ref().map(|m| m.owner.clone());
                    let meta_privacy = page.meta.as_ref().map(|m| m.privacy);
                    let meta_total = page
                        .meta
                        .as_ref()
                        .and_then(|m| m.total_videos.map(|c| c as usize));
                    let meta_is_mix = page
                        .meta
                        .as_ref()
                        .map_or(playlist_id.starts_with("RD"), |m| m.is_mix);

                    if self
                        .watch
                        .active_playlist
                        .as_ref()
                        .is_some_and(|pl| pl.playlist_id == playlist_id)
                    {
                        if page_index == 0
                            && let Some(first_real) = videos.first()
                            && self.selected.as_ref().is_some_and(|cur| {
                                Self::same_video_target(&cur.url, &first_real.url)
                            })
                        {
                            self.selected = Some(first_real.clone());
                            self.selected_metadata = self.metadata_for(&first_real.url).cloned();
                        }
                        if let Some(pl) = &mut self.watch.active_playlist {
                            if page_index == 0
                                && let Some(first_real) = videos.first()
                                && let Some(slot0) = pl.videos.first_mut()
                                && Self::same_video_target(&slot0.url, &first_real.url)
                            {
                                *slot0 = first_real.clone();
                            }
                            pl.append_videos(&videos, meta_total, loading_more);
                            pl.update_metadata(
                                meta_title,
                                meta_owner,
                                meta_privacy,
                                meta_total,
                                loading_more,
                            );
                        }
                    } else if !videos.is_empty() && page_index == 0 {
                        let pl = app_state::ActivePlaylistPlayback::new(
                            playlist_id,
                            meta_title.unwrap_or_else(|| "Mix".to_owned()),
                            meta_owner.unwrap_or_default(),
                            meta_privacy.unwrap_or(playlist_sync::PlaylistPrivacy::Unknown),
                            meta_is_mix,
                            videos,
                            meta_total,
                            0,
                            false,
                            1,
                            loading_more,
                            None,
                        );
                        self.start_playlist_playback(pl);
                    }
                }
                playlist_sync::PlaylistDetailSyncMessage::Finished {
                    request_id,
                    playlist_id,
                    auth_expired: _,
                    result,
                } => {
                    if self.watch.mix_active_request_id != Some(request_id) {
                        continue;
                    }
                    self.watch.mix_rx = None;
                    self.watch.mix_cancel = None;
                    self.watch.mix_active_request_id = None;
                    if let Some(pl) = &mut self.watch.active_playlist
                        && pl.playlist_id == playlist_id
                    {
                        pl.loading_more = false;
                    }
                    if let Err(err) = result {
                        self.message = err;
                    }
                }
            }
        }
        if disconnected && self.watch.mix_rx.is_some() {
            self.watch.mix_rx = None;
            self.watch.mix_cancel = None;
            self.watch.mix_active_request_id = None;
        }
        if metadata_changed {
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        }
    }

    pub(crate) fn dispatch_playlist_remove_video(
        &mut self,
        playlist_id: String,
        video_id: String,
        set_video_id: Option<String>,
        ctx: &egui::Context,
    ) {
        if let Some(cancel) = self.library.remote_playlists.action_cancel.take() {
            cancel.cancel();
        }
        let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
        let (rx, cancel) = playlist_sync::start_playlist_remove_video(
            playlist_id,
            video_id,
            set_video_id,
            self.auth.youtube_web_session.clone(),
            hl,
            ctx,
        );
        self.library.remote_playlists.action_rx = Some(rx);
        self.library.remote_playlists.action_cancel = Some(cancel);
    }

    pub(crate) fn poll_playlist_remove_action(&mut self, _ctx: &egui::Context) {
        let msg = self
            .library
            .remote_playlists
            .action_rx
            .as_ref()
            .map(|rx| rx.try_recv());
        match msg {
            Some(Ok(event)) => {
                self.library.remote_playlists.action_rx = None;
                self.library.remote_playlists.action_cancel = None;
                if event.auth_expired {
                    self.auth.youtube_web_session = None;
                    self.auth.youtube_web_session_checked = true;
                } else if let Some(sess) = event.web_session {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                }
                match event.result {
                    Ok(()) => {
                        if event.playlist_id == "WL" {
                            self.library.watch_later_remote.items.retain(|item| {
                                history::youtube_video_id(&item.result.video.url).as_deref()
                                    != Some(event.video_id.as_str())
                            });
                            self.library
                                .watch_later_remote
                                .seen_keys
                                .remove(&event.video_id);
                            if let Some(total) = &mut self.library.watch_later_remote.total_results
                            {
                                *total = total.saturating_sub(1);
                            }
                            self.library.watch_later_remote.merged_cache_key = None;
                            self.library.watch_later_remote.revision =
                                self.library.watch_later_remote.revision.wrapping_add(1);
                        }
                        if self.library.active_playlist_detail.playlist_id.as_deref()
                            == Some(event.playlist_id.as_str())
                        {
                            self.library.active_playlist_detail.items.retain(|item| {
                                history::youtube_video_id(&item.result.video.url).as_deref()
                                    != Some(event.video_id.as_str())
                            });
                            self.library
                                .active_playlist_detail
                                .seen_keys
                                .remove(&event.video_id);
                            if let Some(total) =
                                &mut self.library.active_playlist_detail.total_results
                            {
                                *total = total.saturating_sub(1);
                            }
                            self.library.active_playlist_detail.merged_cache_key = None;
                            self.library.active_playlist_detail.revision =
                                self.library.active_playlist_detail.revision.wrapping_add(1);
                        }
                        self.library.notice = Some(
                            self.pick(
                                "Vídeo removido da playlist.",
                                "Video removed from playlist.",
                            )
                            .to_owned(),
                        );
                    }
                    Err(err) => {
                        self.library.notice = Some(err);
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.library.remote_playlists.action_rx = None;
                self.library.remote_playlists.action_cancel = None;
            }
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
    }

    pub(crate) fn toggle_collection_video(&mut self, name: &str, video: &Video) {
        if self
            .library
            .data
            .toggle_video_in_collection(name, video.clone())
        {
            self.save_library();
        }
    }

    pub(crate) fn video_saved_in_library(&self, video: &Video) -> bool {
        if self.library.data.is_in_watch_later(&video.url)
            || self.library.data.is_in_favorites(&video.url)
        {
            return true;
        }
        let vid = history::youtube_video_id(&video.url);
        let matches = |record: &library::VideoRecord| {
            if let Some(target_id) = vid.as_deref() {
                history::youtube_video_id(&record.video.url).as_deref() == Some(target_id)
            } else {
                record.video.url == video.url
            }
        };
        self.library
            .data
            .collections
            .iter()
            .any(|c| c.videos.iter().any(matches))
    }

    pub(crate) fn save_menu_ui(&mut self, ui: &mut egui::Ui, video: &Video, ctx: &egui::Context) {
        if self.save_menu_contents_ui(ui, video, ctx) {
            self.playback.save_menu_open = false;
        }
    }

    pub(crate) fn save_menu_contents_ui(
        &mut self,
        ui: &mut egui::Ui,
        video: &Video,
        ctx: &egui::Context,
    ) -> bool {
        let mut close_requested = false;
        let palette = self.palette();
        let border = palette.card_alt_stroke;
        let panel_alt = palette.card_alt_bg;
        let text = palette.header_button_fg;
        let muted = palette.neutral_status_fg;
        let vid = history::youtube_video_id(&video.url);

        egui::Frame::NONE
            .fill(panel_alt)
            .stroke(egui::Stroke::new(1.0, border))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Salvar vídeo em...")
                            .color(text)
                            .size(13.0)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("✕").color(muted).size(12.0))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            close_requested = true;
                        }
                    });
                });
                ui.add_space(6.0);

                let in_watch_later = self.library.data.is_in_watch_later(&video.url);
                let mut wl_checked = in_watch_later;
                if ui
                    .checkbox(
                        &mut wl_checked,
                        RichText::new("⏱ Assistir mais tarde")
                            .color(text)
                            .size(12.5),
                    )
                    .changed()
                    && vid.is_some()
                {
                    self.library.data.toggle_watch_later(video.clone());
                    self.save_library();
                }

                let in_favorites = self.library.data.is_in_favorites(&video.url);
                let mut fav_checked = in_favorites;
                if ui
                    .checkbox(
                        &mut fav_checked,
                        RichText::new("★ Favoritos").color(text).size(12.5),
                    )
                    .changed()
                    && vid.is_some()
                {
                    self.library.data.toggle_favorite(video.clone());
                    self.save_library();
                }

                let collection_names: Vec<(String, bool)> = self
                    .library
                    .data
                    .collections
                    .iter()
                    .map(|c| {
                        let present = c.videos.iter().any(|v| {
                            vid.as_deref().is_some_and(|id| {
                                history::youtube_video_id(&v.video.url).as_deref() == Some(id)
                            }) || v.video.url == video.url
                        });
                        (c.name.clone(), present)
                    })
                    .collect();
                for (col_name, present) in collection_names {
                    let mut checked = present;
                    if ui
                        .checkbox(
                            &mut checked,
                            RichText::new(format!("📁 {col_name}"))
                                .color(text)
                                .size(12.5),
                        )
                        .changed()
                    {
                        self.toggle_collection_video(&col_name, video);
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.library.collection_name)
                            .hint_text("Nova coleção local...")
                            .desired_width(180.0),
                    );
                    let can_create = !self.library.collection_name.trim().is_empty();
                    let create_btn = egui::Button::new(
                        RichText::new("+ Criar coleção")
                            .color(palette.header_button_fg)
                            .size(12.0),
                    )
                    .fill(palette.header_button_bg)
                    .corner_radius(egui::CornerRadius::same(14));
                    if ui.add_enabled(can_create, create_btn).clicked() {
                        let name = std::mem::take(&mut self.library.collection_name);
                        self.toggle_collection_video(&name, video);
                    }
                });

                if self.auth.authenticated {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Playlists da conta Google")
                            .color(muted)
                            .size(11.5)
                            .strong(),
                    );
                    let remote_list = self.auth.remote_playlists.clone();
                    if remote_list.is_empty() && self.auth.remote_playlists_task.is_running() {
                        ui.label(
                            RichText::new("Carregando playlists...")
                                .color(muted)
                                .size(11.5),
                        );
                    } else {
                        for (pl_id, pl_title) in remote_list {
                            let btn = egui::Button::new(
                                RichText::new(format!("+ Salvar em {pl_title}"))
                                    .color(palette.header_button_fg)
                                    .size(12.0),
                            )
                            .fill(palette.header_button_bg)
                            .corner_radius(egui::CornerRadius::same(14));
                            if ui.add(btn).clicked()
                                && let Some(id) = vid.clone()
                            {
                                self.start_add_to_remote_playlist(pl_id, pl_title, id, ctx);
                            }
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(
                                &mut self.playback.new_remote_playlist_title,
                            )
                            .hint_text("Nova playlist no YouTube...")
                            .desired_width(180.0),
                        );
                        let can_create_remote =
                            !self.playback.new_remote_playlist_title.trim().is_empty()
                                && !self.auth.playlist_action_task.is_running();
                        let create_remote_btn = egui::Button::new(
                            RichText::new("+ Criar no YouTube")
                                .color(palette.header_button_fg)
                                .size(12.0),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(egui::CornerRadius::same(14));
                        if ui
                            .add_enabled(can_create_remote, create_remote_btn)
                            .clicked()
                        {
                            let title =
                                std::mem::take(&mut self.playback.new_remote_playlist_title);
                            self.start_create_remote_playlist(title, vid.clone(), ctx);
                        }
                    });
                }
            });
        close_requested
    }

    #[cfg(test)]
    pub(crate) fn open_channel_for_metadata(
        &mut self,
        metadata: &VideoMetadata,
        ctx: &egui::Context,
    ) -> bool {
        let Some(target) = search::channel_target_for_metadata(metadata) else {
            self.channels.error = Some(
                "Não foi possível abrir o canal: os metadados do vídeo não fornecem um ID UC ou handle @ válido."
                    .into(),
            );
            return false;
        };
        self.open_channel(target, ctx)
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_request(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<ChannelPage, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.videos.task.active_request_id() != Some(request_id)
            || self.channels.loading_target.as_ref() != Some(target)
        {
            return false;
        }
        self.channels.videos.task.cancel();
        self.apply_channel_request(target, result, ctx)
    }

    pub(crate) fn apply_channel_request(
        &mut self,
        target: &ChannelTarget,
        result: Result<ChannelPage, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.loading_target.as_ref() != Some(target) {
            return false;
        }
        self.channels.loading_target = None;
        let is_my_channel = self.auth.my_channel.as_ref() == Some(target)
            || matches!(target, ChannelTarget::Handle(h) if h.eq_ignore_ascii_case("@voce"));
        let resolved = match result {
            Ok(page) => Ok(page),
            Err(error)
                if is_my_channel
                    || error.contains("does not have a")
                    || error.contains("has no videos") =>
            {
                Ok(search::empty_channel_page_for_target(target))
            }
            Err(error) => Err(error),
        };
        match resolved {
            Ok(mut page) => {
                if is_my_channel {
                    if page.title.as_deref().unwrap_or("").trim().is_empty() {
                        page.title =
                            self.auth.my_channel_title.clone().or_else(|| {
                                Some(self.pick("Seu canal", "Your channel").to_owned())
                            });
                    }
                    if page.avatar_url.is_none() {
                        page.avatar_url = self.auth.my_channel_avatar_url.clone();
                    }
                    if page.subscriber_count.is_none() {
                        page.subscriber_count = self.auth.my_channel_subscribers;
                    }
                }
                self.apply_channel_page(ctx, page);
            }
            Err(error) => {
                self.channels.error = Some(error);
            }
        }
        true
    }

    #[allow(dead_code)]
    #[cfg(test)]
    pub(crate) fn complete_channel_outcome(
        &mut self,
        request_id: u64,
        target: &ChannelTarget,
        result: Result<search::ChannelFetchOutcome, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.videos.task.active_request_id() != Some(request_id)
            || self.channels.loading_target.as_ref() != Some(target)
        {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.channels.from_cache = outcome.from_cache;
                self.complete_channel_request(request_id, target, Ok(outcome.page), ctx)
            }
            Err(error) => {
                self.channels.from_cache = false;
                self.complete_channel_request(request_id, target, Err(error), ctx)
            }
        }
    }

    pub(crate) fn apply_channel_outcome(
        &mut self,
        target: &ChannelTarget,
        result: Result<search::ChannelFetchOutcome, String>,
        ctx: &egui::Context,
    ) -> bool {
        if self.channels.loading_target.as_ref() != Some(target) {
            return false;
        }
        match result {
            Ok(outcome) => {
                self.channels.from_cache = outcome.from_cache;
                self.apply_channel_request(target, Ok(outcome.page), ctx)
            }
            Err(error) => {
                self.channels.from_cache = false;
                self.apply_channel_request(target, Err(error), ctx)
            }
        }
    }

    pub(crate) fn close_channel_view(&mut self) {
        self.channels.close_view();
    }

    pub(crate) fn start_remote_playlist_fetch(
        &mut self,
        list_id: &str,
        url: &str,
        ctx: &egui::Context,
    ) {
        self.queue.remote_playlist_id = Some(list_id.to_owned());
        self.feed.home.title = Some(format!(
            "{} ({list_id})",
            self.pick("Playlist pública", "Public playlist")
        ));
        self.apply_search_results(ctx, Vec::new());
        self.message = self
            .pick(
                "Carregando vídeos da playlist...",
                "Loading playlist videos...",
            )
            .into();
        let url_owned = url.to_owned();
        let list_id_owned = list_id.to_owned();
        self.queue.remote_playlist_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let result =
                    search::fetch_public_playlist_videos_with_cancel(&url_owned, 100, &cancel);
                app_state::RemotePlaylistFetchMessage {
                    request_id,
                    playlist_id: list_id_owned,
                    result,
                }
            },
        );
    }

    pub(crate) fn poll_remote_playlist(&mut self, ctx: &egui::Context) {
        if let async_task::AsyncTaskPoll::Ready(msg) = self
            .queue
            .remote_playlist_task
            .poll_matching_id(|msg| msg.request_id)
        {
            match msg.result {
                Ok(items) => {
                    let count = items.len();
                    self.queue.remote_playlist_id = Some(msg.playlist_id.clone());
                    self.apply_search_results(ctx, items);
                    self.message = if count == 0 {
                        self.pick(
                            "Nenhum vídeo público encontrado nesta playlist.",
                            "No public videos found in this playlist.",
                        )
                        .into()
                    } else {
                        format!(
                            "{count} {} ({})",
                            self.pick("vídeos carregados na playlist", "videos loaded in playlist"),
                            msg.playlist_id
                        )
                    };
                }
                Err(err) => {
                    self.message = err;
                }
            }
        }
    }

    pub(crate) fn start_liked_videos_fetch(&mut self, force_refresh: bool, ctx: &egui::Context) {
        let has_oauth = self.auth.authenticated && self.auth.session.is_some();
        let has_web = !has_oauth
            && (self.auth.youtube_web_session.is_some()
                || (!self.auth.youtube_web_session_checked && self.auth.cached_has_saved_profile));
        if !has_oauth && !has_web {
            return;
        }
        if !force_refresh
            && (self.library.liked_videos.loaded || self.library.liked_videos.rx.is_some())
        {
            return;
        }
        if let Some(cancel) = self.library.liked_videos.cancel.take() {
            cancel.cancel();
        }
        self.library.liked_videos.next_request_id =
            self.library.liked_videos.next_request_id.wrapping_add(1);
        let request_id = self.library.liked_videos.next_request_id;
        self.library.liked_videos.active_request_id = Some(request_id);
        self.library.liked_videos.items.clear();
        self.library.liked_videos.seen_keys.clear();
        self.library.liked_videos.total_results = None;
        self.library.liked_videos.loaded = false;
        self.library.liked_videos.error = None;
        self.library.liked_videos.revision = self.library.liked_videos.revision.wrapping_add(1);

        let cancel = process::Cancellation::default();
        self.library.liked_videos.cancel = Some(cancel.clone());
        let (sender, receiver) = mpsc::channel();
        self.library.liked_videos.rx = Some(receiver);
        let ctx_clone = ctx.clone();

        if let Some(mut session) = self.auth.session.clone().filter(|_| has_oauth) {
            std::thread::spawn(move || {
                if let Err(error) = session.refresh_if_needed(&cancel) {
                    let _ = sender.send(LikedVideosFetchMessage::Finished {
                        request_id,
                        token: Some(session),
                        total_results: None,
                        result: Err(error),
                    });
                    ctx_clone.request_repaint();
                    return;
                }
                let progress_sender = sender.clone();
                let progress_ctx = ctx_clone.clone();
                let outcome = oauth::liked_videos_feed_progressive(
                    &session.access_token,
                    &cancel,
                    move |progress| {
                        let _ = progress_sender.send(LikedVideosFetchMessage::Progress {
                            request_id,
                            batch: progress.batch,
                            total_results: progress.total_results,
                        });
                        progress_ctx.request_repaint();
                    },
                );
                let (total_results, result) = match outcome {
                    Ok(out) => (out.total_results, Ok(())),
                    Err(err) => (None, Err(err)),
                };
                let _ = sender.send(LikedVideosFetchMessage::Finished {
                    request_id,
                    token: Some(session),
                    total_results,
                    result,
                });
                ctx_clone.request_repaint();
            });
        } else {
            let web_session = self.auth.youtube_web_session.clone();
            let hl = if self.ui_pt() { "pt" } else { "en" }.to_owned();
            let (inner_rx, inner_cancel) = playlist_sync::start_playlist_detail_sync(
                request_id,
                "LL".into(),
                None,
                web_session,
                None,
                false,
                hl,
                ctx,
            );
            std::thread::spawn(move || {
                let mut known_total: Option<u64> = None;
                for msg in inner_rx {
                    if cancel.cancelled() {
                        inner_cancel.cancel();
                        break;
                    }
                    match msg {
                        playlist_sync::PlaylistDetailSyncMessage::SessionUpdated(_) => {}
                        playlist_sync::PlaylistDetailSyncMessage::Page { page, .. } => {
                            if let Some(total) = page.meta.as_ref().and_then(|m| m.total_videos) {
                                known_total = Some(total);
                            }
                            let batch: Vec<search::SearchResult> =
                                page.items.into_iter().map(|it| it.result).collect();
                            let _ = sender.send(LikedVideosFetchMessage::Progress {
                                request_id,
                                batch,
                                total_results: known_total,
                            });
                            ctx_clone.request_repaint();
                        }
                        playlist_sync::PlaylistDetailSyncMessage::Finished { result, .. } => {
                            let _ = sender.send(LikedVideosFetchMessage::Finished {
                                request_id,
                                token: None,
                                total_results: known_total,
                                result: result.map(|_| ()),
                            });
                            ctx_clone.request_repaint();
                            break;
                        }
                    }
                }
            });
        }
    }

    pub(crate) fn cancel_liked_videos_fetch(&mut self) {
        if let Some(cancel) = self.library.liked_videos.cancel.take() {
            cancel.cancel();
        }
        self.library.liked_videos.rx = None;
        self.library.liked_videos.active_request_id = None;
        self.library.liked_videos.revision = self.library.liked_videos.revision.wrapping_add(1);
    }

    pub(crate) fn poll_liked_videos(&mut self) {
        let mut messages = Vec::new();
        let mut disconnected = false;
        if let Some(receiver) = &self.library.liked_videos.rx {
            loop {
                match receiver.try_recv() {
                    Ok(msg) => messages.push(msg),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut changed = false;
        let mut metadata_changed = false;
        for msg in messages {
            match msg {
                LikedVideosFetchMessage::Progress {
                    request_id,
                    batch,
                    total_results,
                } => {
                    if self.library.liked_videos.active_request_id != Some(request_id) {
                        continue;
                    }
                    if total_results.is_some()
                        && self.library.liked_videos.total_results != total_results
                    {
                        self.library.liked_videos.total_results = total_results;
                        changed = true;
                    }
                    let mut liked_playlist_batch = Vec::with_capacity(batch.len());
                    for item in batch {
                        let key = history::youtube_video_id(&item.video.url)
                            .unwrap_or_else(|| item.video.url.clone());
                        if self.library.liked_videos.seen_keys.insert(key) {
                            self.video_metadata
                                .insert(item.video.url.clone(), item.metadata.clone());
                            metadata_changed = true;
                            liked_playlist_batch.push(item.video.clone());
                            self.library.liked_videos.items.push(item);
                            changed = true;
                        }
                    }
                    if !liked_playlist_batch.is_empty()
                        && let Some(pl) = &mut self.watch.active_playlist
                        && pl.playlist_id == "LL"
                    {
                        let total_opt = self.library.liked_videos.total_results.map(|t| t as usize);
                        pl.append_videos(&liked_playlist_batch, total_opt, true);
                    }
                }
                LikedVideosFetchMessage::Finished {
                    request_id,
                    token,
                    total_results,
                    result,
                } => {
                    if self.library.liked_videos.active_request_id != Some(request_id) {
                        continue;
                    }
                    self.library.liked_videos.rx = None;
                    self.library.liked_videos.cancel = None;
                    self.library.liked_videos.active_request_id = None;
                    if let Some(session) = token
                        && (self.auth.session.is_some() || self.auth.authenticated)
                    {
                        self.auth.authenticated = true;
                        let _ = self.set_oauth_session(Some(session));
                    }
                    if total_results.is_some() {
                        self.library.liked_videos.total_results = total_results;
                    }
                    match result {
                        Ok(()) => {
                            self.library.liked_videos.loaded = true;
                            self.library.liked_videos.error = None;
                        }
                        Err(err) => {
                            self.library.liked_videos.error = Some(err);
                        }
                    }
                    if let Some(pl) = &mut self.watch.active_playlist
                        && pl.playlist_id == "LL"
                    {
                        pl.loading_more = false;
                    }
                    changed = true;
                }
            }
        }
        if disconnected && self.library.liked_videos.rx.is_some() {
            self.library.liked_videos.rx = None;
            self.library.liked_videos.cancel = None;
            self.library.liked_videos.active_request_id = None;
            if self.library.liked_videos.error.is_none() && !self.library.liked_videos.loaded {
                self.library.liked_videos.error =
                    Some("A consulta dos vídeos com Gostei foi interrompida.".into());
            }
            changed = true;
        }
        if metadata_changed {
            self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
        }
        if changed {
            self.library.liked_videos.revision = self.library.liked_videos.revision.wrapping_add(1);
        }
    }

    pub(crate) fn poll_channel(&mut self, ctx: &egui::Context) {
        let had_active = self.channels.videos.task.is_active();
        match self
            .channels
            .videos
            .task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                let _ = self.apply_channel_outcome(&msg.target, msg.result, ctx);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if had_active {
                    self.channels.error = Some("A consulta do canal foi interrompida.".into());
                }
                self.channels.loading_target = None;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn poll_channel_more_videos(&mut self) {
        let had_active = self.channels.videos.more_task.is_active();
        match self
            .channels
            .videos
            .more_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                let _ = self.apply_channel_more_videos(&msg.target, msg.result);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if had_active {
                    self.channels.videos.more_error =
                        Some("O carregamento de mais vídeos do canal foi interrompido.".into());
                }
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn poll_channel_playlists(&mut self, ctx: &egui::Context) {
        let had_active = self.channels.playlists.task.is_active();
        match self
            .channels
            .playlists
            .task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                if let Some(sess) = msg.oauth_session {
                    let _ = self.set_oauth_session(Some(sess));
                }
                let _ = self.apply_channel_playlists(&msg.target, msg.result, ctx);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if had_active {
                    self.channels.playlists.error =
                        Some("A consulta de playlists foi interrompida.".into());
                }
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn poll_channel_playlist_videos(&mut self) {
        let had_active = self.channels.playlist_detail.task.is_active();
        match self
            .channels
            .playlist_detail
            .task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                if let Some(sess) = msg.oauth_session {
                    let _ = self.set_oauth_session(Some(sess));
                }
                let _ =
                    self.apply_channel_playlist_videos(&msg.target, &msg.playlist_id, msg.result);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if had_active {
                    self.channels.playlist_detail.error =
                        Some("A consulta dos vídeos da playlist foi interrompida.".into());
                }
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn poll_channel_about(&mut self) {
        let had_active = self.channels.about.task.is_active();
        match self
            .channels
            .about
            .task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                let _ = self.apply_channel_about(&msg.target, msg.result);
            }
            async_task::AsyncTaskPoll::Disconnected => {
                if had_active {
                    self.channels.about.error =
                        Some("A consulta de informações da aba Sobre foi interrompida.".into());
                }
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn save_library(&mut self) {
        self.library.revision = self.library.revision.wrapping_add(1);
        if self.library.read_failed {
            return;
        }
        let Some(path) = self.library.path.clone() else {
            self.library.error =
                Some("Não foi possível salvar a biblioteca: local library path unavailable".into());
            return;
        };
        let expected_fp = self
            .persistence
            .last_fingerprint(persistence::PersistenceSlot::Library)
            .or(self.library.loaded_fingerprint);
        let current_fp = history::file_fingerprint(&path);
        if current_fp != expected_fp && current_fp.is_some() {
            if let Err(error) = library::load(&path) {
                self.library.read_failed = true;
                self.library.error = Some(format!(
                    "Não foi possível salvar a biblioteca (arquivo externo inválido preservado): {error}"
                ));
                return;
            }
            self.library.loaded_fingerprint = current_fp;
            self.persistence.record_loaded_fingerprint(
                persistence::PersistenceSlot::Library,
                path.clone(),
                current_fp,
            );
        }
        self.persistence
            .schedule_library(path, self.library.data.clone(), expected_fp);
        self.library.error = None;
    }

    fn start_channel_continuation_job(
        &mut self,
        browse_id: Option<String>,
        params: Option<String>,
        continuation: Option<String>,
        mode: app_state::ChannelContinuationTarget,
        ctx: &egui::Context,
    ) -> bool {
        let Some(target) = self.channels.page.as_ref().map(|p| p.target.clone()) else {
            return false;
        };
        self.channels.videos.more_error = None;
        let language = self.settings.prefs.value.language;
        let web_session = self.resolve_innertube_web_session();
        self.channels
            .videos
            .continuation_task
            .spawn_with_id_and_cancel(ctx.clone(), move |request_id, cancel| {
                let result = search::fetch_channel_tab_continuation(
                    browse_id.as_deref(),
                    params.as_deref(),
                    continuation.as_deref(),
                    language,
                    None,
                    web_session.as_ref(),
                    &cancel,
                );
                app_state::ChannelContinuationFetchMessage {
                    request_id,
                    target,
                    mode,
                    result,
                }
            });
        true
    }

    pub(crate) fn start_load_more_channel_continuation(&mut self, ctx: &egui::Context) {
        if self.channels.is_loading() || self.channels.is_more_videos_loading() {
            return;
        }
        let Some(token) = self
            .channels
            .page
            .as_ref()
            .and_then(|p| p.next_continuation.clone())
        else {
            return;
        };
        let mode = match self.channels.tab {
            ChannelTab::Shorts => app_state::ChannelContinuationTarget::ShortsAppend,
            ChannelTab::Streams => app_state::ChannelContinuationTarget::StreamsAppend,
            _ => app_state::ChannelContinuationTarget::VideosAppend,
        };
        let _ = self.start_channel_continuation_job(None, None, Some(token), mode, ctx);
    }

    pub(crate) fn select_channel_sort_chip(&mut self, continuation: String, ctx: &egui::Context) {
        if let Some(page) = &mut self.channels.page {
            for chip in &mut page.sort_chips {
                chip.selected = chip.continuation == continuation;
            }
        }
        let mode = match self.channels.tab {
            ChannelTab::Shorts => app_state::ChannelContinuationTarget::ShortsReplace,
            ChannelTab::Streams => app_state::ChannelContinuationTarget::StreamsReplace,
            _ => app_state::ChannelContinuationTarget::VideosReplace,
        };
        let _ = self.start_channel_continuation_job(None, None, Some(continuation), mode, ctx);
    }

    pub(crate) fn open_channel_dynamic_tab(
        &mut self,
        endpoint: innertube::ChannelTabEndpoint,
        ctx: &egui::Context,
    ) {
        self.channels.tab = match &endpoint.kind {
            innertube::ChannelTabKind::Home => ChannelTab::Home,
            innertube::ChannelTabKind::Shorts => ChannelTab::Shorts,
            innertube::ChannelTabKind::Live => ChannelTab::Streams,
            innertube::ChannelTabKind::Playlists => ChannelTab::Playlists,
            innertube::ChannelTabKind::Search => ChannelTab::Search,
            _ => ChannelTab::Videos,
        };
        if let Some(page) = &mut self.channels.page {
            for tab in &mut page.tab_endpoints {
                tab.selected = tab.browse_id == endpoint.browse_id && tab.params == endpoint.params;
            }
        }
        if endpoint.kind == innertube::ChannelTabKind::Playlists {
            if !self.channels.playlists.loaded && !self.channels.is_playlists_loading() {
                self.start_channel_playlists(ctx);
            }
            return;
        }
        let _ = self.start_channel_continuation_job(
            Some(endpoint.browse_id),
            endpoint.params,
            None,
            app_state::ChannelContinuationTarget::DynamicTab(endpoint.kind),
            ctx,
        );
    }

    #[cfg(test)]
    pub(crate) fn complete_channel_continuation(
        &mut self,
        msg: app_state::ChannelContinuationFetchMessage,
    ) -> bool {
        if self.channels.videos.continuation_task.active_request_id() != Some(msg.request_id) {
            return false;
        }
        self.channels.videos.continuation_task.cancel();
        self.apply_channel_continuation(msg)
    }

    pub(crate) fn apply_channel_continuation(
        &mut self,
        msg: app_state::ChannelContinuationFetchMessage,
    ) -> bool {
        if self
            .channels
            .page
            .as_ref()
            .is_none_or(|p| p.target != msg.target)
        {
            return false;
        }
        match msg.result {
            Ok(tab_page) => {
                self.channels.videos.more_error = None;
                for item in &tab_page.videos {
                    if let Some(ts) = item.metadata.upcoming_start_unix {
                        self.channels
                            .upcoming_start_unix_by_url
                            .insert(item.video.url.clone(), ts);
                    }
                    self.video_metadata
                        .insert(item.video.url.clone(), item.metadata.clone());
                }
                if !tab_page.community_posts.is_empty() {
                    self.channels.community_posts = tab_page.community_posts.clone();
                }
                if let Some(page) = &mut self.channels.page {
                    page.next_continuation = tab_page.next_continuation;
                    if !tab_page.sort_chips.is_empty() {
                        page.sort_chips = tab_page.sort_chips;
                    }
                    if !tab_page.tab_endpoints.is_empty() {
                        page.tab_endpoints = tab_page.tab_endpoints;
                    }
                    let append_dedup = |dest: &mut Vec<SearchResult>, items: Vec<SearchResult>| {
                        let mut seen: HashSet<String> =
                            dest.iter().map(|i| i.video.url.clone()).collect();
                        for item in items {
                            if seen.insert(item.video.url.clone()) {
                                dest.push(item);
                            }
                        }
                    };
                    match msg.mode {
                        app_state::ChannelContinuationTarget::VideosAppend => {
                            append_dedup(&mut page.videos, tab_page.videos);
                            self.channels.videos.reached_max = false;
                            self.channels.videos.has_more = page.next_continuation.is_some();
                        }
                        app_state::ChannelContinuationTarget::VideosReplace => {
                            page.videos = tab_page.videos;
                            self.channels.videos.sort = search::ChannelVideosSort::Latest;
                            self.channels.videos.sorted_items.clear();
                            self.channels.videos.reached_max = false;
                            self.channels.videos.has_more = page.next_continuation.is_some();
                        }
                        app_state::ChannelContinuationTarget::ShortsAppend => {
                            append_dedup(&mut self.channels.shorts.items, tab_page.videos);
                            self.channels.shorts.loaded = true;
                            self.channels.shorts.error = None;
                        }
                        app_state::ChannelContinuationTarget::ShortsReplace => {
                            self.channels.shorts.items = tab_page.videos;
                            self.channels.shorts.loaded = true;
                            self.channels.shorts.error = None;
                        }
                        app_state::ChannelContinuationTarget::StreamsAppend => {
                            append_dedup(&mut self.channels.streams.items, tab_page.videos);
                            self.channels.streams.loaded = true;
                            self.channels.streams.error = None;
                        }
                        app_state::ChannelContinuationTarget::StreamsReplace => {
                            self.channels.streams.items = tab_page.videos;
                            self.channels.streams.loaded = true;
                            self.channels.streams.error = None;
                        }
                        app_state::ChannelContinuationTarget::DynamicTab(kind) => match kind {
                            innertube::ChannelTabKind::Shorts => {
                                self.channels.shorts.items = tab_page.videos;
                                self.channels.shorts.loaded = true;
                                self.channels.shorts.error = None;
                            }
                            innertube::ChannelTabKind::Live => {
                                self.channels.streams.items = tab_page.videos;
                                self.channels.streams.loaded = true;
                                self.channels.streams.error = None;
                            }
                            innertube::ChannelTabKind::Posts => {}
                            _ => {
                                if !tab_page.videos.is_empty()
                                    || kind != innertube::ChannelTabKind::Home
                                {
                                    page.videos = tab_page.videos;
                                }
                                self.channels.videos.sort = search::ChannelVideosSort::Latest;
                                self.channels.videos.sorted_items.clear();
                                self.channels.videos.reached_max = false;
                                self.channels.videos.has_more = page.next_continuation.is_some();
                            }
                        },
                    }
                }
                self.feed.metadata_revision = self.feed.metadata_revision.wrapping_add(1);
            }
            Err(error) => {
                self.channels.videos.more_error = Some(error);
            }
        }
        true
    }

    pub(crate) fn poll_channel_continuation(&mut self) {
        if let async_task::AsyncTaskPoll::Ready(msg) = self
            .channels
            .videos
            .continuation_task
            .poll_matching_id(|msg| msg.request_id)
        {
            let _ = self.apply_channel_continuation(msg);
        }
    }
}

fn oauth_cards_to_channel_playlists(
    cards: Vec<playlist_sync::SyncedPlaylistCard>,
) -> Vec<search::ChannelPlaylist> {
    cards
        .into_iter()
        .filter(|c| {
            !c.is_mix && c.id != "WL" && c.id != "LL" && search::is_valid_playlist_id(&c.id)
        })
        .map(|c| search::ChannelPlaylist {
            id: c.id.clone(),
            title: c.title,
            url: format!("https://www.youtube.com/playlist?list={}", c.id),
            thumbnail: c.thumbnail_url,
            item_count: c.video_count,
            description: None,
            privacy: c.privacy,
        })
        .collect()
}
