use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};

impl FastTube {
    pub(crate) fn close_navigation_drawer_if_overlay(&mut self, ctx: &egui::Context) {
        let viewport_width = ctx.input(|i| i.content_rect().width());
        if self.selected.is_some() || viewport_width < layout::PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH
        {
            self.ui.navigation_open = false;
        }
    }

    pub(crate) fn navigation_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        self.ensure_history_synchronized();
        let palette = self.palette();

        egui::ScrollArea::vertical()
            .id_salt("youtube_left_sidebar_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                self.navigation_primary_ui(ui, ctx, palette);
                self.navigation_subscriptions_ui(ui, ctx, palette);
                self.navigation_shortcuts_ui(ui, ctx, palette);
                self.navigation_explore_ui(ui, ctx, palette);
                self.navigation_utilities_ui(ui, ctx, palette);
                self.navigation_now_playing_ui(ui, ctx, palette);
                self.navigation_footer_ui(ui, palette);
                ui.add_space(8.0);
            });
    }

    pub(crate) fn navigation_primary_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let in_home = self.selected.is_none()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Home
            && !self.ui.search_results_mode
            && !matches!(self.feed.filter, FeedFilter::Shorts);

        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Home,
            "In\u{00ed}cio",
            in_home,
            false,
            &palette,
        )
        .clicked()
        {
            self.handle_header_home(ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let in_shorts = self.selected.is_none()
            && !self.ui.settings_open
            && !self.channels.view_open
            && (self.ui.browse_section == BrowseSection::Shorts
                || matches!(self.feed.filter, FeedFilter::Shorts));
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Shorts,
            "Shorts",
            in_shorts,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.ui.browse_section = BrowseSection::Shorts;
            self.feed.filter = FeedFilter::Shorts;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            } else {
                self.sync_player_window_visibility();
            }
            if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
                let _ = self.restore_cached_home_feed(ctx);
            }
            self.invalidate_feed_cache();
            self.ensure_feed_cache();
            if self.feed.cache.ranked.len() < 4
                && self.feed.home.batch_rx.is_none()
                && self.feed.home.rx.is_none()
            {
                self.start_fast_home_feed(false, ctx);
            }
            self.close_navigation_drawer_if_overlay(ctx);
        }
    }

    pub(crate) fn navigation_subscriptions_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        let in_subscriptions = self.selected.is_none()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Subscriptions;
        if sidebar_section_header_item(
            ui,
            "Inscri\u{00e7}\u{00f5}es",
            true,
            in_subscriptions,
            &palette,
        )
        .clicked()
        {
            self.open_subscriptions_section(false, ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }
        ui.add_space(2.0);

        let sidebar_channels = self.feed.home.subscription_channels.clone();
        if !self.auth.authenticated {
            let local_channels = self.followed_channels.data.channels.clone();
            if local_channels.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(self.pick(
                            "Siga canais localmente ou entre com Google",
                            "Follow channels locally or sign in with Google",
                        ))
                        .small()
                        .weak(),
                    );
                });
            } else {
                let visible_limit = if self.ui.subscriptions_expanded {
                    local_channels.len()
                } else {
                    local_channels.len().min(7)
                };
                let mut clicked_local: Option<followed_channels::FollowedChannel> = None;
                for ch in local_channels.iter().take(visible_limit) {
                    let active = matches!(
                        self.current_channel_target(),
                        Some(ChannelTarget::ChannelId(id)) if id == ch.channel_id
                    );
                    if sidebar_channel_item(
                        ui,
                        ctx,
                        &mut self.avatars,
                        ch.avatar_url.as_deref(),
                        &ch.title,
                        active,
                        &palette,
                    )
                    .clicked()
                    {
                        clicked_local = Some(ch.clone());
                    }
                    ui.add_space(2.0);
                }
                if let Some(ch) = clicked_local {
                    self.ui.settings_open = false;
                    self.ui.search_results_mode = false;
                    self.ui.browse_section = BrowseSection::Home;
                    self.close_channel_view();
                    if self.selected.is_some() {
                        self.close_video(ctx);
                    }
                    self.open_channel(ChannelTarget::ChannelId(ch.channel_id), ctx);
                    self.close_navigation_drawer_if_overlay(ctx);
                }
                if local_channels.len() > 7 {
                    let toggle_label = if self.ui.subscriptions_expanded {
                        self.pick("Mostrar menos", "Show less")
                    } else {
                        self.pick("Mostrar mais", "Show more")
                    };
                    let toggle_icon = if self.ui.subscriptions_expanded {
                        YtSidebarIcon::ChevronUp
                    } else {
                        YtSidebarIcon::ChevronDown
                    };
                    if sidebar_pill_item(ui, toggle_icon, toggle_label, false, false, &palette)
                        .clicked()
                    {
                        self.ui.subscriptions_expanded = !self.ui.subscriptions_expanded;
                    }
                }
            }
        } else if sidebar_channels.is_empty()
            && (self.feed.home.channels_rx.is_some()
                || (!self.feed.home.subscription_channels_loaded && self.feed.home.rx.is_some()))
        {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.spinner();
                ui.label(
                    RichText::new("Carregando inscri\u{00e7}\u{00f5}es...")
                        .small()
                        .weak(),
                );
            });
        } else if sidebar_channels.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                let text = if self.feed.home.subscription_channels_error.is_some() {
                    "N\u{00e3}o foi poss\u{00ed}vel carregar as inscri\u{00e7}\u{00f5}es."
                } else {
                    "Voc\u{00ea} ainda n\u{00e3}o segue nenhum canal."
                };
                ui.label(RichText::new(text).small().weak());
            });
        } else {
            let visible_limit = if self.ui.subscriptions_expanded {
                sidebar_channels.len()
            } else {
                sidebar_channels.len().min(7)
            };
            let mut clicked_channel = None;
            for channel in sidebar_channels.iter().take(visible_limit) {
                let active = matches!(
                    self.current_channel_target(),
                    Some(ChannelTarget::ChannelId(channel_id))
                        if channel_id == channel.channel_id
                );
                if sidebar_channel_item(
                    ui,
                    ctx,
                    &mut self.avatars,
                    channel.avatar_url.as_deref(),
                    &channel.title,
                    active,
                    &palette,
                )
                .clicked()
                {
                    clicked_channel = Some(channel.clone());
                }
                ui.add_space(2.0);
            }
            if let Some(channel) = clicked_channel {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Home;
                self.close_channel_view();
                if self.selected.is_some() {
                    self.close_video(ctx);
                }
                self.open_channel(ChannelTarget::ChannelId(channel.channel_id), ctx);
                self.close_navigation_drawer_if_overlay(ctx);
            }

            if sidebar_channels.len() > 7 {
                let toggle_label = if self.ui.subscriptions_expanded {
                    "Mostrar menos"
                } else {
                    "Mostrar mais"
                };
                let toggle_icon = if self.ui.subscriptions_expanded {
                    YtSidebarIcon::ChevronUp
                } else {
                    YtSidebarIcon::ChevronDown
                };
                if sidebar_pill_item(ui, toggle_icon, toggle_label, false, false, &palette)
                    .clicked()
                {
                    self.ui.subscriptions_expanded = !self.ui.subscriptions_expanded;
                }
            }
        }
    }

    pub(crate) fn navigation_shortcuts_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        let you_section_active = self.selected.is_none()
            && !self.ui.settings_open
            && !self.channels.view_open
            && matches!(
                self.ui.browse_section,
                BrowseSection::History | BrowseSection::Library | BrowseSection::Queue
            );
        if sidebar_section_header_item(ui, "Voc\u{00ea}", true, you_section_active, &palette)
            .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.ui.browse_section = BrowseSection::Library;
            self.close_navigation_drawer_if_overlay(ctx);
        }
        ui.add_space(2.0);

        let channel_target = self.current_channel_target();
        let channel_active = self.channels.view_open && !self.ui.settings_open;
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::YourChannel,
            "Seu canal",
            channel_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            if let Some(target) = self.auth.my_channel.clone() {
                self.open_channel(target, ctx);
            } else if self.auth.authenticated {
                self.start_fetch_my_channel(true, ctx);
            } else if channel_target.is_some() {
                self.channels.view_open = true;
                self.sync_player_window_visibility();
            } else {
                self.ui.browse_section = BrowseSection::Home;
                self.feed.filter = FeedFilter::Recommended;
                let _ = self.restore_cached_home_feed(ctx);
                self.invalidate_feed_cache();
            }
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let history_active = self.ui.browse_section == BrowseSection::History
            && !self.channels.view_open
            && self.selected.is_none();
        let history_label = if self.history.data.entries().is_empty() {
            "Hist\u{00f3}rico".to_owned()
        } else {
            format!("Hist\u{00f3}rico ({})", self.history.data.entries().len())
        };
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::History,
            &history_label,
            history_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.ui.browse_section = if self.ui.browse_section == BrowseSection::History {
                BrowseSection::Home
            } else {
                BrowseSection::History
            };
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let playlists_active = self.feed.home.title.as_deref() == Some("Playlists da conta Google")
            || self.feed.home.title.as_deref() == Some("V\u{00ed}deos da playlist");
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Playlists,
            "Playlists",
            playlists_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.ui.browse_section = BrowseSection::Home;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            let interactive = !self.auth.authenticated;
            self.start_browser_feed(":google-playlists".into(), interactive, ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let watch_later_active = self.ui.browse_section == BrowseSection::Library
            && !self.channels.view_open
            && self.selected.is_none();
        let watch_later_label = if self.library.data.watch_later.is_empty() {
            "Assistir mais tarde".to_owned()
        } else {
            format!(
                "Assistir mais tarde ({})",
                self.library.data.watch_later.len()
            )
        };
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::WatchLater,
            &watch_later_label,
            watch_later_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.ui.browse_section = if self.ui.browse_section == BrowseSection::Library {
                BrowseSection::Home
            } else {
                BrowseSection::Library
            };
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let liked_active = self.feed.home.title.as_deref() == Some("V\u{00ed}deos com Gostei")
            && !self.channels.view_open
            && self.selected.is_none();
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Liked,
            "V\u{00ed}deos com \"Gostei\"",
            liked_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.ui.browse_section = BrowseSection::Home;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            let interactive = !self.auth.authenticated;
            self.start_browser_feed(":google-liked".into(), interactive, ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let queue_active = self.ui.browse_section == BrowseSection::Queue
            && !self.channels.view_open
            && self.selected.is_none();
        let queue_label = if self.queue.data.is_empty() {
            "Fila de reprodu\u{00e7}\u{00e3}o".to_owned()
        } else {
            format!(
                "Fila ({}/{})",
                self.queue.data.len(),
                self.queue.data.limit()
            )
        };
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Queue,
            &queue_label,
            queue_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.ui.browse_section = if self.ui.browse_section == BrowseSection::Queue {
                BrowseSection::Home
            } else {
                BrowseSection::Queue
            };
            self.close_navigation_drawer_if_overlay(ctx);
        }
    }

    pub(crate) fn navigation_explore_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        let in_explore = self.selected.is_none()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Explore;
        if sidebar_section_header_item(ui, "Explorar", true, in_explore, &palette).clicked() {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.ui.browse_section = BrowseSection::Explore;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.close_navigation_drawer_if_overlay(ctx);
        }
        ui.add_space(2.0);

        let explore_items = [
            (YtSidebarIcon::Music, "M\u{00fa}sica", FeedFilter::Music),
            (YtSidebarIcon::Live, "Ao vivo", FeedFilter::Live),
            (YtSidebarIcon::Gaming, "Jogos", FeedFilter::Gaming),
            (YtSidebarIcon::Podcasts, "Podcasts", FeedFilter::Podcasts),
            (
                YtSidebarIcon::Trending,
                "Em alta",
                FeedFilter::RecentlyUploaded,
            ),
            (YtSidebarIcon::Mixes, "Mixes", FeedFilter::Mixes),
            (YtSidebarIcon::NewToYou, "Novidades", FeedFilter::NewToYou),
        ];

        let visible_count = if self.ui.explore_expanded {
            explore_items.len()
        } else {
            4
        };

        for (icon, label, filter) in explore_items.into_iter().take(visible_count) {
            let active = self.selected.is_none()
                && !self.ui.settings_open
                && !self.channels.view_open
                && self.ui.browse_section == BrowseSection::Home
                && self.feed.filter == filter;
            if sidebar_pill_item(ui, icon, label, active, false, &palette).clicked() {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Home;
                self.close_channel_view();
                if self.selected.is_some() {
                    self.close_video(ctx);
                }
                self.feed.filter = filter;
                if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
                    let _ = self.restore_cached_home_feed(ctx);
                }
                self.invalidate_feed_cache();
                self.ensure_feed_cache();
                if self.feed.cache.ranked.len() < 6
                    && self.feed.home.batch_rx.is_none()
                    && self.feed.home.rx.is_none()
                {
                    self.start_fast_home_feed(false, ctx);
                }
                self.close_navigation_drawer_if_overlay(ctx);
            }
            ui.add_space(2.0);
        }

        let toggle_label = if self.ui.explore_expanded {
            "Mostrar menos"
        } else {
            "Mostrar mais"
        };
        let toggle_icon = if self.ui.explore_expanded {
            YtSidebarIcon::ChevronUp
        } else {
            YtSidebarIcon::ChevronDown
        };
        if sidebar_pill_item(ui, toggle_icon, toggle_label, false, false, &palette).clicked() {
            self.ui.explore_expanded = !self.ui.explore_expanded;
        }
    }

    pub(crate) fn navigation_utilities_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Library,
            "Biblioteca",
            self.ui.browse_section == BrowseSection::Library && self.selected.is_none(),
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() {
                self.close_video(ctx);
            }
            self.ui.browse_section = BrowseSection::Library;
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let dl_active = self.ui.browse_section == BrowseSection::Downloads
            && !self.channels.view_open
            && self.selected.is_none();
        let active_dl = self.downloads.manager.active_count();
        let dl_label = if active_dl > 0 {
            format!("{} ({active_dl})", self.pick("Downloads", "Downloads"))
        } else {
            self.pick("Downloads", "Downloads").to_owned()
        };
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Downloads,
            &dl_label,
            dl_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
            self.ui.browse_section = BrowseSection::Downloads;
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let insights_active = self.ui.browse_section == BrowseSection::Insights
            && !self.channels.view_open
            && self.selected.is_none();
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Insights,
            self.pick("Insights", "Insights"),
            insights_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            self.close_channel_view();
            if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
            self.ui.browse_section = BrowseSection::Insights;
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Settings,
            self.pick("Configurações", "Settings"),
            self.ui.settings_open,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = !self.ui.settings_open;
            self.sync_player_window_visibility();
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Shortcuts,
            self.pick("Atalhos do teclado", "Keyboard shortcuts"),
            self.ui.shortcuts_open,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.shortcuts_open = !self.ui.shortcuts_open;
            self.close_navigation_drawer_if_overlay(ctx);
        }

        if self.auth.authenticated && self.feed.home.rx.is_none() {
            ui.add_space(2.0);
            if sidebar_pill_item(
                ui,
                YtSidebarIcon::SignOut,
                self.pick("Sair da conta", "Sign out"),
                false,
                false,
                &palette,
            )
            .clicked()
            {
                self.sign_out_and_reload_home(ctx);
                self.close_navigation_drawer_if_overlay(ctx);
            }
        }
    }

    pub(crate) fn navigation_footer_ui(&self, ui: &mut egui::Ui, palette: ThemePalette) {
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new(
                    "Sobre  Imprensa  Direitos autorais\nTermos  Privacidade  Seguran\u{00e7}a\n\u{00a9} 2026 FastTube",
                )
                .size(11.5)
                .color(palette.neutral_status_fg),
            );
        });
        ui.add_space(12.0);
    }

    pub(crate) fn navigation_now_playing_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        if let Some(video) = self.selected.clone() {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            let playing_button = egui::Button::new(
                RichText::new("\u{25b6}  Assistindo")
                    .strong()
                    .color(palette.playing_nav_fg),
            )
            .fill(palette.playing_nav_bg)
            .corner_radius(10);
            if ui
                .add_sized([ui.available_width(), 34.0], playing_button)
                .on_hover_text(&video.title)
                .clicked()
            {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.sync_player_window_visibility();
                self.close_navigation_drawer_if_overlay(ctx);
            }

            ui.add_space(4.0);
            let theater_label = if self.playback.theater {
                "\u{25ad}  Sair do teatro"
            } else {
                "\u{25ad}  Modo teatro"
            };
            if ui
                .add_sized(
                    [ui.available_width(), 30.0],
                    egui::Button::new(RichText::new(theater_label).small()).corner_radius(10),
                )
                .on_hover_text("Alternar modo teatro (T)")
                .clicked()
            {
                self.toggle_theater_mode();
            }

            ui.add_space(4.0);
            if ui
                .add_sized(
                    [ui.available_width(), 30.0],
                    egui::Button::new(RichText::new("\u{2715}  Fechar v\u{00ed}deo").small())
                        .corner_radius(10),
                )
                .on_hover_text("Encerrar reprodu\u{00e7}\u{00e3}o atual")
                .clicked()
            {
                self.close_video(ctx);
                self.ui.navigation_open = false;
            }
        }
    }

    pub(crate) fn compact_navigation_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let palette = self.palette();
        ui.set_min_width(COMPACT_NAVIGATION_WIDTH);
        ui.set_max_width(COMPACT_NAVIGATION_WIDTH);
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            let home_active = self.selected.is_none()
                && !self.ui.settings_open
                && !self.channels.view_open
                && self.ui.browse_section == BrowseSection::Home
                && !self.ui.search_results_mode
                && !matches!(self.feed.filter, FeedFilter::Shorts);
            if compact_sidebar_item(
                ui,
                YtSidebarIcon::Home,
                self.pick("Início", "Home"),
                home_active,
                &palette,
            )
            .clicked()
            {
                self.handle_header_home(ctx);
            }

            let shorts_active = self.ui.browse_section == BrowseSection::Shorts
                || matches!(self.feed.filter, FeedFilter::Shorts);
            if compact_sidebar_item(ui, YtSidebarIcon::Shorts, "Shorts", shorts_active, &palette)
                .clicked()
            {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Shorts;
                self.feed.filter = FeedFilter::Shorts;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
                    let _ = self.restore_cached_home_feed(ctx);
                }
                self.invalidate_feed_cache();
                self.ensure_feed_cache();
                if self.feed.cache.ranked.len() < 4
                    && self.feed.home.batch_rx.is_none()
                    && self.feed.home.rx.is_none()
                {
                    self.start_fast_home_feed(false, ctx);
                }
            }

            let subscriptions_active = self.selected.is_none()
                && !self.ui.settings_open
                && !self.channels.view_open
                && self.ui.browse_section == BrowseSection::Subscriptions;
            if compact_sidebar_item(
                ui,
                YtSidebarIcon::Subscriptions,
                self.pick("Inscrições", "Subscriptions"),
                subscriptions_active,
                &palette,
            )
            .clicked()
            {
                self.open_subscriptions_section(false, ctx);
            }

            let you_active = self.selected.is_none()
                && !self.ui.settings_open
                && matches!(
                    self.ui.browse_section,
                    BrowseSection::History | BrowseSection::Library | BrowseSection::Queue
                );
            if compact_sidebar_item(
                ui,
                YtSidebarIcon::You,
                self.pick("Você", "You"),
                you_active,
                &palette,
            )
            .clicked()
            {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Library;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
            }

            let dl_active = self.selected.is_none()
                && !self.ui.settings_open
                && self.ui.browse_section == BrowseSection::Downloads;
            if compact_sidebar_item(
                ui,
                YtSidebarIcon::Downloads,
                "Downloads",
                dl_active,
                &palette,
            )
            .clicked()
            {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Downloads;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
            }

            let ins_active = self.selected.is_none()
                && !self.ui.settings_open
                && self.ui.browse_section == BrowseSection::Insights;
            if compact_sidebar_item(
                ui,
                YtSidebarIcon::Insights,
                "Insights",
                ins_active,
                &palette,
            )
            .clicked()
            {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Insights;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
            }
        });
    }

    pub(crate) fn watch_navigation_drawer_ui(&mut self, ctx: &egui::Context) {
        let palette = self.palette();
        let content = ctx.input(|i| i.content_rect());
        let width = layout::compute_youtube_sidebar_width(content.width());
        let top = content.top();
        let height = content.height().max(120.0);
        let scrim_left = (content.left() + width).min(content.right());
        let scrim_width = (content.right() - scrim_left).max(0.0);
        if scrim_width > 0.0 {
            egui::Area::new(egui::Id::new("watch_navigation_drawer_scrim"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(scrim_left, top))
                .show(ctx, |ui| {
                    let (rect, response) = ui
                        .allocate_exact_size(egui::vec2(scrim_width, height), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        0.0,
                        Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                    );
                    if response.clicked() {
                        self.ui.navigation_open = false;
                    }
                });
        }
        let fill = if self.settings.theme.value == Theme::Dark {
            Color32::from_rgb(15, 15, 15)
        } else {
            Color32::from_rgb(255, 255, 255)
        };
        let stroke = if self.settings.theme.value == Theme::Dark {
            Color32::from_rgb(48, 48, 48)
        } else {
            Color32::from_rgb(222, 222, 222)
        };
        egui::Area::new(egui::Id::new("watch_navigation_drawer"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(content.left(), top))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, stroke))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(width, height));
                        ui.set_max_size(egui::vec2(width, height));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            self.header_menu_button_ui(ui);
                            ui.add_space(8.0);
                            if self.header_brand_ui(ui, palette) {
                                self.handle_header_home(ctx);
                                self.ui.navigation_open = false;
                            }
                        });
                        ui.add_space(2.0);
                        self.navigation_ui(ui, ctx);
                    });
            });
    }

    pub(crate) fn format_continue_watching_remaining(entry: &HistoryEntry) -> String {
        if let Some(dur_ms) = entry.duration_ms
            && dur_ms > entry.position_ms
        {
            let remaining_secs = (dur_ms - entry.position_ms) as f64 / 1000.0;
            return format!("Restam {}", format_time(remaining_secs));
        }
        let pos_ms = entry.resume_position_ms().unwrap_or(entry.position_ms);
        format!("Em {}", format_time(pos_ms as f64 / 1000.0))
    }

    pub(crate) fn continue_watching_shelf_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        let mut items: Vec<(HistoryEntry, Video)> = Vec::new();
        for entry in self.history.data.entries() {
            if entry.is_near_end() || entry.resume_position_ms().is_none() {
                continue;
            }
            let Some(mut video) = self.video_for_history_entry(entry) else {
                continue;
            };
            let meta = self.metadata_for(&video.url);
            if recommendations::is_short_video(&video, meta)
                || entry.title.to_lowercase().contains("#short")
            {
                continue;
            }
            if video.thumbnail.trim().is_empty() {
                video.thumbnail =
                    format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", entry.video_id);
            }
            items.push((entry.clone(), video));
            if items.len() >= 10 {
                break;
            }
        }
        if items.is_empty() {
            return None;
        }
        let palette = self.palette();
        let mut chosen = None;
        let mut completed = None;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Continuar assistindo")
                    .size(20.0)
                    .strong()
                    .color(palette.channel_fg),
            );
        });
        ui.add_space(10.0);
        let available = ui.available_width().max(220.0);
        let columns = compute_continue_watching_columns(available);
        let gap = 14.0;
        let card_width = ((available - gap * (columns.saturating_sub(1) as f32)) / columns as f32)
            .clamp(210.0, 290.0);

        egui::ScrollArea::horizontal()
            .id_salt("continue_watching_shelf_scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (entry, video) in &items {
                        ui.push_id(("continue", &entry.video_id), |ui| {
                            ui.vertical(|ui| {
                                ui.set_width(card_width);
                                ui.set_max_width(card_width);
                                let thumb_height = (card_width * 9.0 / 16.0).round();
                                let thumb = self.thumbnails.show_rounded(
                                    ui,
                                    ctx,
                                    &video.thumbnail,
                                    egui::vec2(card_width, thumb_height),
                                    12.0,
                                );
                                let progress = entry.duration_ms.map(|duration| {
                                    (entry.position_ms as f32 / duration.max(1) as f32)
                                        .clamp(0.04, 1.0)
                                });
                                paint_thumbnail_overlays(
                                    ui,
                                    thumb.rect,
                                    &video.duration,
                                    progress,
                                    false,
                                );
                                if thumb
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text("Retomar reprodução")
                                    .clicked()
                                {
                                    chosen = Some(video.clone());
                                }
                                ui.add_space(8.0);
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(truncate_text_chars(&entry.title, 58))
                                                .size(14.0)
                                                .strong()
                                                .color(palette.header_button_fg),
                                        )
                                        .frame(false)
                                        .wrap(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(&entry.title)
                                    .clicked()
                                {
                                    chosen = Some(video.clone());
                                }
                                if !entry.channel.is_empty() {
                                    ui.label(
                                        RichText::new(truncate_text_chars(&entry.channel, 36))
                                            .size(12.5)
                                            .color(palette.neutral_status_fg),
                                    );
                                }
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(Self::format_continue_watching_remaining(
                                            entry,
                                        ))
                                        .size(12.0)
                                        .color(palette.neutral_status_fg),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .small_button("Concluir")
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text("Marcar este vídeo como assistido")
                                                .clicked()
                                            {
                                                completed = Some(entry.clone());
                                            }
                                        },
                                    );
                                });
                            });
                        });
                    }
                });
            });
        ui.add_space(18.0);
        if let Some(mut entry) = completed {
            entry.position_ms = entry.duration_ms.unwrap_or(entry.position_ms.max(1));
            let _ = self.history.data.record(entry);
            self.history.dirty = true;
            self.save_history_to_disk(Instant::now());
            self.invalidate_feed_cache();
            self.toasts.success("Vídeo marcado como concluído.");
        }
        chosen
    }

    #[allow(dead_code)]
    pub(crate) fn continue_watching_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        self.thumbnails.begin_pass();
        let chosen = self.continue_watching_shelf_ui(ui, ctx);
        self.thumbnails.end_pass(ctx);
        chosen
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recommendation_card_contents_ui(
        thumbnails: &mut ThumbnailCache,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &ThemePalette,
        video: &Video,
        compact: bool,
        progress_ratio: Option<f32>,
        in_history: bool,
        is_music: bool,
        is_new: bool,
        title_truncated: &str,
        channel_label: Option<&str>,
        subline: Option<&str>,
    ) -> (bool, bool) {
        let mut clicked = false;
        let mut channel_clicked = false;
        let panel_width = ui.available_width().max(160.0);
        let thumb_width = if compact {
            (panel_width * 0.42).clamp(140.0, 168.0)
        } else {
            (panel_width * 0.36).clamp(168.0, 210.0)
        };
        let thumb_size = egui::vec2(thumb_width, (thumb_width * 9.0 / 16.0).round());

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let thumb = thumbnails.show_rounded(ui, ctx, &video.thumbnail, thumb_size, 8.0);
            paint_thumbnail_overlays(ui, thumb.rect, &video.duration, progress_ratio, is_music);
            if is_new {
                paint_thumbnail_novidade_badge(ui, thumb.rect);
            }
            if thumb.clicked() {
                clicked = true;
            }

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.set_max_width(ui.available_width().max(60.0));
                let title_button = egui::Button::new(
                    RichText::new(title_truncated)
                        .size(14.0)
                        .strong()
                        .color(palette.header_button_fg),
                )
                .frame(false)
                .wrap();
                if ui
                    .add(title_button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(&video.title)
                    .clicked()
                {
                    clicked = true;
                }

                if let Some(label) = channel_label {
                    let channel_button = egui::Button::new(
                        RichText::new(label)
                            .size(12.0)
                            .color(palette.neutral_status_fg),
                    )
                    .frame(false);
                    if ui
                        .add(channel_button)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Abrir canal")
                        .clicked()
                    {
                        channel_clicked = true;
                    }
                }

                if let Some(subline) = subline {
                    ui.label(
                        RichText::new(subline)
                            .size(12.0)
                            .color(palette.neutral_status_fg),
                    );
                }

                if in_history {
                    ui.label(
                        RichText::new("Continuar assistindo")
                            .size(11.5)
                            .color(palette.resume_status_fg),
                    );
                }
            });
        });

        (clicked, channel_clicked)
    }

    pub(crate) fn watch_recommendation_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        compact: bool,
    ) -> bool {
        let palette = self.palette();
        let metadata = self.metadata_for(&video.url);
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
        let is_new = recommendations::is_recent_upload(metadata, None);
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

        let (clicked, channel_clicked) = Self::recommendation_card_contents_ui(
            &mut self.thumbnails,
            ui,
            ctx,
            &palette,
            video,
            compact,
            progress_ratio,
            in_history,
            is_music,
            is_new,
            &title_truncated,
            channel_label.as_deref(),
            subline.as_deref(),
        );

        if channel_clicked && let Some(target) = channel_target {
            self.open_channel(target, ctx);
        }
        clicked
    }

    pub(crate) fn filter_chips_bar_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let palette = self.palette();
        self.ensure_feed_cache();
        let chips = self.feed.cache.chips.clone();
        let top_topics = self.feed.cache.profile.top_topics(6);
        let pinned_topics = self.settings.prefs.value.pinned_topics.clone();
        let mut selected_chip: Option<FeedFilter> = None;
        let mut clear_channel_filter = false;
        let mut toggle_pin_topic: Option<String> = None;

        let available_w = ui.available_width().max(160.0);
        let reserved_w = 32.0 * 3.0 + 8.0 + ui.spacing().item_spacing.x * 3.0;
        let chips_w = (available_w - reserved_w).max(80.0);
        let can_scroll_left = self.ui.chips_scroll_offset > 0.5;
        let mut prev_clicked = false;
        let mut next_clicked = false;
        let mut scroll_id_and_bounds = None;

        ui.horizontal(|ui| {
            let (prev_rect, prev_resp) =
                ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
            if can_scroll_left && prev_resp.hovered() {
                ui.painter()
                    .circle_filled(prev_rect.center(), 16.0, palette.header_button_bg);
            }
            let prev_fg = if can_scroll_left {
                palette.header_button_fg
            } else {
                palette.neutral_status_fg.gamma_multiply(0.4)
            };
            let pc = prev_rect.center();
            ui.painter().add(egui::Shape::line(
                vec![
                    egui::pos2(pc.x + 2.5, pc.y - 5.0),
                    egui::pos2(pc.x - 3.0, pc.y),
                    egui::pos2(pc.x + 2.5, pc.y + 5.0),
                ],
                egui::Stroke::new(1.6, prev_fg),
            ));
            let prev_resp = prev_resp
                .on_hover_cursor(if can_scroll_left {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text("Categorias anteriores");
            if prev_resp.clicked() && can_scroll_left {
                prev_clicked = true;
            }

            ui.add_space(4.0);
            let scroll_out = ui
                .allocate_ui_with_layout(
                    egui::vec2(chips_w, 34.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt("youtube_filter_chips")
                            .auto_shrink([false, true])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .horizontal_scroll_offset(self.ui.chips_scroll_offset)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);

                                    if let FeedFilter::Channel(channel_name) = &self.feed.filter {
                                        let chip_btn = egui::Button::new(
                                            RichText::new(format!("{channel_name}  \u{2715}"))
                                                .size(13.5)
                                                .strong()
                                                .color(palette.active_button_fg),
                                        )
                                        .fill(palette.active_button_bg)
                                        .corner_radius(8);
                                        if ui
                                            .add_sized([0.0, 32.0], chip_btn)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(
                                                "Remover filtro de canal e voltar para Tudo",
                                            )
                                            .clicked()
                                        {
                                            clear_channel_filter = true;
                                        }
                                    }

                                    for chip in &chips {
                                        let is_active = self.feed.filter == *chip
                                            && self.ui.browse_section != BrowseSection::History
                                            && self.ui.browse_section != BrowseSection::Queue;
                                        let (bg, fg) = if is_active {
                                            (palette.active_button_bg, palette.active_button_fg)
                                        } else {
                                            (palette.header_button_bg, palette.header_button_fg)
                                        };
                                        let topic_name = match chip {
                                            FeedFilter::Topic(t) => Some(t.as_str()),
                                            _ => None,
                                        };
                                        let is_pinned = topic_name
                                            .is_some_and(|topic| self.is_topic_pinned(topic));
                                        let label = if let Some(topic) = topic_name
                                            && is_pinned
                                        {
                                            format!("📌 {topic}")
                                        } else {
                                            chip.label()
                                        };
                                        let text = if is_active {
                                            RichText::new(&label).size(13.5).strong().color(fg)
                                        } else {
                                            RichText::new(&label).size(13.5).color(fg)
                                        };
                                        let btn = egui::Button::new(text).fill(bg).corner_radius(8);
                                        let resp = ui
                                            .add_sized([0.0, 32.0], btn)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if let Some(topic) = topic_name {
                                            let topic_owned = topic.to_owned();
                                            let hint = if is_pinned {
                                                "Clique direito para desafixar tópico"
                                            } else {
                                                "Clique direito para fixar tópico"
                                            };
                                            let resp = resp.on_hover_text(hint);
                                            resp.context_menu(|ui| {
                                                let action_label = if is_pinned {
                                                    "Desafixar tópico"
                                                } else {
                                                    "Fixar tópico"
                                                };
                                                if ui.button(action_label).clicked() {
                                                    toggle_pin_topic = Some(topic_owned.clone());
                                                    ui.close();
                                                }
                                            });
                                            if resp.clicked() {
                                                selected_chip = Some(chip.clone());
                                            }
                                        } else if resp.clicked() {
                                            selected_chip = Some(chip.clone());
                                        }
                                    }
                                });
                            })
                    },
                )
                .inner;

            let max_x = (scroll_out.content_size.x - scroll_out.inner_rect.width()).max(0.0);
            let current_x = scroll_out.state.offset.x.clamp(0.0, max_x);
            let can_scroll_right = current_x + 0.5 < max_x;

            ui.add_space(4.0);
            let pin_btn = egui::Button::new(
                RichText::new("📌")
                    .size(14.0)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .corner_radius(16);
            let pin_resp = ui
                .add_sized([32.0, 32.0], pin_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Fixar ou desafixar tópicos favoritos");
            egui::Popup::menu(&pin_resp).show(|ui| {
                ui.set_min_width(210.0);
                ui.label(
                    RichText::new("Tópicos nos filtros")
                        .size(12.5)
                        .strong()
                        .color(palette.neutral_status_fg),
                );
                ui.separator();
                let mut candidates: Vec<String> = Vec::new();
                for topic in &pinned_topics {
                    if !candidates
                        .iter()
                        .any(|c| c.eq_ignore_ascii_case(topic.trim()))
                    {
                        candidates.push(topic.clone());
                    }
                }
                for chip in &chips {
                    if let FeedFilter::Topic(topic) = chip
                        && !candidates
                            .iter()
                            .any(|c| c.eq_ignore_ascii_case(topic.trim()))
                    {
                        candidates.push(topic.clone());
                    }
                }
                for topic in &top_topics {
                    if !candidates
                        .iter()
                        .any(|c| c.eq_ignore_ascii_case(topic.trim()))
                    {
                        candidates.push(topic.clone());
                    }
                }
                if candidates.is_empty() {
                    ui.label(
                        RichText::new("Nenhum tópico detectado no feed ainda.")
                            .size(12.0)
                            .color(palette.neutral_status_fg),
                    );
                } else {
                    for topic in candidates {
                        let pinned = self.is_topic_pinned(&topic);
                        let label = if pinned {
                            format!("✓ 📌 {topic} (desafixar)")
                        } else {
                            format!("📌 Fixar {topic}")
                        };
                        if ui.button(label).clicked() {
                            toggle_pin_topic = Some(topic);
                            ui.close();
                        }
                    }
                }
            });

            let (next_rect, next_resp) =
                ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
            if can_scroll_right && next_resp.hovered() {
                ui.painter()
                    .circle_filled(next_rect.center(), 16.0, palette.header_button_bg);
            }
            let next_fg = if can_scroll_right {
                palette.header_button_fg
            } else {
                palette.neutral_status_fg.gamma_multiply(0.4)
            };
            let c = next_rect.center();
            ui.painter().add(egui::Shape::line(
                vec![
                    egui::pos2(c.x - 2.5, c.y - 5.0),
                    egui::pos2(c.x + 3.0, c.y),
                    egui::pos2(c.x - 2.5, c.y + 5.0),
                ],
                egui::Stroke::new(1.6, next_fg),
            ));
            let next_resp = next_resp
                .on_hover_cursor(if can_scroll_right {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text("Mais categorias");
            if next_resp.clicked() {
                next_clicked = true;
            }
            scroll_id_and_bounds = Some((scroll_out.id, scroll_out.state, current_x, max_x));
        });

        if let Some((scroll_id, mut state, current_x, max_x)) = scroll_id_and_bounds {
            let new_x = if prev_clicked {
                (current_x - 220.0).max(0.0)
            } else if next_clicked {
                (current_x + 220.0).min(max_x)
            } else {
                current_x
            };
            self.ui.chips_scroll_offset = new_x;
            if prev_clicked || next_clicked {
                state.offset.x = new_x;
                state.store(ctx, scroll_id);
            }
        }

        if clear_channel_filter {
            self.feed.filter = FeedFilter::All;
            self.ui.browse_section = BrowseSection::Home;
            self.invalidate_feed_cache();
        }
        if let Some(topic) = toggle_pin_topic {
            self.toggle_pinned_topic(&topic);
        }
        if let Some(chip) = selected_chip {
            self.ui.search_results_mode = false;
            self.ui.browse_section = if matches!(chip, FeedFilter::Shorts) {
                BrowseSection::Shorts
            } else {
                BrowseSection::Home
            };
            self.feed.filter = chip;
            if !self.restore_cached_home_feed(ctx) {
                self.feed.home.title = None;
            }
            self.invalidate_feed_cache();
            self.ensure_feed_cache();
            if self.feed.cache.ranked.len() < 6
                && self.feed.search.rx.is_none()
                && self.feed.home.rx.is_none()
                && self.feed.home.batch_rx.is_none()
            {
                self.start_fast_home_feed(false, ctx);
            }
        }
    }

    pub(crate) fn youtube_grid_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scored: &ScoredVideo,
        width: f32,
    ) -> HomeCardActions {
        let video = &scored.video;
        let palette = self.palette();
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let is_playlist = self.feed.home.playlist_urls.contains(&video.url)
            || video.url.contains("playlist?list=");
        let is_mix = is_playlist && video.title.to_lowercase().contains("mix");
        let (is_music, verified_by_views, subline, channel_target) = {
            let metadata = self.metadata_for(&video.url);
            (
                recommendations::is_music_video(video, metadata),
                metadata
                    .and_then(|m| m.view_count)
                    .is_some_and(|v| v >= 100_000),
                format_youtube_card_subline(&video.duration, metadata),
                self.channel_target_for_video(video),
            )
        };
        let progress_ratio = history::youtube_video_id(&video.url)
            .and_then(|id| self.history.data.get(&id))
            .and_then(|entry| {
                let dur = entry.duration_ms? as f32;
                if dur <= 0.0 || entry.position_ms == 0 {
                    None
                } else {
                    Some((entry.position_ms as f32 / dur).clamp(0.04, 1.0))
                }
            });
        let in_watch_later = self
            .library
            .data
            .watch_later
            .iter()
            .any(|v| v.video.url == video.url);
        let in_favorites = self
            .library
            .data
            .favorites
            .iter()
            .any(|v| v.video.url == video.url);
        let avatar_key = if is_playlist {
            String::new()
        } else {
            recommendations::normalize_channel_key(&video.channel)
        };

        let mut actions = HomeCardActions::default();

        ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let thumb_height = (width * 9.0 / 16.0).round();
            if is_playlist {
                let mut thumb_rect = egui::Rect::NOTHING;
                let mut thumb_clicked = false;
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (tabs_rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 9.0), egui::Sense::hover());
                    paint_mix_top_tabs(ui, tabs_rect, dark_theme);
                    let resp = self.thumbnails.show_rounded(
                        ui,
                        ctx,
                        &video.thumbnail,
                        egui::vec2(width, (thumb_height - 9.0).max(40.0)),
                        12.0,
                    );
                    thumb_rect = resp.rect;
                    thumb_clicked = resp.clicked();
                });
                paint_playlist_badge_overlay(
                    ui,
                    thumb_rect,
                    if is_mix { "Mix" } else { "Playlist" },
                );
                if thumb_clicked {
                    actions.open_clicked = true;
                }
            } else {
                let is_new_followed = self.followed_channels.new_video_urls.contains(&video.url);
                let thumb_resp = self.thumbnails.show_rounded(
                    ui,
                    ctx,
                    &video.thumbnail,
                    egui::vec2(width, thumb_height),
                    12.0,
                );
                paint_thumbnail_overlays(
                    ui,
                    thumb_resp.rect,
                    &video.duration,
                    progress_ratio,
                    is_music,
                );
                if is_new_followed {
                    paint_short_novo_badge(ui, thumb_resp.rect);
                }
                let pointer_pos = ui.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
                let quick_bar_rect = egui::Rect::from_min_max(
                    egui::pos2(
                        (thumb_resp.rect.right() - (thumb_resp.rect.width() - 12.0).max(0.0))
                            .max(thumb_resp.rect.left() + 6.0),
                        thumb_resp.rect.top() + 8.0,
                    ),
                    egui::pos2(
                        thumb_resp.rect.right() - 8.0,
                        (thumb_resp.rect.top() + 36.0).min(thumb_resp.rect.bottom() - 8.0),
                    ),
                );
                let thumb_hovered = thumb_resp.hovered()
                    || pointer_pos.is_some_and(|pos| thumb_resp.rect.contains(pos));
                let mut quick_action_interacted = false;
                if thumb_hovered
                    && quick_bar_rect.height() >= 22.0
                    && quick_bar_rect.width() >= 90.0
                {
                    let overlay_bg = Color32::from_rgba_unmultiplied(15, 15, 15, 224);
                    let active_bg = Color32::from_rgba_unmultiplied(62, 166, 255, 235);
                    let wl_text = if in_watch_later {
                        "\u{2713} Salvo"
                    } else if width >= 220.0 {
                        "Assistir depois"
                    } else {
                        "Depois"
                    };
                    let wl_tip = if in_watch_later {
                        "Remover de Assistir depois"
                    } else {
                        "Salvar em Assistir depois"
                    };
                    ui.scope_builder(egui::UiBuilder::new().max_rect(quick_bar_rect), |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let wl_btn = egui::Button::new(
                                RichText::new(wl_text).size(11.5).strong().color(
                                    if in_watch_later {
                                        Color32::BLACK
                                    } else {
                                        Color32::WHITE
                                    },
                                ),
                            )
                            .fill(if in_watch_later {
                                active_bg
                            } else {
                                overlay_bg
                            })
                            .corner_radius(6)
                            .min_size(egui::vec2(0.0, 24.0));
                            let wl_resp = ui
                                .add(wl_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(wl_tip);
                            if wl_resp.hovered() || wl_resp.is_pointer_button_down_on() {
                                quick_action_interacted = true;
                            }
                            if wl_resp.clicked() {
                                actions.toggle_watch_later = true;
                                quick_action_interacted = true;
                            }

                            let queue_btn = egui::Button::new(
                                RichText::new("+ Fila")
                                    .size(11.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(overlay_bg)
                            .corner_radius(6)
                            .min_size(egui::vec2(0.0, 24.0));
                            let queue_resp = ui
                                .add(queue_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("Adicionar \u{00e0} fila");
                            if queue_resp.hovered() || queue_resp.is_pointer_button_down_on() {
                                quick_action_interacted = true;
                            }
                            if queue_resp.clicked() {
                                actions.enqueue_clicked = true;
                                quick_action_interacted = true;
                            }
                        });
                    });
                }
                if thumb_resp.clicked() && !quick_action_interacted {
                    actions.open_clicked = true;
                }
            }

            ui.add_space(10.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if !is_playlist {
                    let avatar_label = if video.channel.trim().is_empty() {
                        "YT"
                    } else {
                        video.channel.as_str()
                    };
                    let avatar_url = (!avatar_key.is_empty())
                        .then(|| {
                            self.feed
                                .home
                                .channel_avatars
                                .get(&avatar_key)
                                .map(String::as_str)
                        })
                        .flatten();
                    let avatar_resp = Self::paint_avatar_with_cache(
                        &mut self.avatars,
                        ui,
                        ctx,
                        avatar_url,
                        avatar_label,
                        36.0,
                        true,
                    );
                    if avatar_resp.clicked() {
                        if let Some(target) = &channel_target {
                            actions.open_channel = Some(target.clone());
                        } else if !video.channel.trim().is_empty() {
                            actions.filter_channel = Some(video.channel.clone());
                        }
                    }
                }

                let text_col_w = if is_playlist {
                    (width - 32.0).max(80.0)
                } else {
                    (width - 78.0).max(80.0)
                };
                ui.vertical(|ui| {
                    ui.set_width(text_col_w);
                    ui.set_max_width(text_col_w);

                    let title_limit = if width < 220.0 {
                        44
                    } else if width < 280.0 {
                        58
                    } else {
                        72
                    };
                    let clean_title = truncate_text_chars(&video.title, title_limit);
                    let title_btn = egui::Button::new(
                        RichText::new(clean_title)
                            .size(15.5)
                            .strong()
                            .color(palette.channel_fg),
                    )
                    .frame(false)
                    .wrap();
                    if ui
                        .add(title_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(&video.title)
                        .clicked()
                    {
                        actions.open_clicked = true;
                    }

                    ui.add_space(3.0);
                    if is_playlist {
                        let subline = if video.channel.trim().is_empty() {
                            "Playlist".to_owned()
                        } else {
                            format!("{} \u{00b7} Playlist", video.channel.trim())
                        };
                        ui.label(
                            RichText::new(subline)
                                .size(13.0)
                                .color(palette.neutral_status_fg),
                        );
                    } else {
                        if !video.channel.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let ch_btn = egui::Button::new(
                                    RichText::new(truncate_text_chars(&video.channel, 34))
                                        .size(13.0)
                                        .color(palette.neutral_status_fg),
                                )
                                .frame(false);
                                if ui
                                    .add(ch_btn)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    if let Some(target) = &channel_target {
                                        actions.open_channel = Some(target.clone());
                                    } else {
                                        actions.filter_channel = Some(video.channel.clone());
                                    }
                                }
                                if channel_target.is_some() || verified_by_views {
                                    paint_verified_badge(ui, &palette);
                                }
                            });
                        }

                        if let Some(subline) = &subline {
                            ui.label(
                                RichText::new(subline)
                                    .size(13.0)
                                    .color(palette.neutral_status_fg),
                            );
                        }

                        if let Some(reason_label) = scored.primary_reason_label() {
                            ui.add_space(3.0);
                            let badge_resp = egui::Frame::new()
                                .fill(palette.card_alt_bg)
                                .stroke(egui::Stroke::new(1.0, palette.card_stroke))
                                .corner_radius(6)
                                .inner_margin(egui::Margin::symmetric(6, 2))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Por que este vídeo? · {reason_label}"
                                        ))
                                        .size(11.0)
                                        .color(palette.neutral_status_fg),
                                    );
                                })
                                .response;
                            badge_resp.on_hover_text(scored.explanation_tooltip());
                        }
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    let more_resp = paint_more_dots_button(ui, &palette);
                    egui::Popup::menu(&more_resp).show(|ui| {
                        ui.set_min_width(210.0);
                        if !is_playlist {
                            if ui.button("Adicionar \u{00e0} fila").clicked() {
                                actions.enqueue_clicked = true;
                                ui.close();
                            }
                            let wl_label = if in_watch_later {
                                "Remover de Assistir depois"
                            } else {
                                "Assistir mais tarde"
                            };
                            if ui.button(wl_label).clicked() {
                                actions.toggle_watch_later = true;
                                ui.close();
                            }
                            let fav_label = if in_favorites {
                                "Remover dos favoritos"
                            } else {
                                "Favoritar"
                            };
                            if ui.button(fav_label).clicked() {
                                actions.toggle_favorite = true;
                                ui.close();
                            }
                            ui.menu_button(
                                if self.ui_pt() {
                                    "⬇ Baixar"
                                } else {
                                    "⬇ Download"
                                },
                                |ui| {
                                    let pt = self.ui_pt();
                                    for fmt in downloads::DownloadFormat::ALL {
                                        let lbl = if pt { fmt.label_pt() } else { fmt.label_en() };
                                        if ui.button(lbl).clicked() {
                                            actions.download_format = Some(fmt);
                                            ui.close();
                                        }
                                    }
                                },
                            );
                        }
                        if let Some(target) = &channel_target {
                            if ui.button("Ir para o canal").clicked() {
                                actions.open_channel = Some(target.clone());
                                ui.close();
                            }
                        } else if !video.channel.trim().is_empty()
                            && ui.button("Filtrar por canal").clicked()
                        {
                            actions.filter_channel = Some(video.channel.clone());
                            ui.close();
                        }
                        if !video.channel.trim().is_empty()
                            && ui.button("Menos deste canal").clicked()
                        {
                            actions.mute_channel = Some(video.channel.clone());
                            ui.close();
                        }
                        if let Some(topic_hint) = scored.primary_topic_hint()
                            && ui
                                .button(format!("Silenciar tópico: {topic_hint}"))
                                .clicked()
                        {
                            actions.mute_topic = Some(topic_hint);
                            ui.close();
                        }
                        if ui.button("Copiar link").clicked() {
                            actions.copy_url = Some(video.url.clone());
                            ui.close();
                        }
                    });
                });
            });
        });

        actions
    }

    pub(crate) fn shorts_shelf_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        ranked: &[ScoredVideo],
        short_indices: &[usize],
        available_width: f32,
    ) -> (Option<Video>, Option<Video>, Option<String>, bool) {
        if short_indices.is_empty()
            || self.feed.home.shorts_shelf_hidden
            || self.settings.prefs.value.hide_shorts_in_home
        {
            return (None, None, None, false);
        }
        let palette = self.palette();
        let mut selected = None;
        let mut enqueue = None;
        let mut copy_url = None;
        let mut hide_shelf = false;

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            paint_shorts_badge_icon(ui);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Shorts")
                    .size(20.0)
                    .strong()
                    .color(palette.channel_fg),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let shelf_more = paint_more_dots_button(ui, &palette);
                egui::Popup::menu(&shelf_more).show(|ui| {
                    if ui.button("Ocultar esta estante").clicked() {
                        hide_shelf = true;
                        ui.close();
                    }
                });
            });
        });
        ui.add_space(10.0);

        let columns = ((available_width + 14.0) / 195.0).floor().clamp(2.0, 5.0) as usize;
        let card_w = ((available_width - 14.0 * (columns.saturating_sub(1) as f32))
            / columns as f32)
            .max(130.0);
        let card_h = (card_w * 16.0 / 9.0).round();

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            for &idx in short_indices.iter().take(columns) {
                let video = &ranked[idx].video;
                let (is_recent, view_count) = {
                    let meta = self.metadata_for(&video.url);
                    (
                        recommendations::is_recent_upload(meta, None),
                        meta.and_then(|m| m.view_count),
                    )
                };
                ui.push_id(("short_shelf", &video.url), |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        ui.set_max_width(card_w);
                        let thumb = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            &video.thumbnail,
                            egui::vec2(card_w, card_h),
                            12.0,
                        );
                        if is_recent {
                            paint_short_novo_badge(ui, thumb.rect);
                        }
                        if thumb.clicked() {
                            selected = Some(video.clone());
                        }
                        ui.add_space(8.0);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let title_w = (card_w - 28.0).max(60.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(title_w, 0.0),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    let short_title = truncate_text_chars(&video.title, 48);
                                    let title_resp = ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(short_title)
                                                    .size(14.0)
                                                    .strong()
                                                    .color(palette.channel_fg),
                                            )
                                            .frame(false)
                                            .wrap(),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&video.title);
                                    if title_resp.clicked() {
                                        selected = Some(video.clone());
                                    }
                                },
                            );
                            let more_resp = paint_more_dots_button(ui, &palette);
                            egui::Popup::menu(&more_resp).show(|ui| {
                                if ui.button("Adicionar \u{00e0} fila").clicked() {
                                    enqueue = Some(video.clone());
                                    ui.close();
                                }
                                if ui.button("Copiar link").clicked() {
                                    copy_url = Some(video.url.clone());
                                    ui.close();
                                }
                            });
                        });
                        if let Some(views) = view_count {
                            ui.label(
                                RichText::new(recommendations::format_youtube_compact_views(views))
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            );
                        } else if !video.channel.is_empty() {
                            ui.label(
                                RichText::new(&video.channel)
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            );
                        }
                    });
                });
            }
        });
        ui.add_space(20.0);
        (selected, enqueue, copy_url, hide_shelf)
    }

    pub(crate) fn subscription_feed_filter_bar_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            changed |= ui
                .checkbox(
                    &mut self.feed_preferences.values.hide_watched,
                    "Ocultar assistidos",
                )
                .changed();
            changed |= ui
                .checkbox(
                    &mut self.feed_preferences.values.hide_shorts,
                    "Ocultar Shorts",
                )
                .changed();
            changed |= ui
                .checkbox(
                    &mut self.feed_preferences.values.hide_lives,
                    "Ocultar lives",
                )
                .changed();
            changed |= ui
                .checkbox(
                    &mut self.feed_preferences.values.newest_first,
                    "Mais recentes",
                )
                .changed();
            let channels: Vec<String> = self
                .feed
                .videos
                .iter()
                .map(|video| video.channel.clone())
                .filter(|name| !name.is_empty())
                .collect();
            egui::ComboBox::from_id_salt("subscription_channel_filter")
                .selected_text(
                    self.feed_preferences
                        .values
                        .channel
                        .as_deref()
                        .unwrap_or("Todos os canais"),
                )
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.feed_preferences.values.channel.is_none(),
                            "Todos os canais",
                        )
                        .clicked()
                    {
                        self.feed_preferences.values.channel = None;
                        changed = true;
                    }
                    for channel in channels {
                        if ui
                            .selectable_label(
                                self.feed_preferences.values.channel.as_deref() == Some(&channel),
                                &channel,
                            )
                            .clicked()
                        {
                            self.feed_preferences.values.channel = Some(channel);
                            changed = true;
                        }
                    }
                });
        });
        changed
    }

    pub(crate) fn apply_subscription_feed_filters(
        &mut self,
        ui: &mut egui::Ui,
        ranked: &[ScoredVideo],
        indices: &mut Vec<usize>,
    ) {
        let changed = self.subscription_feed_filter_bar_ui(ui);
        if changed && let Some(path) = &self.feed_preferences.path {
            let pref = feed_prefs::FeedPreferences {
                hide_watched: self.feed_preferences.values.hide_watched,
                hide_shorts: self.feed_preferences.values.hide_shorts,
                hide_lives: self.feed_preferences.values.hide_lives,
                newest_first: self.feed_preferences.values.newest_first,
                channel: self.feed_preferences.values.channel.clone(),
            };
            if let Err(error) = feed_prefs::save(path, &pref) {
                self.message = format!("Filtros não salvos: {error}");
            }
        }
        let filters = &self.feed_preferences.values;
        indices.retain(|&idx| {
            let item = &ranked[idx];
            let viewed = history::youtube_video_id(&item.video.url)
                .and_then(|id| self.history.data.get(&id))
                .is_some_and(|entry| entry.is_near_end());
            let short = recommendations::is_short_video(&item.video, Some(&item.metadata));
            let live = recommendations::is_live_video(&item.video, Some(&item.metadata));
            filters.include(viewed, short, live, &item.video.channel)
        });
        if self.feed_preferences.values.newest_first {
            indices.sort_by_key(|&idx| {
                std::cmp::Reverse(ranked[idx].metadata.upload_date.as_deref().unwrap_or(""))
            });
        }
    }

    pub(crate) fn partition_home_feed_indices(
        &self,
        ranked: &[ScoredVideo],
        indices: &[usize],
    ) -> (Vec<usize>, Vec<usize>) {
        let show_shorts_only = matches!(self.feed.filter, FeedFilter::Shorts)
            || self.ui.browse_section == BrowseSection::Shorts;
        if show_shorts_only {
            return (indices.to_vec(), Vec::new());
        }
        let mut shorts_list = Vec::new();
        let mut regular_list = Vec::with_capacity(indices.len());
        for &idx in indices {
            if ranked[idx].is_short {
                shorts_list.push(idx);
            } else {
                regular_list.push(idx);
            }
        }
        if regular_list.is_empty() {
            (shorts_list, Vec::new())
        } else {
            (regular_list, shorts_list)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_home_grid_actions(
        &mut self,
        opened_short: bool,
        enqueue: Option<Video>,
        download_req: Option<(Video, downloads::DownloadFormat)>,
        toggle_watch_later: Option<Video>,
        toggle_favorite: Option<Video>,
        copy_url: Option<String>,
        playlist: Option<String>,
        open_channel: Option<ChannelTarget>,
        filter_channel: Option<String>,
        mute_channel: Option<String>,
        mute_topic: Option<String>,
        hide_shorts_shelf: bool,
        ctx: &egui::Context,
    ) {
        if hide_shorts_shelf {
            self.feed.home.shorts_shelf_hidden = true;
        }
        if opened_short {
            self.ui.browse_section = BrowseSection::Shorts;
            self.feed.filter = FeedFilter::Shorts;
        }
        if let Some(video) = enqueue {
            let _ = self.enqueue_video(video);
        }
        if let Some((video, fmt)) = download_req {
            self.start_download_video(video, fmt, ctx);
        }
        if let Some(video) = toggle_watch_later {
            let prev_len = self.library.data.watch_later.len();
            self.library.data.toggle_watch_later(video);
            self.save_library();
            let curr_len = self.library.data.watch_later.len();
            if curr_len != prev_len {
                self.toasts.success(if curr_len > prev_len {
                    "Salvo em \"Assistir mais tarde\"."
                } else {
                    "Removido de \"Assistir mais tarde\"."
                });
            }
        }
        if let Some(video) = toggle_favorite {
            let prev_len = self.library.data.favorites.len();
            self.library.data.toggle_favorite(video);
            self.save_library();
            let curr_len = self.library.data.favorites.len();
            if curr_len != prev_len {
                self.toasts.success(if curr_len > prev_len {
                    "Adicionado aos favoritos."
                } else {
                    "Removido dos favoritos."
                });
            }
        }
        if let Some(url) = copy_url {
            match copy_text_to_clipboard(ctx, &url) {
                Ok(()) => {
                    let msg = "Link copiado para a \u{00e1}rea de transfer\u{00ea}ncia.";
                    self.message = msg.into();
                    self.toasts.success(msg);
                }
                Err(error) => {
                    self.message = error.clone();
                    self.toasts.error(error);
                }
            }
        }
        if let Some(target) = playlist {
            if let Some(list_id) = crate::extract_playlist_id_from_url(&target) {
                self.start_remote_playlist_fetch(&list_id, &target, ctx);
            } else {
                self.start_browser_feed(target, false, ctx);
            }
        }
        if let Some(target) = open_channel {
            self.open_channel(target, ctx);
        } else if let Some(ch_name) = filter_channel {
            self.feed.filter = FeedFilter::Channel(ch_name);
            self.invalidate_feed_cache();
        }
        if let Some(channel) = mute_channel {
            self.mute_channel_preference(&channel);
        }
        if let Some(topic) = mute_topic {
            self.mute_topic_preference(&topic);
        }
    }

    pub(crate) fn home_grid_metrics(&self, available: f32) -> (f32, usize) {
        if self.settings.prefs.value.compact_card_density {
            let gap = 12.0;
            let columns = if available < layout::SINGLE_COLUMN_BROWSE_MAX_WIDTH {
                1
            } else {
                (((available + gap) / 250.0).floor() as usize).clamp(2, 6)
            };
            (gap, columns)
        } else {
            (16.0, compute_home_grid_columns(available))
        }
    }

    pub(crate) fn home_grid_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        self.ensure_feed_cache();
        let ranked = std::mem::take(&mut self.feed.cache.ranked);
        let is_subscription_feed = self
            .feed
            .home
            .title
            .as_deref()
            .is_some_and(|title| title.contains("envios recentes"));
        let mut visible_indices: Vec<usize> = (0..ranked.len()).collect();
        if is_subscription_feed {
            self.apply_subscription_feed_filters(ui, &ranked, &mut visible_indices);
        }

        let (main_indices, short_indices) =
            self.partition_home_feed_indices(&ranked, &visible_indices);
        let has_main_videos = !main_indices.is_empty();
        let show_continue_shelf = self.ui.browse_section == BrowseSection::Home
            && !self.ui.search_results_mode
            && self.feed.home.title.is_none()
            && matches!(self.feed.filter, FeedFilter::All | FeedFilter::Recommended);

        let mut selected = None;
        let mut opened_short = false;
        let mut playlist = None;
        let mut enqueue = None;
        let mut download_req: Option<(Video, downloads::DownloadFormat)> = None;
        let mut toggle_watch_later = None;
        let mut toggle_favorite = None;
        let mut copy_url = None;
        let mut open_channel = None;
        let mut filter_channel = None;
        let mut mute_channel = None;
        let mut mute_topic = None;
        let mut hide_shorts_shelf = false;

        self.thumbnails.begin_pass();
        let scroll_out = egui::ScrollArea::vertical()
            .id_salt("home_grid")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if show_continue_shelf
                    && let Some(resume_video) = self.continue_watching_shelf_ui(ui, ctx)
                {
                    selected = Some(resume_video);
                }

                let available = ui.available_width().max(220.0);
                let (gap, columns) = self.home_grid_metrics(available);
                let width = ((available - gap * (columns.saturating_sub(1) as f32))
                    / columns as f32)
                    .max(160.0);
                let row_height = (width * 9.0 / 16.0).round() + 10.0 + 118.0;
                let expanded_clip = ui.clip_rect().expand2(egui::vec2(0.0, 450.0));

                for (row_index, row) in main_indices.chunks(columns).enumerate() {
                    let cursor = ui.cursor();
                    let row_top = cursor.min.y;
                    let row_rect = egui::Rect::from_min_size(
                        egui::pos2(cursor.min.x, row_top),
                        egui::vec2(available, row_height),
                    );
                    if row_index > 0 && !row_rect.intersects(expanded_clip) {
                        ui.allocate_exact_size(
                            egui::vec2(available, row_height + 24.0),
                            egui::Sense::hover(),
                        );
                        continue;
                    }
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for &idx in row {
                            let scored = &ranked[idx];
                            let video = &scored.video;
                            ui.push_id(&video.url, |ui| {
                                let is_playlist = self.feed.home.playlist_urls.contains(&video.url)
                                    || video.url.contains("playlist?list=");
                                let card_actions =
                                    self.youtube_grid_card_ui(ui, ctx, scored, width);
                                if card_actions.open_clicked {
                                    if is_playlist {
                                        playlist = Some(video.url.clone());
                                    } else {
                                        selected = Some(video.clone());
                                    }
                                }
                                if card_actions.enqueue_clicked {
                                    enqueue = Some(video.clone());
                                }
                                if let Some(fmt) = card_actions.download_format {
                                    download_req = Some((video.clone(), fmt));
                                }
                                if card_actions.toggle_watch_later {
                                    toggle_watch_later = Some(video.clone());
                                }
                                if card_actions.toggle_favorite {
                                    toggle_favorite = Some(video.clone());
                                }
                                if card_actions.copy_url.is_some() {
                                    copy_url = card_actions.copy_url;
                                }
                                if card_actions.open_channel.is_some() {
                                    open_channel = card_actions.open_channel;
                                }
                                if card_actions.filter_channel.is_some() {
                                    filter_channel = card_actions.filter_channel;
                                }
                                if card_actions.mute_channel.is_some() {
                                    mute_channel = card_actions.mute_channel;
                                }
                                if card_actions.mute_topic.is_some() {
                                    mute_topic = card_actions.mute_topic;
                                }
                            });
                        }
                    });
                    ui.add_space(24.0);

                    if row_index == 0 && !short_indices.is_empty() {
                        let (short_sel, short_enq, short_copy, hide_shelf) =
                            self.shorts_shelf_ui(ui, ctx, &ranked, &short_indices, available);
                        if short_sel.is_some() {
                            opened_short = true;
                            selected = short_sel;
                        }
                        if short_enq.is_some() {
                            enqueue = short_enq;
                        }
                        if short_copy.is_some() {
                            copy_url = short_copy;
                        }
                        hide_shorts_shelf |= hide_shelf;
                    }
                }

                if main_indices.is_empty() && !short_indices.is_empty() {
                    let (short_sel, short_enq, short_copy, hide_shelf) =
                        self.shorts_shelf_ui(ui, ctx, &ranked, &short_indices, available);
                    if short_sel.is_some() {
                        opened_short = true;
                        selected = short_sel;
                    }
                    if short_enq.is_some() {
                        enqueue = short_enq;
                    }
                    if short_copy.is_some() {
                        copy_url = short_copy;
                    }
                    hide_shorts_shelf |= hide_shelf;
                }

                if self.feed.home.batch_rx.is_some()
                    && (self.ui.browse_section == BrowseSection::Home
                        || self.ui.browse_section == BrowseSection::Shorts)
                    && !self.ui.search_results_mode
                    && self.feed.home.title.is_none()
                {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space((available * 0.5 - 12.0).max(0.0));
                        ui.spinner();
                    });
                    ui.add_space(16.0);
                }
            });
        self.feed.cache.ranked = ranked;
        self.thumbnails.end_pass(ctx);

        let in_fast_home = (self.ui.browse_section == BrowseSection::Home
            || self.ui.browse_section == BrowseSection::Shorts)
            && !self.ui.search_results_mode
            && self.feed.home.title.is_none();
        if in_fast_home
            && has_main_videos
            && self.feed.home.has_more
            && self.feed.home.batch_rx.is_none()
            && self.feed.home.rx.is_none()
            && self.feed.search.rx.is_none()
        {
            let max_scroll = (scroll_out.content_size.y - scroll_out.inner_rect.height()).max(0.0);
            if scroll_out.content_size.y > 100.0
                && (scroll_out.content_size.y <= scroll_out.inner_rect.height() + 260.0
                    || scroll_out.state.offset.y >= (max_scroll - 260.0).max(0.0))
            {
                self.start_fast_home_feed(true, ctx);
            }
        }

        if let Some(video) = selected.as_ref() {
            let cid = self.channel_target_for_video(video).map(|t| match t {
                ChannelTarget::ChannelId(id) => id,
                ChannelTarget::Handle(h) => h,
            });
            self.mark_followed_video_seen(cid.as_deref(), &video.url);
        }

        self.apply_home_grid_actions(
            opened_short,
            enqueue,
            download_req,
            toggle_watch_later,
            toggle_favorite,
            copy_url,
            playlist,
            open_channel,
            filter_channel,
            mute_channel,
            mute_topic,
            hide_shorts_shelf,
            ctx,
        );
        selected
    }

    pub(crate) fn is_video_unwatched(&self, url: &str) -> bool {
        let Some(id) = history::youtube_video_id(url) else {
            return true;
        };
        match self.history.data.get(&id) {
            Some(entry) => entry.position_ms == 0 && !entry.is_near_end(),
            None => true,
        }
    }

    pub(crate) fn youtube_search_result_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
    ) -> SearchCardAction {
        let palette = self.palette();
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let metadata = self.metadata_for(&video.url).cloned();
        let kind = metadata.as_ref().map(|m| m.kind).unwrap_or_default();
        let is_channel = kind == search::SearchResultKind::Channel;
        let is_playlist = kind == search::SearchResultKind::Playlist
            || self.feed.home.playlist_urls.contains(&video.url)
            || video.url.contains("playlist?list=");
        let is_short = kind == search::SearchResultKind::Short || video.url.contains("/shorts/");
        let is_live = !is_channel
            && !is_playlist
            && !is_short
            && metadata.as_ref().is_some_and(|m| {
                m.is_live
                    || m.badges.iter().any(|b| {
                        b.eq_ignore_ascii_case("ao vivo") || b.eq_ignore_ascii_case("live")
                    })
            });
        let is_music = !is_channel
            && !is_playlist
            && recommendations::is_music_video(video, metadata.as_ref());

        let channel_target = self.channel_target_for_video(video);
        let progress_ratio = if is_channel || is_playlist {
            None
        } else {
            self.youtube_search_progress_ratio(&video.url)
        };
        let description = description_snippet(
            metadata
                .as_ref()
                .and_then(|meta| meta.description.as_deref()),
            175,
        );
        let stats_line =
            format_search_card_subline(video, metadata.as_ref(), is_channel, is_playlist, is_live);
        let avatar_url = metadata
            .as_ref()
            .and_then(|m| m.channel_avatar_url.as_deref())
            .filter(|u| !u.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| {
                self.lookup_channel_avatar(&video.channel)
                    .map(str::to_owned)
            });

        let vid = history::youtube_video_id(&video.url);
        let in_watch_later = self.library.data.watch_later.iter().any(|v| {
            vid.as_deref()
                .is_some_and(|id| history::youtube_video_id(&v.video.url).as_deref() == Some(id))
                || v.video.url == video.url
        });

        let panel_width = ui.available_width().max(240.0);
        let wide = panel_width >= 660.0;
        let thumb_width = if wide {
            (panel_width * 0.42).clamp(300.0, 500.0)
        } else {
            panel_width
        };
        let thumb_size = egui::vec2(thumb_width, (thumb_width * 9.0 / 16.0).round());

        let mut action = SearchCardAction::default();
        let open_clicked = Cell::new(false);

        let paint_thumb_col = |app: &mut Self, ui: &mut egui::Ui| {
            if is_channel {
                let (col_rect, col_resp) = ui.allocate_exact_size(thumb_size, egui::Sense::click());
                let avatar_diam = (thumb_size.y * 0.72).clamp(84.0, 136.0);
                let avatar_rect = egui::Rect::from_center_size(
                    col_rect.center(),
                    egui::vec2(avatar_diam, avatar_diam),
                );
                let painted = avatar_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|uri| !uri.is_empty())
                    .is_some_and(|uri| app.avatars.try_paint_circle(ui, ctx, avatar_rect, uri));
                if !painted {
                    let label = if video.title.trim().is_empty() {
                        "YT"
                    } else {
                        video.title.as_str()
                    };
                    let [r, g, b] = recommendations::channel_avatar_rgb(label);
                    let initials = recommendations::channel_avatar_initials(label);
                    ui.painter().circle_filled(
                        avatar_rect.center(),
                        avatar_diam * 0.5,
                        Color32::from_rgb(r, g, b),
                    );
                    ui.painter().text(
                        avatar_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        initials,
                        egui::FontId::proportional((avatar_diam * 0.36).clamp(16.0, 44.0)),
                        Color32::WHITE,
                    );
                }
                if col_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    open_clicked.set(true);
                }
            } else if is_playlist {
                let mut thumb_rect = egui::Rect::NOTHING;
                let mut thumb_clicked = false;
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (tabs_rect, _) =
                        ui.allocate_exact_size(egui::vec2(thumb_width, 9.0), egui::Sense::hover());
                    paint_mix_top_tabs(ui, tabs_rect, dark_theme);
                    let resp = app.thumbnails.show_rounded(
                        ui,
                        ctx,
                        &video.thumbnail,
                        egui::vec2(thumb_width, (thumb_size.y - 9.0).max(40.0)),
                        12.0,
                    );
                    thumb_rect = resp.rect;
                    thumb_clicked = resp.clicked();
                });
                let count_label = metadata
                    .as_ref()
                    .and_then(|m| m.playlist_count.as_deref())
                    .unwrap_or("Playlist");
                paint_playlist_badge_overlay(ui, thumb_rect, count_label);
                if thumb_clicked {
                    open_clicked.set(true);
                }
            } else {
                let thumb =
                    app.thumbnails
                        .show_rounded(ui, ctx, &video.thumbnail, thumb_size, 12.0);
                let overlay_dur = if is_live { "" } else { video.duration.as_str() };
                paint_thumbnail_overlays(ui, thumb.rect, overlay_dur, progress_ratio, is_music);
                if is_short {
                    paint_short_tag_overlay(ui, thumb.rect);
                }
                if thumb.clicked() {
                    open_clicked.set(true);
                }
            }
        };

        let mut show_details = |app: &mut Self, ui: &mut egui::Ui| {
            let details_w = ui.available_width().max(120.0);
            ui.vertical(|ui| {
                ui.set_width(details_w);
                ui.set_max_width(details_w);
                ui.spacing_mut().item_spacing.y = 0.0;

                ui.horizontal_top(|ui| {
                    let title_width = (details_w - 34.0).max(80.0);
                    ui.vertical(|ui| {
                        ui.set_width(title_width);
                        ui.set_max_width(title_width);
                        ui.spacing_mut().item_spacing.y = 4.0;

                        let title_btn = egui::Button::new(
                            RichText::new(truncate_text_chars(&video.title, 110))
                                .size(if wide { 18.5 } else { 16.5 })
                                .color(palette.header_button_fg),
                        )
                        .frame(false)
                        .wrap();
                        if ui
                            .add(title_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&video.title)
                            .clicked()
                        {
                            open_clicked.set(true);
                        }

                        if let Some(line) = &stats_line {
                            ui.label(
                                RichText::new(line)
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            );
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        let more_resp = paint_more_dots_button(ui, &palette);
                        egui::Popup::menu(&more_resp).show(|ui| {
                            ui.set_min_width(236.0);
                            ui.spacing_mut().item_spacing.y = 2.0;

                            if !is_channel && !is_playlist {
                                if search_menu_item_ui(
                                    ui,
                                    SearchMenuIcon::Queue,
                                    "Adicionar \u{00e0} fila",
                                    &palette,
                                )
                                .clicked()
                                {
                                    action.enqueue_clicked = true;
                                    ui.close();
                                }

                                let wl_label = if in_watch_later {
                                    "Remover de \"Assistir mais tarde\""
                                } else {
                                    "Salvar em \"Assistir mais tarde\""
                                };
                                if search_menu_item_ui(
                                    ui,
                                    SearchMenuIcon::Clock,
                                    wl_label,
                                    &palette,
                                )
                                .clicked()
                                {
                                    action.toggle_watch_later = true;
                                    ui.close();
                                }

                                if search_menu_item_ui(
                                    ui,
                                    SearchMenuIcon::Bookmark,
                                    "Salvar na playlist",
                                    &palette,
                                )
                                .clicked()
                                {
                                    action.open_save_modal = true;
                                    ui.close();
                                }

                                let pt = app.ui_pt();
                                ui.menu_button(
                                    if pt {
                                        "\u{2b07} Baixar"
                                    } else {
                                        "\u{2b07} Download"
                                    },
                                    |ui| {
                                        for fmt in downloads::DownloadFormat::ALL {
                                            let lbl =
                                                if pt { fmt.label_pt() } else { fmt.label_en() };
                                            if ui.button(lbl).clicked() {
                                                action.download_format = Some(fmt);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                            } else {
                                let open_label = if is_playlist {
                                    "Abrir playlist"
                                } else {
                                    "Abrir canal"
                                };
                                if search_menu_item_ui(
                                    ui,
                                    SearchMenuIcon::Queue,
                                    open_label,
                                    &palette,
                                )
                                .clicked()
                                {
                                    open_clicked.set(true);
                                    ui.close();
                                }
                            }

                            if search_menu_item_ui(
                                ui,
                                SearchMenuIcon::Share,
                                "Compartilhar",
                                &palette,
                            )
                            .clicked()
                            {
                                action.share_clicked = true;
                                ui.close();
                            }

                            ui.separator();

                            if search_menu_item_ui(ui, SearchMenuIcon::Flag, "Denunciar", &palette)
                                .clicked()
                            {
                                action.open_report_modal = true;
                                ui.close();
                            }
                        });
                    });
                });

                if !is_channel {
                    let channel_label = if video.channel.trim().is_empty() {
                        channel_target
                            .as_ref()
                            .map(channel_target_display_label)
                            .unwrap_or("Canal")
                    } else {
                        video.channel.as_str()
                    };
                    if !channel_label.trim().is_empty() {
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if app
                                .paint_avatar_or_initials(
                                    ui,
                                    ctx,
                                    avatar_url.as_deref(),
                                    channel_label,
                                    24.0,
                                    true,
                                )
                                .clicked()
                            {
                                if let Some(target) = &channel_target {
                                    action.open_channel = Some(target.clone());
                                } else if !video.channel.trim().is_empty() {
                                    action.filter_channel = Some(video.channel.clone());
                                }
                            }

                            let ch_btn = egui::Button::new(
                                RichText::new(truncate_text_chars(channel_label, 42))
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            )
                            .frame(false);
                            if ui
                                .add(ch_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                if let Some(target) = &channel_target {
                                    action.open_channel = Some(target.clone());
                                } else if !video.channel.trim().is_empty() {
                                    action.filter_channel = Some(video.channel.clone());
                                }
                            }

                            let is_verified = metadata.as_ref().is_some_and(|m| {
                                m.channel_verified || m.view_count.is_some_and(|v| v >= 100_000)
                            });
                            if is_verified {
                                paint_verified_badge(ui, &palette);
                            }
                        });
                    }
                } else if metadata.as_ref().is_some_and(|m| m.channel_verified) {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        paint_verified_badge(ui, &palette);
                        ui.label(
                            RichText::new("Canal verificado")
                                .size(12.0)
                                .color(palette.neutral_status_fg),
                        );
                    });
                }

                if let Some(text) = description.as_ref() {
                    ui.add_space(10.0);
                    ui.label(RichText::new(text).size(12.5).color(palette.snippet_fg));
                }

                let extra_badges: Vec<&str> = metadata
                    .as_ref()
                    .map(|m| {
                        m.badges
                            .iter()
                            .map(String::as_str)
                            .filter(|b| {
                                !(is_live
                                    && (b.eq_ignore_ascii_case("ao vivo")
                                        || b.eq_ignore_ascii_case("live")))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let show_new_fallback = extra_badges.is_empty()
                    && !is_live
                    && !is_channel
                    && !is_playlist
                    && recommendations::is_recent_upload(metadata.as_ref(), None);

                if is_live || !extra_badges.is_empty() || show_new_fallback {
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                        if is_live {
                            paint_live_now_badge(ui);
                        }
                        for badge in &extra_badges {
                            paint_search_metadata_badge(ui, badge, &palette);
                        }
                        if show_new_fallback {
                            paint_search_metadata_badge(ui, "Novo", &palette);
                        }
                    });
                }

                if is_playlist {
                    ui.add_space(8.0);
                    let view_all = egui::Button::new(
                        RichText::new("Ver playlist completa")
                            .size(12.5)
                            .strong()
                            .color(palette.neutral_status_fg),
                    )
                    .frame(false);
                    if ui
                        .add(view_all)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        open_clicked.set(true);
                    }
                }
            });
        };

        if wide {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                paint_thumb_col(self, ui);
                show_details(self, ui);
            });
        } else {
            paint_thumb_col(self, ui);
            ui.add_space(10.0);
            show_details(self, ui);
        }

        if open_clicked.get() {
            if is_channel {
                if let Some(target) = channel_target {
                    action.open_channel = Some(target);
                }
            } else {
                action.open_clicked = true;
            }
        }

        action
    }

    pub(crate) fn youtube_search_progress_ratio(&self, url: &str) -> Option<f32> {
        let history_entry =
            history::youtube_video_id(url).and_then(|id| self.history.data.get(&id).cloned());
        history_entry.as_ref().and_then(|entry| {
            let duration_ms = entry.duration_ms? as f32;
            if duration_ms <= 0.0 || entry.position_ms == 0 {
                None
            } else {
                Some((entry.position_ms as f32 / duration_ms).clamp(0.04, 1.0))
            }
        })
    }

    pub(crate) fn search_results_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        let palette = self.palette();
        let mut trigger_search_query: Option<String> = None;
        let mut selected_chip: Option<search::SearchChip> = None;

        let max_search_w = ui.available_width().min(1260.0);
        ui.set_max_width(max_search_w);

        ui.horizontal(|ui| {
            let avail_w = ui.available_width().max(180.0);
            let filters_btn_w = 96.0;
            let chips_w = (avail_w - filters_btn_w - 8.0).max(80.0);

            ui.allocate_ui_with_layout(
                egui::vec2(chips_w, 34.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("youtube_search_chips")
                        .auto_shrink([false, true])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);
                                let chips = [
                                    search::SearchChip::All,
                                    search::SearchChip::Shorts,
                                    search::SearchChip::Unwatched,
                                    search::SearchChip::Videos,
                                    search::SearchChip::RecentlyUploaded,
                                    search::SearchChip::Live,
                                ];
                                for chip in chips {
                                    let is_active = self.feed.search_chip == chip;
                                    let (bg, fg) = if is_active {
                                        (palette.active_button_bg, palette.active_button_fg)
                                    } else {
                                        (palette.header_button_bg, palette.header_button_fg)
                                    };
                                    let text = if is_active {
                                        RichText::new(chip.label()).size(13.5).strong().color(fg)
                                    } else {
                                        RichText::new(chip.label()).size(13.5).color(fg)
                                    };
                                    let btn = egui::Button::new(text).fill(bg).corner_radius(8);
                                    if ui
                                        .add_sized([0.0, 32.0], btn)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        selected_chip = Some(chip);
                                    }
                                }
                            });
                        });
                },
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let filters_active = !self.feed.search_filters.is_default();
                let resp = paint_search_filters_button(
                    ui,
                    self.feed.search_filters_open,
                    filters_active,
                    &palette,
                );
                if resp.clicked() {
                    self.feed.search_filters_open = !self.feed.search_filters_open;
                }
            });
        });

        ui.add_space(10.0);

        if let Some(chip) = selected_chip {
            let prev_chip = self.feed.search_chip;
            self.feed.search_chip = chip;
            let needs_remote_fetch = chip != search::SearchChip::Unwatched
                && (prev_chip != chip || prev_chip == search::SearchChip::Unwatched);
            if needs_remote_fetch
                && !self.feed.query.trim().is_empty()
                && !(prev_chip == search::SearchChip::Unwatched && chip == search::SearchChip::All)
            {
                self.start_search(ctx);
            }
        }

        if let Some(term) = trigger_search_query.take() {
            self.feed.query = term.clone();
            self.feed.search_chip = search::SearchChip::All;
            self.record_recent_search(&term);
            self.start_search(ctx);
        }

        let visible_indices: Vec<usize> = self
            .feed
            .videos
            .iter()
            .enumerate()
            .filter_map(|(idx, video)| {
                if self.feed.search_reported_urls.contains(&video.url) {
                    return None;
                }
                if self.feed.search_chip == search::SearchChip::Unwatched
                    && !self.is_video_unwatched(&video.url)
                {
                    return None;
                }
                Some(idx)
            })
            .collect();

        if visible_indices.is_empty() {
            ui.add_space(36.0);
            ui.vertical_centered(|ui| {
                if self.feed.search.rx.is_some() {
                    ui.spinner();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Pesquisando no YouTube...")
                            .size(15.0)
                            .color(palette.neutral_status_fg),
                    );
                } else {
                    ui.label(
                        RichText::new("Nenhum resultado encontrado.")
                            .size(18.0)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Tente outra pesquisa ou remova os filtros.")
                            .size(13.0)
                            .color(palette.neutral_status_fg),
                    );
                    if !self.feed.search_filters.is_default()
                        || self.feed.search_chip != search::SearchChip::All
                    {
                        ui.add_space(10.0);
                        if ui.button("Limpar filtros").clicked() {
                            self.feed.search_filters = search::SearchFilters::default();
                            self.feed.search_chip = search::SearchChip::All;
                            if !self.feed.query.trim().is_empty() {
                                self.start_search(ctx);
                            }
                        }
                    }
                    if !self.feed.recent_searches.is_empty() {
                        ui.add_space(14.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Buscas recentes:")
                                    .small()
                                    .color(palette.neutral_status_fg),
                            );
                            let recent = self.feed.recent_searches.clone();
                            for term in recent {
                                let chip = egui::Button::new(
                                    RichText::new(&term).small().color(palette.header_button_fg),
                                )
                                .fill(palette.card_alt_bg)
                                .corner_radius(14);
                                if ui
                                    .add(chip)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    trigger_search_query = Some(term);
                                }
                            }
                        });
                    }
                }
            });
            if let Some(term) = trigger_search_query {
                self.feed.query = term.clone();
                self.feed.search_chip = search::SearchChip::All;
                self.record_recent_search(&term);
                self.start_search(ctx);
            }
            return None;
        }

        let mut chosen = None;
        let mut enqueue = None;
        let mut download_req: Option<(Video, downloads::DownloadFormat)> = None;
        let mut toggle_watch_later = None;
        let mut open_save_modal = None;
        let mut open_share_modal = None;
        let mut open_report_modal = None;
        let mut open_channel = None;
        let mut filter_channel = None;
        let generation = self.feed.generation;
        let videos = std::mem::take(&mut self.feed.videos);
        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt("youtube_search_results")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 18.0;
                for (visible_pos, idx) in visible_indices.iter().enumerate() {
                    let video = &videos[*idx];
                    let is_playlist = self.feed.home.playlist_urls.contains(&video.url)
                        || video.url.contains("playlist?list=");
                    let card_action = ui
                        .push_id(("search_card", *idx, &video.url), |ui| {
                            self.youtube_search_result_card_ui(ui, ctx, video)
                        })
                        .inner;
                    if card_action.open_clicked {
                        if is_playlist {
                            if let Some(list_id) = crate::extract_playlist_id_from_url(&video.url) {
                                self.start_remote_playlist_fetch(&list_id, &video.url, ctx);
                            } else {
                                self.start_browser_feed(video.url.clone(), false, ctx);
                            }
                        } else {
                            chosen = Some(video.clone());
                        }
                    }
                    if card_action.enqueue_clicked {
                        enqueue = Some(video.clone());
                    }
                    if let Some(fmt) = card_action.download_format {
                        download_req = Some((video.clone(), fmt));
                    }
                    if card_action.toggle_watch_later {
                        toggle_watch_later = Some(video.clone());
                    }
                    if card_action.open_save_modal {
                        open_save_modal = Some(video.clone());
                    }
                    if card_action.share_clicked {
                        open_share_modal = Some(video.clone());
                    }
                    if card_action.open_report_modal {
                        open_report_modal = Some(video.clone());
                    }
                    if card_action.open_channel.is_some() {
                        open_channel = card_action.open_channel;
                    }
                    if card_action.filter_channel.is_some() {
                        filter_channel = card_action.filter_channel;
                    }
                    if visible_pos + 1 < visible_indices.len() {
                        ui.add_space(2.0);
                    }
                }
            });
        self.thumbnails.end_pass(ctx);
        if self.feed.generation == generation {
            self.feed.videos = videos;
        }
        if let Some(video) = enqueue
            && self.enqueue_video(video).is_ok()
        {
            self.message = self
                .pick("Adicionado \u{00e0} fila.", "Added to queue.")
                .into();
        }
        if let Some((video, fmt)) = download_req {
            self.start_download_video(video, fmt, ctx);
        }
        if let Some(video) = toggle_watch_later {
            self.library.data.toggle_watch_later(video);
            self.save_library();
            let msg = self.pick(
                "Lista \"Assistir mais tarde\" atualizada.",
                "\"Watch Later\" list updated.",
            );
            self.message = msg.into();
            self.toasts.success(msg);
        }
        if let Some(video) = open_save_modal {
            self.feed.search_save_modal_video = Some(video);
            if self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
                && self.auth.remote_playlists_rx.is_none()
            {
                self.start_fetch_remote_playlists(ctx);
            }
        }
        if let Some(video) = open_share_modal {
            self.open_share_modal(&video);
        }
        if let Some(video) = open_report_modal {
            self.feed.search_report_reason = None;
            self.feed.search_report_details.clear();
            self.feed.search_report_modal_video = Some(video);
        }
        if let Some(target) = open_channel {
            self.open_channel(target, ctx);
        } else if let Some(channel) = filter_channel {
            self.ui.search_results_mode = true;
            self.feed.filter = FeedFilter::Channel(channel);
            self.invalidate_feed_cache();
        }
        if let Some(video) = chosen.as_ref() {
            let cid = self.channel_target_for_video(video).map(|t| match t {
                ChannelTarget::ChannelId(id) => id,
                ChannelTarget::Handle(h) => h,
            });
            self.mark_followed_video_seen(cid.as_deref(), &video.url);
        }
        chosen
    }

    pub(crate) fn search_filters_modal_ui(&mut self, ctx: &egui::Context) {
        if !self.feed.search_filters_open {
            return;
        }
        let palette = self.palette();
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let modal_bg = if dark_theme {
            Color32::from_rgb(33, 33, 33)
        } else {
            Color32::WHITE
        };
        let modal_stroke = if dark_theme {
            Color32::from_rgb(56, 56, 56)
        } else {
            Color32::from_rgb(218, 218, 218)
        };
        let header_line = if dark_theme {
            Color32::from_rgb(58, 58, 58)
        } else {
            Color32::from_rgb(225, 225, 225)
        };

        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::new("search_filters_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
                resp
            });
        if backdrop.inner.clicked() {
            self.feed.search_filters_open = false;
            return;
        }

        let mut close_clicked = false;
        let mut filters_changed = false;
        let max_w = (screen.width() - 48.0).clamp(320.0, 820.0);

        egui::Window::new("Filtros de pesquisa")
            .id(egui::Id::new("search_filters_modal_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(max_w)
            .max_width(max_w)
            .order(egui::Order::Foreground)
            .frame(
                egui::Frame::NONE
                    .fill(modal_bg)
                    .stroke(egui::Stroke::new(1.0, modal_stroke))
                    .corner_radius(12.0)
                    .inner_margin(egui::Margin::symmetric(24, 20))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_width((max_w - 48.0).max(260.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Filtros de pesquisa")
                            .size(19.0)
                            .strong()
                            .color(palette.header_button_fg),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close_btn = egui::Button::new(
                            RichText::new("\u{2715}")
                                .size(17.0)
                                .color(palette.header_button_fg),
                        )
                        .frame(false);
                        if ui
                            .add(close_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            close_clicked = true;
                        }
                    });
                });
                ui.add_space(14.0);

                let compact = max_w < 680.0;
                let col_count = if compact { 2 } else { 5 };
                let col_gap = 18.0;
                let col_w = ((ui.available_width() - col_gap * (col_count as f32 - 1.0))
                    / col_count as f32)
                    .max(115.0);

                let render_col_header = |ui: &mut egui::Ui, title: &str| {
                    ui.label(
                        RichText::new(title)
                            .size(11.5)
                            .strong()
                            .color(palette.header_button_fg),
                    );
                    ui.add_space(6.0);
                    let (line_rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 1.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(line_rect, 0.0, header_line);
                    ui.add_space(8.0);
                };

                let filter_option = |ui: &mut egui::Ui, label: &str, active: bool| -> bool {
                    let text = if active {
                        RichText::new(label)
                            .size(13.5)
                            .strong()
                            .color(palette.header_button_fg)
                    } else {
                        RichText::new(label)
                            .size(13.5)
                            .color(palette.neutral_status_fg)
                    };
                    let resp = ui
                        .add(egui::Button::new(text).frame(false).wrap())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    resp.clicked()
                };

                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = col_gap;

                    ui.vertical(|ui| {
                        ui.set_width(col_w);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        render_col_header(ui, "TIPO");
                        for (label, kind) in [
                            ("V\u{00ed}deos", search::SearchTypeFilter::Videos),
                            ("Shorts", search::SearchTypeFilter::Shorts),
                            ("Canais", search::SearchTypeFilter::Channels),
                            ("Playlists", search::SearchTypeFilter::Playlists),
                            ("Filmes", search::SearchTypeFilter::Movies),
                        ] {
                            let active = self.feed.search_filters.kind == kind;
                            if filter_option(ui, label, active) {
                                self.feed.search_filters.kind = if active {
                                    search::SearchTypeFilter::All
                                } else {
                                    kind
                                };
                                self.feed.search_chip = search::SearchChip::All;
                                filters_changed = true;
                            }
                        }
                    });

                    ui.vertical(|ui| {
                        ui.set_width(col_w);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        render_col_header(ui, "DURA\u{00c7}\u{00c3}O");
                        for (label, dur) in [
                            ("Menos de 3 minutos", search::SearchDurationFilter::Short),
                            ("De 3 a 20 minutos", search::SearchDurationFilter::Medium),
                            ("Com mais de 20 minutos", search::SearchDurationFilter::Long),
                        ] {
                            let active = self.feed.search_filters.duration == dur;
                            if filter_option(ui, label, active) {
                                self.feed.search_filters.duration = if active {
                                    search::SearchDurationFilter::Any
                                } else {
                                    dur
                                };
                                filters_changed = true;
                            }
                        }
                    });

                    if !compact {
                        ui.vertical(|ui| {
                            ui.set_width(col_w);
                            ui.spacing_mut().item_spacing.y = 8.0;
                            render_col_header(ui, "DATA DO UPLOAD");
                            for (label, date) in [
                                ("Hoje", search::SearchUploadDateFilter::Today),
                                ("Esta semana", search::SearchUploadDateFilter::ThisWeek),
                                ("Este m\u{00ea}s", search::SearchUploadDateFilter::ThisMonth),
                                ("Este ano", search::SearchUploadDateFilter::ThisYear),
                            ] {
                                let active = self.feed.search_filters.upload_date == date;
                                if filter_option(ui, label, active) {
                                    self.feed.search_filters.upload_date = if active {
                                        search::SearchUploadDateFilter::Any
                                    } else {
                                        date
                                    };
                                    filters_changed = true;
                                }
                            }
                        });

                        ui.vertical(|ui| {
                            ui.set_width(col_w);
                            ui.spacing_mut().item_spacing.y = 8.0;
                            render_col_header(ui, "CARACTER\u{00cd}STICAS");
                            for (label, active, toggle) in
                                feature_filter_items(&self.feed.search_filters.features)
                            {
                                if filter_option(ui, label, active) {
                                    toggle(&mut self.feed.search_filters.features);
                                    filters_changed = true;
                                }
                            }
                        });

                        ui.vertical(|ui| {
                            ui.set_width(col_w);
                            ui.spacing_mut().item_spacing.y = 8.0;
                            render_col_header(ui, "PRIORIDADE");
                            for (label, sort) in [
                                ("Relev\u{00e2}ncia", search::SearchSort::Relevance),
                                ("Popularidade", search::SearchSort::Views),
                            ] {
                                let active = self.feed.search_filters.sort == sort;
                                if filter_option(ui, label, active) && !active {
                                    self.feed.search_filters.sort = sort;
                                    filters_changed = true;
                                }
                            }
                        });
                    }
                });

                if compact {
                    ui.add_space(12.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = col_gap;
                        ui.vertical(|ui| {
                            ui.set_width(col_w);
                            ui.spacing_mut().item_spacing.y = 8.0;
                            render_col_header(ui, "DATA DO UPLOAD");
                            for (label, date) in [
                                ("Hoje", search::SearchUploadDateFilter::Today),
                                ("Esta semana", search::SearchUploadDateFilter::ThisWeek),
                                ("Este m\u{00ea}s", search::SearchUploadDateFilter::ThisMonth),
                                ("Este ano", search::SearchUploadDateFilter::ThisYear),
                            ] {
                                let active = self.feed.search_filters.upload_date == date;
                                if filter_option(ui, label, active) {
                                    self.feed.search_filters.upload_date = if active {
                                        search::SearchUploadDateFilter::Any
                                    } else {
                                        date
                                    };
                                    filters_changed = true;
                                }
                            }
                        });

                        ui.vertical(|ui| {
                            ui.set_width(col_w);
                            ui.spacing_mut().item_spacing.y = 8.0;
                            render_col_header(ui, "PRIORIDADE");
                            for (label, sort) in [
                                ("Relev\u{00e2}ncia", search::SearchSort::Relevance),
                                ("Popularidade", search::SearchSort::Views),
                            ] {
                                let active = self.feed.search_filters.sort == sort;
                                if filter_option(ui, label, active) && !active {
                                    self.feed.search_filters.sort = sort;
                                    filters_changed = true;
                                }
                            }
                        });
                    });
                    ui.add_space(12.0);
                    render_col_header(ui, "CARACTER\u{00cd}STICAS");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(14.0, 8.0);
                        for (label, active, toggle) in
                            feature_filter_items(&self.feed.search_filters.features)
                        {
                            if filter_option(ui, label, active) {
                                toggle(&mut self.feed.search_filters.features);
                                filters_changed = true;
                            }
                        }
                    });
                }

                if !self.feed.search_filters.is_default() {
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Limpar filtros")
                                        .size(12.5)
                                        .color(palette.header_button_fg),
                                )
                                .fill(palette.header_button_bg)
                                .corner_radius(16),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.feed.search_filters = search::SearchFilters::default();
                            filters_changed = true;
                        }
                    });
                }
            });

        if close_clicked || filters_changed {
            self.feed.search_filters_open = false;
        }
        if filters_changed && !self.feed.query.trim().is_empty() {
            self.start_search(ctx);
        }
    }

    pub(crate) fn search_save_modal_ui(&mut self, ctx: &egui::Context) {
        let Some(video) = self.feed.search_save_modal_video.clone() else {
            return;
        };
        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::new("search_save_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
                resp
            });
        if backdrop.inner.clicked() {
            self.feed.search_save_modal_video = None;
            return;
        }

        let mut close = false;
        egui::Window::new("Salvar na playlist")
            .id(egui::Id::new("search_save_modal_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(360.0)
            .order(egui::Order::Foreground)
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                if self.save_menu_contents_ui(ui, &video, ctx) {
                    close = true;
                }
            });
        if close {
            self.feed.search_save_modal_video = None;
        }
    }

    pub(crate) fn search_report_modal_ui(&mut self, ctx: &egui::Context) {
        let Some(video) = self.feed.search_report_modal_video.clone() else {
            return;
        };
        let palette = self.palette();
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let modal_bg = if dark_theme {
            Color32::from_rgb(33, 33, 33)
        } else {
            Color32::WHITE
        };
        let modal_stroke = if dark_theme {
            Color32::from_rgb(56, 56, 56)
        } else {
            Color32::from_rgb(218, 218, 218)
        };
        let accent_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let disabled_fg = if dark_theme {
            Color32::from_rgb(113, 113, 113)
        } else {
            Color32::from_rgb(144, 144, 144)
        };

        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::new("search_report_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
                resp
            });
        if backdrop.inner.clicked() {
            self.feed.search_report_modal_video = None;
            self.feed.search_report_reason = None;
            return;
        }

        let mut close_modal = false;
        let mut confirm_report = false;

        let modal_w = (screen.width() - 48.0).clamp(300.0, 380.0);
        egui::Window::new("Denunciar imagem ou t\u{00ed}tulo")
            .id(egui::Id::new("search_report_modal_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(modal_w)
            .max_width(modal_w)
            .order(egui::Order::Foreground)
            .frame(
                egui::Frame::NONE
                    .fill(modal_bg)
                    .stroke(egui::Stroke::new(1.0, modal_stroke))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::symmetric(24, 22))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_width((modal_w - 48.0).max(240.0));
                ui.label(
                    RichText::new("Denunciar imagem ou t\u{00ed}tulo")
                        .size(16.0)
                        .strong()
                        .color(palette.header_button_fg),
                );
                ui.add_space(16.0);

                for (idx, reason) in oauth::AbuseReportReason::ALL.iter().enumerate() {
                    let selected = self.feed.search_report_reason == Some(idx);
                    let row_resp = ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        let (circle_rect, circle_resp) =
                            ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                        let ring_color = if selected {
                            accent_blue
                        } else if dark_theme {
                            Color32::from_rgb(200, 200, 200)
                        } else {
                            Color32::from_rgb(96, 96, 96)
                        };
                        ui.painter().circle_stroke(
                            circle_rect.center(),
                            9.0,
                            egui::Stroke::new(1.8, ring_color),
                        );
                        if selected {
                            ui.painter()
                                .circle_filled(circle_rect.center(), 5.0, accent_blue);
                        }
                        let text_resp = ui.add(
                            egui::Label::new(
                                RichText::new(reason.label())
                                    .size(14.5)
                                    .color(palette.header_button_fg),
                            )
                            .sense(egui::Sense::click()),
                        );
                        circle_resp.union(text_resp)
                    });
                    if row_resp
                        .inner
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.feed.search_report_reason = Some(idx);
                    }
                    if idx + 1 < oauth::AbuseReportReason::ALL.len() {
                        ui.add_space(12.0);
                    }
                }

                ui.add_space(22.0);
                let has_selection = self.feed.search_report_reason.is_some();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 18.0;
                        let report_fg = if has_selection {
                            accent_blue
                        } else {
                            disabled_fg
                        };
                        let report_btn = ui.add_enabled(
                            has_selection,
                            egui::Button::new(
                                RichText::new("Denunciar")
                                    .size(14.0)
                                    .strong()
                                    .color(report_fg),
                            )
                            .frame(false),
                        );
                        let report_btn = if has_selection {
                            report_btn.on_hover_cursor(egui::CursorIcon::PointingHand)
                        } else {
                            report_btn
                        };
                        if report_btn.clicked() {
                            confirm_report = true;
                        }

                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Cancelar")
                                        .size(14.0)
                                        .strong()
                                        .color(palette.header_button_fg),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            close_modal = true;
                        }
                    });
                });
            });

        if confirm_report
            && let Some(reason_idx) = self.feed.search_report_reason
            && let Some(reason) = oauth::AbuseReportReason::ALL.get(reason_idx).copied()
        {
            self.feed.search_reported_urls.insert(video.url.clone());
            self.feed.videos.retain(|v| v.url != video.url);
            self.invalidate_feed_cache();
            self.feed.search_report_modal_video = None;
            self.feed.search_report_reason = None;
            self.feed.search_report_details.clear();
            if self.auth.session.is_some()
                && let Some(video_id) = history::youtube_video_id(&video.url)
            {
                self.start_video_report(video_id, reason, ctx);
            } else {
                self.message = format!(
                    "Den\u{00fa}ncia registrada ({}). Resultado ocultado.",
                    reason.label()
                );
            }
        } else if close_modal {
            self.feed.search_report_modal_video = None;
            self.feed.search_report_reason = None;
        }
    }

    pub(crate) fn open_share_modal(&mut self, video: &Video) {
        let initial_secs = if self
            .selected
            .as_ref()
            .is_some_and(|sel| sel.url == video.url)
        {
            self.current_player_live_pos_ms().unwrap_or(0) / 1000
        } else {
            0
        };
        self.feed.share_modal_video = Some(video.clone());
        self.feed.share_start_at_enabled = false;
        self.feed.share_start_at_text = format_share_timestamp(initial_secs);
        self.feed.share_embed_open = false;
        self.feed.share_targets_scroll_offset = 0.0;
        self.feed.share_modal_copied_at = None;
    }

    pub(crate) fn share_modal_ui(&mut self, ctx: &egui::Context) {
        let Some(video) = self.feed.share_modal_video.clone() else {
            return;
        };
        let palette = self.palette();
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let modal_bg = if dark_theme {
            Color32::from_rgb(33, 33, 33)
        } else {
            Color32::WHITE
        };
        let modal_stroke = if dark_theme {
            Color32::from_rgb(56, 56, 56)
        } else {
            Color32::from_rgb(218, 218, 218)
        };
        let divider_color = if dark_theme {
            Color32::from_rgb(60, 60, 60)
        } else {
            Color32::from_rgb(225, 225, 225)
        };
        let url_box_bg = if dark_theme {
            Color32::from_rgb(15, 15, 15)
        } else {
            Color32::from_rgb(245, 245, 245)
        };
        let url_box_stroke = if dark_theme {
            Color32::from_rgb(63, 63, 63)
        } else {
            Color32::from_rgb(204, 204, 204)
        };
        let accent_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let chevron_bg = if dark_theme {
            Color32::from_rgb(40, 40, 40)
        } else {
            Color32::WHITE
        };

        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(egui::Id::new("share_modal_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 140));
                resp
            });
        if backdrop.inner.clicked() {
            self.feed.share_modal_video = None;
            return;
        }

        let video_id = history::youtube_video_id(&video.url);
        let parsed_start_secs = if video_id.is_some() && self.feed.share_start_at_enabled {
            parse_share_timestamp_secs(&self.feed.share_start_at_text)
        } else {
            None
        };
        let share_url = build_youtube_share_url(&video.url, parsed_start_secs);
        let recently_copied = self
            .feed
            .share_modal_copied_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(2));

        let mut close_modal = false;
        let mut copy_text: Option<String> = None;
        let mut create_post_clicked = false;
        let mut external_target_url: Option<String> = None;

        let modal_w = (screen.width() - 48.0).clamp(320.0, 512.0);
        egui::Window::new("Compartilhar")
            .id(egui::Id::new("share_modal_window"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_width(modal_w)
            .max_width(modal_w)
            .order(egui::Order::Foreground)
            .frame(
                egui::Frame::NONE
                    .fill(modal_bg)
                    .stroke(egui::Stroke::new(1.0, modal_stroke))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::symmetric(20, 18))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(0, 0, 0, 140),
                    }),
            )
            .show(ctx, |ui| {
                let inner_w = (modal_w - 40.0).max(260.0);
                ui.set_width(inner_w);

                // Top header: centered "Compartilhar em um post" + top-right close button
                let (header_rect, _) =
                    ui.allocate_exact_size(egui::vec2(inner_w, 28.0), egui::Sense::hover());
                ui.painter().text(
                    header_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Compartilhar em um post",
                    egui::FontId::proportional(16.0),
                    palette.header_button_fg,
                );
                let close_rect = egui::Rect::from_center_size(
                    egui::pos2(header_rect.right() - 12.0, header_rect.center().y),
                    egui::vec2(28.0, 28.0),
                );
                let close_resp = ui
                    .scope_builder(egui::UiBuilder::new().max_rect(close_rect), |ui| {
                        ui.add_sized(
                            [28.0, 28.0],
                            egui::Button::new(
                                RichText::new("\u{2715}")
                                    .size(16.0)
                                    .color(palette.header_button_fg),
                            )
                            .frame(false),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                    })
                    .inner;
                if close_resp.clicked() {
                    close_modal = true;
                }

                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    let post_btn = egui::Button::new(
                        RichText::new("Criar post")
                            .size(14.0)
                            .strong()
                            .color(palette.active_button_fg),
                    )
                    .fill(palette.active_button_bg)
                    .corner_radius(18);
                    if ui
                        .add_sized([108.0, 36.0], post_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        create_post_clicked = true;
                    }
                    if let Some(subs) = self.auth.my_channel_subscribers {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format_subscribers_pt(subs))
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                });

                ui.add_space(14.0);
                let (div1, _) =
                    ui.allocate_exact_size(egui::vec2(inner_w, 1.0), egui::Sense::hover());
                ui.painter().rect_filled(div1, 0.0, divider_color);
                ui.add_space(14.0);

                if self.feed.share_embed_open
                    && let Some(id) = video_id.as_deref()
                {
                    let embed_code = build_youtube_embed_code(id, &video.title, parsed_start_secs);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Incorporar v\u{00ed}deo")
                                .size(15.0)
                                .strong()
                                .color(palette.header_button_fg),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Voltar")
                                            .size(13.0)
                                            .color(palette.header_button_fg),
                                    )
                                    .fill(palette.header_button_bg)
                                    .corner_radius(16),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.feed.share_embed_open = false;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(url_box_bg)
                        .stroke(egui::Stroke::new(1.0, url_box_stroke))
                        .corner_radius(12.0)
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.set_width((inner_w - 24.0).max(200.0));
                            ui.label(
                                RichText::new(&embed_code)
                                    .size(12.5)
                                    .monospace()
                                    .color(palette.header_button_fg),
                            );
                        });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let copy_label = if recently_copied {
                                "\u{2713} Copiado"
                            } else {
                                "Copiar"
                            };
                            if ui
                                .add_sized(
                                    [86.0, 34.0],
                                    egui::Button::new(
                                        RichText::new(copy_label)
                                            .size(13.5)
                                            .strong()
                                            .color(palette.active_button_fg),
                                    )
                                    .fill(palette.active_button_bg)
                                    .corner_radius(17),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                copy_text = Some(embed_code);
                            }
                        });
                    });
                } else {
                    ui.label(
                        RichText::new("Compartilhar")
                            .size(15.0)
                            .strong()
                            .color(palette.header_button_fg),
                    );
                    ui.add_space(14.0);

                    let targets = [
                        ShareTargetKind::Embed,
                        ShareTargetKind::WhatsApp,
                        ShareTargetKind::X,
                        ShareTargetKind::Messages,
                        ShareTargetKind::Facebook,
                        ShareTargetKind::Email,
                        ShareTargetKind::Reddit,
                        ShareTargetKind::LinkedIn,
                    ];

                    let row_rect = ui.available_rect_before_wrap();
                    let row_bounds =
                        egui::Rect::from_min_size(row_rect.min, egui::vec2(inner_w, 86.0));

                    let scroll_out = egui::ScrollArea::horizontal()
                        .id_salt("share_targets_scroll")
                        .auto_shrink([false, true])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .horizontal_scroll_offset(self.feed.share_targets_scroll_offset)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 14.0;
                                for kind in targets {
                                    if kind == ShareTargetKind::Embed && video_id.is_none() {
                                        continue;
                                    }
                                    let (col_rect, col_resp) = ui.allocate_exact_size(
                                        egui::vec2(66.0, 82.0),
                                        egui::Sense::click(),
                                    );
                                    let icon_rect = egui::Rect::from_center_size(
                                        egui::pos2(col_rect.center().x, col_rect.top() + 28.0),
                                        egui::vec2(56.0, 56.0),
                                    );
                                    paint_share_target_icon(
                                        ui.painter(),
                                        icon_rect,
                                        kind,
                                        dark_theme,
                                        col_resp.hovered(),
                                    );
                                    ui.painter().text(
                                        egui::pos2(col_rect.center().x, col_rect.top() + 70.0),
                                        egui::Align2::CENTER_CENTER,
                                        kind.label(),
                                        egui::FontId::proportional(12.0),
                                        palette.neutral_status_fg,
                                    );
                                    if col_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        match kind {
                                            ShareTargetKind::Embed => {
                                                self.feed.share_embed_open = true;
                                            }
                                            ShareTargetKind::Messages => {
                                                copy_text = Some(share_url.clone());
                                                external_target_url =
                                                    Some(kind.launch_url(&share_url, &video.title));
                                            }
                                            _ => {
                                                external_target_url =
                                                    Some(kind.launch_url(&share_url, &video.title));
                                            }
                                        }
                                    }
                                }
                            });
                        });

                    let max_scroll =
                        (scroll_out.content_size.x - scroll_out.inner_rect.width()).max(0.0);
                    let current_offset = scroll_out.state.offset.x.clamp(0.0, max_scroll);
                    self.feed.share_targets_scroll_offset = current_offset;

                    let chevron_y = row_bounds.top() + 28.0;
                    if current_offset > 2.0 {
                        let left_rect = egui::Rect::from_center_size(
                            egui::pos2(row_bounds.left() + 18.0, chevron_y),
                            egui::vec2(36.0, 36.0),
                        );
                        let left_resp = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(left_rect), |ui| {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(36.0, 36.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().circle_filled(rect.center(), 18.0, chevron_bg);
                                ui.painter().circle_stroke(
                                    rect.center(),
                                    18.0,
                                    egui::Stroke::new(1.0, modal_stroke),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "<",
                                    egui::FontId::proportional(16.0),
                                    palette.header_button_fg,
                                );
                                resp
                            })
                            .inner;
                        if left_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.feed.share_targets_scroll_offset =
                                (current_offset - 160.0).max(0.0);
                        }
                    }

                    if max_scroll - current_offset > 2.0 {
                        let right_rect = egui::Rect::from_center_size(
                            egui::pos2(row_bounds.right() - 18.0, chevron_y),
                            egui::vec2(36.0, 36.0),
                        );
                        let right_resp = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(right_rect), |ui| {
                                let (rect, resp) = ui.allocate_exact_size(
                                    egui::vec2(36.0, 36.0),
                                    egui::Sense::click(),
                                );
                                ui.painter().circle_filled(rect.center(), 18.0, chevron_bg);
                                ui.painter().circle_stroke(
                                    rect.center(),
                                    18.0,
                                    egui::Stroke::new(1.0, modal_stroke),
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    ">",
                                    egui::FontId::proportional(16.0),
                                    palette.header_button_fg,
                                );
                                resp
                            })
                            .inner;
                        if right_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.feed.share_targets_scroll_offset =
                                (current_offset + 160.0).min(max_scroll);
                        }
                    }

                    ui.add_space(16.0);

                    // URL box + Copiar button
                    egui::Frame::new()
                        .fill(url_box_bg)
                        .stroke(egui::Stroke::new(1.0, url_box_stroke))
                        .corner_radius(12.0)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width((inner_w - 24.0).max(200.0));
                            ui.horizontal(|ui| {
                                let copy_btn_w = if recently_copied { 96.0 } else { 78.0 };
                                let url_max_w =
                                    (ui.available_width() - copy_btn_w - 10.0).max(100.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(url_max_w, 34.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&share_url)
                                                    .size(13.5)
                                                    .color(palette.header_button_fg),
                                            )
                                            .truncate(),
                                        );
                                    },
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let copy_label = if recently_copied {
                                            "\u{2713} Copiado"
                                        } else {
                                            "Copiar"
                                        };
                                        let copy_btn = egui::Button::new(
                                            RichText::new(copy_label)
                                                .size(13.5)
                                                .strong()
                                                .color(palette.active_button_fg),
                                        )
                                        .fill(palette.active_button_bg)
                                        .corner_radius(17);
                                        if ui
                                            .add_sized([copy_btn_w, 34.0], copy_btn)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            copy_text = Some(share_url.clone());
                                        }
                                    },
                                );
                            });
                        });
                }

                if video_id.is_some() {
                    ui.add_space(16.0);
                    let (div2, _) =
                        ui.allocate_exact_size(egui::vec2(inner_w, 1.0), egui::Sense::hover());
                    ui.painter().rect_filled(div2, 0.0, divider_color);
                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let checked = self.feed.share_start_at_enabled;
                        let (box_rect, box_resp) =
                            ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                        if checked {
                            ui.painter().rect_filled(box_rect, 3.0, accent_blue);
                            ui.painter().line_segment(
                                [
                                    egui::pos2(box_rect.left() + 4.0, box_rect.center().y),
                                    egui::pos2(box_rect.center().x - 1.0, box_rect.bottom() - 4.0),
                                ],
                                egui::Stroke::new(2.0, Color32::WHITE),
                            );
                            ui.painter().line_segment(
                                [
                                    egui::pos2(box_rect.center().x - 1.0, box_rect.bottom() - 4.0),
                                    egui::pos2(box_rect.right() - 3.5, box_rect.top() + 4.0),
                                ],
                                egui::Stroke::new(2.0, Color32::WHITE),
                            );
                        } else {
                            let stroke_color = if dark_theme {
                                Color32::from_rgb(170, 170, 170)
                            } else {
                                Color32::from_rgb(96, 96, 96)
                            };
                            ui.painter().rect_stroke(
                                box_rect,
                                3.0,
                                egui::Stroke::new(1.8, stroke_color),
                                egui::StrokeKind::Inside,
                            );
                        }

                        let label_color = if checked {
                            palette.header_button_fg
                        } else {
                            palette.neutral_status_fg
                        };
                        let label_resp = ui.add(
                            egui::Label::new(
                                RichText::new("Come\u{00e7}ar em")
                                    .size(13.5)
                                    .color(label_color),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if box_resp
                            .union(label_resp)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.feed.share_start_at_enabled = !self.feed.share_start_at_enabled;
                        }

                        ui.add_enabled(
                            self.feed.share_start_at_enabled,
                            egui::TextEdit::singleline(&mut self.feed.share_start_at_text)
                                .desired_width(64.0)
                                .hint_text("0:00"),
                        );
                    });
                }
            });

        if create_post_clicked {
            let _ = copy_text_to_clipboard(ctx, &share_url);
            let post_url = match self.auth.my_channel.as_ref() {
                Some(ChannelTarget::ChannelId(id)) => {
                    format!("https://www.youtube.com/channel/{id}/community?show_create_dialog=1")
                }
                _ => "https://www.youtube.com/post".to_owned(),
            };
            open_external_url(&post_url);
            self.feed.share_modal_copied_at = Some(Instant::now());
            let msg = "Link copiado. Abrindo cria\u{00e7}\u{00e3}o de post no YouTube.";
            self.message = msg.into();
            self.toasts.success(msg);
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        if let Some(url) = external_target_url {
            open_external_url(&url);
        }
        if let Some(text) = copy_text {
            match copy_text_to_clipboard(ctx, &text) {
                Ok(()) => {
                    self.feed.share_modal_copied_at = Some(Instant::now());
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|sel| sel.url == video.url)
                    {
                        self.watch.share_copied_at = Some(Instant::now());
                    }
                    self.message = if self.feed.share_embed_open {
                        "C\u{00f3}digo de incorpora\u{00e7}\u{00e3}o copiado.".into()
                    } else {
                        format!("Link do v\u{00ed}deo copiado: {text}")
                    };
                    self.toasts.success(if self.feed.share_embed_open {
                        "C\u{00f3}digo de incorpora\u{00e7}\u{00e3}o copiado."
                    } else {
                        "Link copiado para a \u{00e1}rea de transfer\u{00ea}ncia."
                    });
                    ctx.request_repaint_after(Duration::from_secs(2));
                }
                Err(error) => {
                    self.message = error.clone();
                    self.toasts.error(error);
                }
            }
        }
        if close_modal {
            self.feed.share_modal_video = None;
        }
    }

    pub(crate) fn explore_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        available_width: f32,
    ) {
        let palette = self.palette();
        ui.heading("Explorar");
        ui.label(
            RichText::new(
                "Escolha um assunto para montar uma descoberta. O ranking abaixo combina suas preferências locais com os resultados públicos carregados.",
            )
            .weak(),
        );
        ui.add_space(12.0);

        let categories = [
            (
                "Em alta",
                "Conteúdo em destaque",
                FeedFilter::RecentlyUploaded,
                "tendências brasil hoje",
            ),
            (
                "Música",
                "Clipes, álbuns e apresentações",
                FeedFilter::Music,
                "música clipe oficial",
            ),
            (
                "Jogos",
                "Gameplay, análise e esports",
                FeedFilter::Gaming,
                "gameplay jogos brasil",
            ),
            (
                "Podcasts",
                "Conversas e episódios completos",
                FeedFilter::Podcasts,
                "podcast entrevista completa",
            ),
            (
                "Ao vivo",
                "Transmissões em tempo real",
                FeedFilter::Live,
                "ao vivo transmissão",
            ),
            (
                "Mixes",
                "Sequências para ouvir por mais tempo",
                FeedFilter::Mixes,
                "mix música playlist",
            ),
            (
                "Novidades",
                "Assuntos que você ainda não viu",
                FeedFilter::NewToYou,
                "novidades cultura tecnologia brasil",
            ),
        ];
        let columns = if available_width < 620.0 { 1 } else { 2 };
        let gap = 10.0;
        let tile_width =
            ((available_width - gap * (columns as f32 - 1.0)) / columns as f32).clamp(180.0, 360.0);
        let mut chosen: Option<(FeedFilter, &str)> = None;

        egui::Grid::new("fasttube_explore_categories")
            .num_columns(columns)
            .spacing(egui::vec2(gap, gap))
            .show(ui, |ui| {
                for (index, (title, subtitle, filter, query)) in categories.into_iter().enumerate()
                {
                    let button = egui::Button::new(
                        RichText::new(format!("{title}\n{subtitle}"))
                            .size(13.0)
                            .strong()
                            .color(palette.header_button_fg),
                    )
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12);
                    if ui
                        .add_sized([tile_width, 66.0], button)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        chosen = Some((filter, query));
                    }
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });

        if let Some((filter, query)) = chosen {
            self.ui.browse_section = BrowseSection::Home;
            self.ui.search_results_mode = false;
            self.feed.filter = filter;
            self.feed.query = query.to_owned();
            self.invalidate_feed_cache();
            self.start_search(ctx);
        }
    }

    pub(crate) fn browse_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        self.ensure_history_synchronized();
        let palette = self.palette();
        self.browse_alerts_ui(ui, palette);

        if self.channels.view_open {
            return self.channel_section_ui(
                ui,
                ctx,
                "browse_channel_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
            );
        }

        if self.ui.browse_section == BrowseSection::Explore {
            self.explore_section_ui(ui, ctx, ui.available_width());
            return None;
        }

        if self.ui.browse_section == BrowseSection::Downloads {
            self.downloads_section_ui(ui, ctx);
            return None;
        }

        if self.ui.browse_section == BrowseSection::Insights {
            self.insights_section_ui(ui, ctx);
            return None;
        }

        if self.ui.browse_section != BrowseSection::Home {
            self.queue.remote_playlist_id = None;
        }

        if let Some(list_id) = self.queue.remote_playlist_id.clone() {
            let pt = self.ui_pt();
            let loading = self.queue.remote_playlist_rx.is_some();
            let count = self.feed.videos.len();
            let mut close_banner = false;
            let mut play_all = false;
            let mut enqueue_all = false;
            let mut save_collection = false;

            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                        ui.label(
                            RichText::new(if loading {
                                format!(
                                    "{} ({list_id})...",
                                    if pt {
                                        "Carregando playlist pública"
                                    } else {
                                        "Loading public playlist"
                                    }
                                )
                            } else {
                                format!(
                                    "{} ({list_id}) · {count} {}",
                                    if pt {
                                        "Playlist pública"
                                    } else {
                                        "Public playlist"
                                    },
                                    if pt { "vídeo(s)" } else { "video(s)" }
                                )
                            })
                            .size(13.5)
                            .strong()
                            .color(palette.channel_fg),
                        );

                        if !loading && count > 0 {
                            let play_btn = egui::Button::new(
                                RichText::new(if pt {
                                    "▶ Reproduzir tudo"
                                } else {
                                    "▶ Play All"
                                })
                                .size(12.5)
                                .strong()
                                .color(palette.active_button_fg),
                            )
                            .fill(palette.active_button_bg)
                            .corner_radius(16);
                            if ui
                                .add(play_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                play_all = true;
                            }

                            let queue_btn = egui::Button::new(
                                RichText::new(if pt { "+ Fila tudo" } else { "+ Queue All" })
                                    .size(12.5)
                                    .color(palette.header_button_fg),
                            )
                            .fill(palette.header_button_bg)
                            .corner_radius(16);
                            if ui
                                .add(queue_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                enqueue_all = true;
                            }

                            if self.queue.remote_playlist_save_name.is_empty() {
                                self.queue.remote_playlist_save_name =
                                    format!("Playlist {list_id}");
                            }
                            ui.add_sized(
                                [160.0, 26.0],
                                egui::TextEdit::singleline(
                                    &mut self.queue.remote_playlist_save_name,
                                )
                                .hint_text(if pt {
                                    "Nome da coleção"
                                } else {
                                    "Collection name"
                                }),
                            );
                            let save_btn = egui::Button::new(
                                RichText::new(if pt {
                                    "Salvar como coleção"
                                } else {
                                    "Save as Collection"
                                })
                                .size(12.5)
                                .color(palette.header_button_fg),
                            )
                            .fill(palette.header_button_bg)
                            .corner_radius(16);
                            if ui
                                .add(save_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                save_collection = true;
                            }
                        }

                        let close_btn = egui::Button::new(
                            RichText::new("✕")
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(16);
                        if ui
                            .add(close_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(if pt {
                                "Fechar playlist e voltar"
                            } else {
                                "Close playlist view"
                            })
                            .clicked()
                        {
                            close_banner = true;
                        }
                    });
                });
            ui.add_space(10.0);

            if close_banner {
                self.queue.remote_playlist_id = None;
                self.queue.remote_playlist_active_id = None;
                if let Some(cancel) = self.queue.remote_playlist_cancel.take() {
                    cancel.cancel();
                }
                self.queue.remote_playlist_rx = None;
                self.feed.home.title = None;
                let _ = self.restore_cached_home_feed(ctx);
                self.invalidate_feed_cache();
            } else if save_collection && !self.feed.videos.is_empty() {
                let raw_name = self.queue.remote_playlist_save_name.trim().to_owned();
                let target_name = if raw_name.is_empty() {
                    self.library
                        .data
                        .next_available_collection_name(&format!("Playlist {list_id}"))
                } else {
                    raw_name
                };
                match self
                    .library
                    .data
                    .create_collection_with_videos(&target_name, &self.feed.videos)
                {
                    Ok((idx, added)) => {
                        let created = self.library.data.collections[idx].name.clone();
                        self.save_library();
                        let msg = if pt {
                            format!("Playlist salva como coleção '{created}' ({added} vídeo(s)).")
                        } else {
                            format!("Playlist saved as collection '{created}' ({added} video(s)).")
                        };
                        self.message = msg.clone();
                        self.toasts.success(msg);
                    }
                    Err(err) => {
                        self.toasts.warning(err);
                    }
                }
            } else if enqueue_all && !self.feed.videos.is_empty() {
                let mut added = 0usize;
                let videos = self.feed.videos.clone();
                for video in videos {
                    if self.queue.data.add(video).is_ok() {
                        added += 1;
                    } else {
                        break;
                    }
                }
                self.persist_queue();
                let msg = if pt {
                    format!("{added} vídeo(s) adicionado(s) à fila.")
                } else {
                    format!("{added} video(s) added to queue.")
                };
                self.message = msg.clone();
                self.toasts.success(msg);
            } else if play_all && !self.feed.videos.is_empty() {
                let videos = self.feed.videos.clone();
                self.clear_queue();
                for video in videos.iter().skip(1) {
                    if self.queue.data.add(video.clone()).is_err() {
                        break;
                    }
                }
                self.persist_queue();
                let msg = if pt {
                    format!("Reproduzindo playlist ({} vídeo(s)).", videos.len())
                } else {
                    format!("Playing playlist ({} video(s)).", videos.len())
                };
                self.toasts.info(msg);
                return Some(videos[0].clone());
            }
        }

        self.browse_subscriptions_header_ui(ui, ctx, palette);

        if self.ui.search_results_mode && self.ui.browse_section == BrowseSection::Home {
            return self.search_results_ui(ui, ctx);
        }

        self.filter_chips_bar_ui(ui, ctx);
        ui.add_space(12.0);

        if self.ui.browse_section == BrowseSection::History {
            return self.history_section_ui(
                ui,
                "browse_history_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                None,
            );
        }

        if self.ui.browse_section == BrowseSection::Queue {
            self.queue_section_ui(
                ui,
                "browse_queue_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                None,
            );
            return None;
        }

        if self.ui.browse_section == BrowseSection::Library {
            return self.browse_library_ui(ui, ctx, palette);
        }

        self.browse_home_content_ui(ui, ctx, palette)
    }

    pub(crate) fn browse_alerts_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette) {
        ui.add_space(4.0);
        if let Some(error) = self
            .settings
            .buffer
            .error
            .as_ref()
            .or(self.settings.playback.error.as_ref())
            .or(self.settings.theme.error.as_ref())
        {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Aviso de configurações: {error}"))
                            .color(palette.error_banner_fg),
                    );
                });
            ui.add_space(6.0);
        }
        if let Some(error) = &self.history.error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Aviso do histórico: {error}"))
                            .color(palette.error_banner_fg),
                    );
                });
            ui.add_space(6.0);
        }
        if let Some(notice) = &self.history.notice {
            egui::Frame::new()
                .fill(palette.notice_bg)
                .stroke(egui::Stroke::new(1.0, palette.notice_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(notice).small().color(palette.notice_fg));
                });
            ui.add_space(6.0);
        }
    }

    pub(crate) fn browse_subscriptions_header_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        if self.ui.browse_section == BrowseSection::Subscriptions {
            let pt = self.ui_pt();
            let is_refreshing = if self.auth.authenticated {
                self.feed.home.rx.is_some()
            } else {
                self.followed_channels.feed_rx.is_some()
            };
            let has_local_followed = !self.followed_channels.data.channels.is_empty();
            let mut refresh_clicked = false;
            ui.horizontal(|ui| {
                ui.heading(if pt {
                    "Inscri\u{00e7}\u{00f5}es"
                } else {
                    "Subscriptions"
                });
                if self.auth.authenticated || has_local_followed {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if is_refreshing {
                            if pt {
                                "Atualizando..."
                            } else {
                                "Refreshing..."
                            }
                        } else if self.auth.authenticated {
                            if pt {
                                "\u{21bb} Atualizar"
                            } else {
                                "\u{21bb} Refresh"
                            }
                        } else if pt {
                            "\u{21bb} Atualizar RSS"
                        } else {
                            "\u{21bb} Refresh RSS"
                        };
                        let button = egui::Button::new(
                            RichText::new(label)
                                .size(13.0)
                                .strong()
                                .color(palette.header_button_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(16);
                        if ui.add_enabled(!is_refreshing, button).clicked() {
                            refresh_clicked = true;
                        }
                        if self.auth.authenticated
                            && self.feed.home.subscriptions_refreshing_in_background
                        {
                            ui.label(
                                RichText::new(if pt {
                                    "Sincronizando em segundo plano..."
                                } else {
                                    "Syncing in background..."
                                })
                                .size(12.0)
                                .color(palette.info_accent_fg),
                            );
                        } else if !is_refreshing
                            && self.auth.authenticated
                            && self.feed.home.subscriptions_cached_loaded
                            && let Some(cached_at) = self.feed.home.subscriptions_cached_at
                        {
                            let mins = cached_at.elapsed().as_secs() / 60;
                            let status = if mins == 0 {
                                if pt {
                                    "Atualizado agora".to_owned()
                                } else {
                                    "Updated just now".to_owned()
                                }
                            } else if mins == 1 {
                                if pt {
                                    "Atualizado há 1 min".to_owned()
                                } else {
                                    "Updated 1 min ago".to_owned()
                                }
                            } else if pt {
                                format!("Atualizado há {mins} min")
                            } else {
                                format!("Updated {mins} min ago")
                            };
                            ui.label(
                                RichText::new(status)
                                    .size(12.0)
                                    .color(palette.neutral_status_fg),
                            );
                        } else if !self.auth.authenticated
                            && let Some(loaded_at) = self.followed_channels.loaded_at
                        {
                            let mins = loaded_at.elapsed().as_secs() / 60;
                            let status = if mins == 0 {
                                if pt {
                                    "RSS atualizado agora".to_owned()
                                } else {
                                    "RSS updated just now".to_owned()
                                }
                            } else if pt {
                                format!("RSS há {mins} min")
                            } else {
                                format!("RSS {mins}m ago")
                            };
                            ui.label(
                                RichText::new(status)
                                    .size(12.0)
                                    .color(palette.neutral_status_fg),
                            );
                        }
                    });
                }
            });
            ui.label(
                RichText::new(if self.auth.authenticated {
                    if pt {
                        "Envios recentes dos canais que você acompanha. O Feed FastTube reorganiza os itens localmente."
                    } else {
                        "Recent uploads from your subscribed channels. FastTube ranks items locally."
                    }
                } else if has_local_followed {
                    if pt {
                        "Canais seguidos localmente (sem login Google) via RSS público do YouTube."
                    } else {
                        "Locally followed channels (no Google login) via public YouTube RSS feeds."
                    }
                } else if pt {
                    "Siga canais localmente na página do canal para acompanhar envios por RSS (sem conta Google), ou conecte sua conta nas Configurações."
                } else {
                    "Follow channels locally on any channel page to track uploads via RSS (no Google account required), or sign in from Settings."
                })
                .weak(),
            );

            if !self.auth.authenticated && has_local_followed {
                ui.add_space(6.0);
                let mut open_channel: Option<ChannelTarget> = None;
                let mut unfollow: Option<followed_channels::FollowedChannel> = None;
                let channels = self.followed_channels.data.channels.clone();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for ch in channels {
                        let ch_btn = egui::Button::new(
                            RichText::new(truncate_text_chars(&ch.title, 24))
                                .size(12.0)
                                .color(palette.header_button_fg),
                        )
                        .fill(palette.card_alt_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(14);
                        if ui
                            .add(ch_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            open_channel = if ch.channel_id.starts_with("UC") {
                                Some(ChannelTarget::ChannelId(ch.channel_id.clone()))
                            } else if let Some(h) = ch.handle.clone() {
                                Some(ChannelTarget::Handle(h))
                            } else {
                                Some(ChannelTarget::ChannelId(ch.channel_id.clone()))
                            };
                        }
                        let rm_btn = egui::Button::new(
                            RichText::new("✕")
                                .size(10.5)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(12);
                        if ui
                            .add(rm_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(if pt { "Deixar de seguir" } else { "Unfollow" })
                            .clicked()
                        {
                            unfollow = Some(ch);
                        }
                    }
                });
                if let Some(target) = open_channel {
                    self.open_channel(target, ctx);
                } else if let Some(ch) = unfollow {
                    self.toggle_local_follow_channel(
                        ch.channel_id,
                        ch.title,
                        ch.handle,
                        ch.url,
                        ch.avatar_url,
                    );
                }
            }

            ui.add_space(8.0);
            if refresh_clicked {
                if self.auth.authenticated {
                    self.open_subscriptions_section(true, ctx);
                } else {
                    self.start_local_followed_feed(true, ctx);
                }
            }
        }
    }

    pub(crate) fn home_skeleton_grid_ui(&self, ui: &mut egui::Ui, palette: ThemePalette) {
        let available = ui.available_width().max(220.0);
        let (gap, columns) = self.home_grid_metrics(available);
        let width =
            ((available - gap * (columns.saturating_sub(1) as f32)) / columns as f32).max(160.0);
        let thumb_height = (width * 9.0 / 16.0).round();
        let skeleton_bg = palette.card_alt_bg;

        for _row in 0..2 {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for _col in 0..columns {
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.set_max_width(width);
                        let (thumb_rect, _) = ui.allocate_exact_size(
                            egui::vec2(width, thumb_height),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(thumb_rect, 12.0, skeleton_bg);
                        ui.add_space(10.0);
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let (av_rect, _) = ui
                                .allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                            ui.painter()
                                .circle_filled(av_rect.center(), 18.0, skeleton_bg);
                            let line_w = (width - 48.0).max(80.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 8.0;
                                let (r1, _) = ui.allocate_exact_size(
                                    egui::vec2(line_w * 0.88, 14.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(r1, 4.0, skeleton_bg);
                                let (r2, _) = ui.allocate_exact_size(
                                    egui::vec2(line_w * 0.56, 12.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(r2, 4.0, skeleton_bg);
                            });
                        });
                    });
                }
            });
            ui.add_space(24.0);
        }
    }

    pub(crate) fn browse_home_content_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let is_fast_home_loading = self.feed.videos.is_empty()
            && (self.feed.home.batch_rx.is_some()
                || self.feed.search.rx.is_some()
                || (self.ui.browse_section == BrowseSection::Subscriptions
                    && self.feed.home.rx.is_some()));
        if !self.message.is_empty()
            && self.message != "Pesquise um vídeo ou cole um link do YouTube."
            && (self.feed.home.rx.is_some()
                || (self.feed.videos.is_empty() && !is_fast_home_loading))
        {
            egui::Frame::new()
                .fill(palette.message_box_bg)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(RichText::new(&self.message).small());
                });
            ui.add_space(8.0);
        }

        if !self.feed.videos.is_empty() {
            return self.home_grid_ui(ui, ctx);
        }

        if is_fast_home_loading {
            self.home_skeleton_grid_ui(ui, palette);
            return None;
        }

        if self.ui.browse_section == BrowseSection::Subscriptions {
            return None;
        }

        let mut chosen_from_history = None;
        if !self.queue.data.is_empty() {
            self.queue_section_ui(
                ui,
                "browse_queue_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                Some(180.0),
            );
            ui.add_space(8.0);
        }
        if !self.history.data.entries().is_empty() {
            chosen_from_history = self.history_section_ui(
                ui,
                "browse_history_scroll",
                false,
                Some(BROWSE_MAX_CONTENT_WIDTH),
                None,
            );
        }
        chosen_from_history
    }

    pub(crate) fn browse_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        let available_width = ui.available_width();
        let content_width = (available_width - 48.0).clamp(1.0, BROWSE_MAX_CONTENT_WIDTH);
        let side_margin = ((available_width - content_width) * 0.5).max(0.0);
        let mut chosen = None;
        ui.horizontal_top(|ui| {
            ui.add_space(side_margin);
            ui.vertical(|ui| {
                ui.set_width(content_width);
                ui.set_max_width(content_width);
                chosen = self.browse_ui(ui, ctx);
            });
        });
        chosen
    }
}
