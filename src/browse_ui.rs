use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};
use std::ops::Range;

/// Computes the visible row slice and exact top/bottom spacer heights for a uniform-height virtualized grid or list.
pub(crate) fn compute_uniform_visible_row_range(
    total_rows: usize,
    row_height: f32,
    row_spacing: f32,
    list_top: f32,
    clip_top: f32,
    clip_bottom: f32,
    overscan_rows: usize,
) -> (Range<usize>, f32, f32) {
    if total_rows == 0 {
        return (0..0, 0.0, 0.0);
    }
    let safe_h = row_height.max(1.0);
    let safe_gap = row_spacing.max(0.0);
    let stride = safe_h + safe_gap;
    let total_height = total_rows as f32 * safe_h + total_rows.saturating_sub(1) as f32 * safe_gap;

    let rel_top = clip_top - list_top;
    let rel_bottom = clip_bottom - list_top;

    if rel_bottom <= 0.0 {
        let end = overscan_rows.min(total_rows);
        let bottom_spacer = if end < total_rows {
            let rem = total_rows - end;
            rem as f32 * safe_h
                + if end > 0 {
                    rem as f32 * safe_gap
                } else {
                    rem.saturating_sub(1) as f32 * safe_gap
                }
        } else {
            0.0
        };
        return (0..end, 0.0, bottom_spacer);
    }
    if rel_top >= total_height {
        let start = total_rows.saturating_sub(overscan_rows);
        let top_spacer = if start == total_rows {
            total_height
        } else if start > 0 {
            start as f32 * stride
        } else {
            0.0
        };
        return (start..total_rows, top_spacer, 0.0);
    }

    let first_visible = if rel_top <= 0.0 {
        0
    } else {
        (rel_top / stride).floor() as usize
    }
    .min(total_rows.saturating_sub(1));
    let last_visible_excl = ((rel_bottom / stride).ceil() as usize).clamp(1, total_rows);

    let start = first_visible.saturating_sub(overscan_rows);
    let end = last_visible_excl
        .saturating_add(overscan_rows)
        .min(total_rows)
        .max(start);

    let top_spacer = if start == 0 {
        0.0
    } else if start == total_rows {
        total_height
    } else {
        start as f32 * stride
    };
    let bottom_spacer = if end >= total_rows {
        0.0
    } else {
        let rem = total_rows - end;
        rem as f32 * safe_h
            + if end > start {
                rem as f32 * safe_gap
            } else {
                rem.saturating_sub(1) as f32 * safe_gap
            }
    };
    (start..end, top_spacer, bottom_spacer)
}

/// Computes the visible item slice and exact top/bottom spacer heights for a variable-height virtualized list.
pub(crate) fn compute_variable_visible_range(
    item_heights: &[f32],
    spacing: f32,
    list_top: f32,
    clip_top: f32,
    clip_bottom: f32,
    overscan_items: usize,
) -> (Range<usize>, f32, f32) {
    let total = item_heights.len();
    if total == 0 {
        return (0..0, 0.0, 0.0);
    }
    let gap = spacing.max(0.0);
    let mut y = list_top;
    let mut first_visible: Option<usize> = None;
    let mut last_visible_excl: usize = 0;

    for (idx, &raw_h) in item_heights.iter().enumerate() {
        let h = raw_h.max(1.0);
        let item_bottom = y + h;
        if item_bottom >= clip_top && y <= clip_bottom {
            if first_visible.is_none() {
                first_visible = Some(idx);
            }
            last_visible_excl = idx + 1;
        }
        y = item_bottom + if idx + 1 < total { gap } else { 0.0 };
    }

    let (start, end) = match first_visible {
        Some(first) => {
            let s = first.saturating_sub(overscan_items);
            let e = last_visible_excl
                .saturating_add(overscan_items)
                .min(total)
                .max(s);
            (s, e)
        }
        None => {
            if clip_bottom < list_top {
                (0, overscan_items.min(total))
            } else {
                (total.saturating_sub(overscan_items), total)
            }
        }
    };

    let top_spacer = if start == 0 {
        0.0
    } else if start == total {
        item_heights.iter().map(|h| h.max(1.0)).sum::<f32>() + total.saturating_sub(1) as f32 * gap
    } else {
        item_heights[..start]
            .iter()
            .map(|h| h.max(1.0))
            .sum::<f32>()
            + start as f32 * gap
    };

    let bottom_spacer = if end >= total {
        0.0
    } else {
        let rem = total - end;
        item_heights[end..].iter().map(|h| h.max(1.0)).sum::<f32>()
            + if end > start {
                rem as f32 * gap
            } else {
                rem.saturating_sub(1) as f32 * gap
            }
    };

    (start..end, top_spacer, bottom_spacer)
}

pub(crate) fn estimated_search_card_height(panel_width: f32) -> f32 {
    let w = panel_width.max(240.0);
    if w >= 660.0 {
        let thumb_w = (w * 0.42).clamp(300.0, 500.0);
        (thumb_w * 9.0 / 16.0).round().max(176.0)
    } else {
        let thumb_h = (w * 9.0 / 16.0).round();
        thumb_h + 148.0
    }
}

pub(crate) fn estimated_home_shorts_shelf_height(available_width: f32) -> f32 {
    let avail = available_width.max(220.0);
    let columns = ((avail + 14.0) / 182.0).floor().clamp(2.0, 5.0) as usize;
    let card_w = ((avail - 14.0 * (columns.saturating_sub(1) as f32)) / columns as f32).max(130.0);
    let card_h = (card_w * 16.0 / 9.0).round();
    // Header (6 + 28 + 10) + 9:16 card + footer (8 + ~60) + bottom padding (20).
    card_h + 132.0
}

#[derive(Clone, Default)]
struct SearchResultsHeightCache {
    generation: u64,
    width_key: i32,
    search_chip: Option<search::SearchChip>,
    heights: Vec<f32>,
}

impl FastTube {
    pub(crate) fn close_navigation_drawer_if_overlay(&mut self, ctx: &egui::Context) {
        let viewport_width = ctx.input(|i| i.content_rect().width());
        let is_standard_watch = self.is_watch_page_active() && !self.is_watch_short_mode();
        if is_standard_watch || viewport_width < layout::PERSISTENT_SIDEBAR_MIN_VIEWPORT_WIDTH {
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
                self.navigation_shortcuts_ui(ui, ctx, palette);
                self.navigation_subscriptions_ui(ui, ctx, palette);
                self.navigation_explore_ui(ui, ctx, palette);
                self.navigation_utilities_ui(ui, ctx, palette);
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
        let in_home = !self.is_watch_page_active()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Home
            && !self.ui.search_results_mode
            && !matches!(
                self.feed.filter,
                FeedFilter::Shorts
                    | FeedFilter::RecentlyUploaded
                    | FeedFilter::Music
                    | FeedFilter::Live
                    | FeedFilter::Gaming
                    | FeedFilter::Podcasts
                    | FeedFilter::Topic(_)
            );

        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Home,
            self.pick("In\u{00ed}cio", "Home"),
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
        let shorts_watch_active = self.is_watch_page_active() && self.is_watch_short_mode();
        let in_shorts = !self.ui.settings_open
            && !self.channels.view_open
            && (shorts_watch_active
                || (!self.is_watch_page_active()
                    && (self.ui.browse_section == BrowseSection::Shorts
                        || (self.ui.browse_section == BrowseSection::Home
                            && matches!(self.feed.filter, FeedFilter::Shorts)))));
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
            self.open_shorts_tab(ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let in_subscriptions = !self.is_watch_page_active()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Subscriptions;
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Subscriptions,
            self.pick("Inscri\u{00e7}\u{00f5}es", "Subscriptions"),
            in_subscriptions,
            false,
            &palette,
        )
        .clicked()
        {
            self.open_subscriptions_section(false, ctx);
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

        let in_subscriptions = !self.is_watch_page_active()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Subscriptions;
        if sidebar_section_header_item(
            ui,
            self.pick("Inscri\u{00e7}\u{00f5}es", "Subscriptions"),
            true,
            in_subscriptions && !self.ui.manage_subscriptions_open,
            &palette,
        )
        .clicked()
        {
            self.ui.manage_subscriptions_open = false;
            self.open_subscriptions_section(false, ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Subscriptions,
            self.pick("Gerenciar inscri\u{00e7}\u{00f5}es", "Manage subscriptions"),
            in_subscriptions && self.ui.manage_subscriptions_open,
            false,
            &palette,
        )
        .clicked()
        {
            self.open_subscriptions_section(false, ctx);
            self.ui.manage_subscriptions_open = true;
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
                    let (has_unseen, is_live) = sidebar_channel_status(
                        &ch.channel_id,
                        &ch.title,
                        &self.followed_channels.new_video_urls,
                        &[
                            self.followed_channels.feed_items.as_slice(),
                            self.feed.home.subscriptions_cached_items.as_slice(),
                            self.feed.home.cached_items.as_slice(),
                        ],
                        &self.feed.videos,
                        &self.video_metadata,
                    );
                    if sidebar_channel_item(
                        ui,
                        ctx,
                        &mut self.avatars,
                        ch.avatar_url.as_deref(),
                        &ch.title,
                        active,
                        has_unseen,
                        is_live,
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
                    if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
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
            && (self.feed.home.channels_task.is_running()
                || (!self.feed.home.subscription_channels_loaded
                    && self.feed.home.stream_task.is_running()))
        {
            skeleton::sidebar_subscriptions_skeleton(ui, self.is_dark_theme());
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
                let (has_unseen, is_live) = sidebar_channel_status(
                    &channel.channel_id,
                    &channel.title,
                    &self.followed_channels.new_video_urls,
                    &[
                        self.followed_channels.feed_items.as_slice(),
                        self.feed.home.subscriptions_cached_items.as_slice(),
                        self.feed.home.cached_items.as_slice(),
                    ],
                    &self.feed.videos,
                    &self.video_metadata,
                );
                if sidebar_channel_item(
                    ui,
                    ctx,
                    &mut self.avatars,
                    channel.avatar_url.as_deref(),
                    &channel.title,
                    active,
                    has_unseen,
                    is_live,
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
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.open_channel(ChannelTarget::ChannelId(channel.channel_id), ctx);
                self.close_navigation_drawer_if_overlay(ctx);
            }

            if sidebar_channels.len() > 7 {
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

        let you_section_active = !self.is_watch_page_active()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Library;
        if sidebar_section_header_item(
            ui,
            self.pick("Voc\u{00ea}", "You"),
            true,
            you_section_active,
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
            self.ui.browse_section = BrowseSection::Library;
            self.close_navigation_drawer_if_overlay(ctx);
        }
        ui.add_space(2.0);

        let history_active = self.ui.browse_section == BrowseSection::History
            && !self.channels.view_open
            && !self.is_watch_page_active();
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::History,
            self.pick("Hist\u{00f3}rico", "History"),
            history_active,
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
            self.ui.browse_section = if self.ui.browse_section == BrowseSection::History {
                BrowseSection::Home
            } else {
                self.on_enter_history_section(ctx);
                BrowseSection::History
            };
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let playlists_active = self.ui.browse_section == BrowseSection::Playlists
            && !self.ui.settings_open
            && !self.channels.view_open
            && !self.is_watch_page_active();
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
            self.close_channel_view();
            if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
            self.library.selected_collection_idx = None;
            self.library.active_playlist_detail.playlist_id = None;
            if self.feed.home.title.as_deref() == Some("Vídeos da playlist") {
                self.feed.home.title = None;
            }
            self.ui.browse_section = BrowseSection::Playlists;
            self.on_enter_playlists_section(ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let channel_target = self.current_channel_target();
        let channel_active = self.channels.view_open && !self.ui.settings_open;
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::YourChannel,
            self.pick("Seu canal", "Your channel"),
            channel_active,
            false,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
            if let Some(target) = self.auth.my_channel.clone() {
                self.open_channel(target, ctx);
            } else if self.auth.authenticated {
                let placeholder = search::ChannelTarget::Handle("@voce".to_owned());
                let mut page = search::empty_channel_page_for_target(&placeholder);
                page.title = self
                    .auth
                    .my_channel_title
                    .clone()
                    .or_else(|| Some(self.pick("Seu canal", "Your channel").to_owned()));
                page.avatar_url = self.auth.my_channel_avatar_url.clone();
                page.subscriber_count = self.auth.my_channel_subscribers;
                self.apply_channel_page(ctx, page);
                self.start_fetch_my_channel(true, ctx);
            } else if channel_target.is_some() {
                self.channels.view_open = true;
                self.sync_player_window_visibility();
            } else {
                let local_target = search::ChannelTarget::Handle("@voce".to_owned());
                let mut page = search::empty_channel_page_for_target(&local_target);
                page.title = Some(self.pick("Seu canal", "Your channel").to_owned());
                self.apply_channel_page(ctx, page);
            }
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let watch_later_active = self.ui.browse_section == BrowseSection::WatchLater
            && !self.ui.settings_open
            && !self.channels.view_open
            && !self.is_watch_page_active();
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::WatchLater,
            self.pick("Assistir mais tarde", "Watch later"),
            watch_later_active,
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
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = BrowseSection::WatchLater;
            self.on_enter_watch_later_section(ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let liked_active = self.ui.browse_section == BrowseSection::LikedVideos
            && !self.ui.settings_open
            && !self.channels.view_open
            && !self.is_watch_page_active();
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::Liked,
            self.pick("V\u{00ed}deos com \"Gostei\"", "Liked videos"),
            liked_active,
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
            self.library.detail_chip_filter = 0;
            self.ui.browse_section = BrowseSection::LikedVideos;
            self.start_liked_videos_fetch(false, ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let dl_active = self.ui.browse_section == BrowseSection::Downloads
            && !self.ui.settings_open
            && !self.channels.view_open
            && !self.is_watch_page_active();
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

        let _ = sidebar_section_header_item(
            ui,
            self.pick("Mais do YouTube", "More from YouTube"),
            false,
            false,
            &palette,
        );
        ui.add_space(2.0);

        let labs_active = !self.is_watch_page_active()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::YouTubeLabs;
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::YouTubeLabs,
            "YouTube Labs",
            labs_active,
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
            self.ui.browse_section = BrowseSection::YouTubeLabs;
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        let in_explore_hub = !self.is_watch_page_active()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::Explore;
        if sidebar_section_header_item(
            ui,
            self.pick("Explorar", "Explore"),
            true,
            in_explore_hub,
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
            self.ui.browse_section = BrowseSection::Explore;
            self.select_explore_category(self.feed.explore_category, ctx);
            self.close_navigation_drawer_if_overlay(ctx);
        }
        ui.add_space(2.0);

        let movies_topic = self.pick("Filmes", "Movies").to_owned();
        let playables_topic = self.pick("Sala de Jogos", "Playables").to_owned();
        enum ExploreNavTarget {
            Hub(innertube::ExploreCategory),
            Filter(FeedFilter),
            Subscriptions,
        }
        let mut explore_items: Vec<(YtSidebarIcon, &str, ExploreNavTarget)> = vec![
            (
                YtSidebarIcon::Trending,
                self.pick("Em alta", "Trending"),
                ExploreNavTarget::Hub(innertube::ExploreCategory::Trending),
            ),
            (
                YtSidebarIcon::Music,
                self.pick("M\u{00fa}sica", "Music"),
                ExploreNavTarget::Hub(innertube::ExploreCategory::Music),
            ),
            (
                YtSidebarIcon::Movies,
                self.pick("Filmes", "Movies"),
                ExploreNavTarget::Filter(FeedFilter::Topic(movies_topic)),
            ),
            (
                YtSidebarIcon::Live,
                self.pick("Ao vivo", "Live"),
                ExploreNavTarget::Filter(FeedFilter::Live),
            ),
            (
                YtSidebarIcon::Gaming,
                self.pick("Jogos", "Gaming"),
                ExploreNavTarget::Hub(innertube::ExploreCategory::Gaming),
            ),
            (
                YtSidebarIcon::Podcasts,
                self.pick("Podcasts", "Podcasts"),
                ExploreNavTarget::Hub(innertube::ExploreCategory::Podcasts),
            ),
        ];
        if self.ui.explore_expanded {
            explore_items.push((
                YtSidebarIcon::Playables,
                self.pick("Sala de Jogos", "Playables"),
                ExploreNavTarget::Filter(FeedFilter::Topic(playables_topic)),
            ));
            explore_items.push((
                YtSidebarIcon::Subscriptions,
                self.pick("Assinaturas", "Memberships"),
                ExploreNavTarget::Subscriptions,
            ));
        }

        for (icon, label, target) in explore_items {
            let active = match &target {
                ExploreNavTarget::Hub(cat) => in_explore_hub && self.feed.explore_category == *cat,
                ExploreNavTarget::Filter(filter) => {
                    !self.is_watch_page_active()
                        && !self.ui.settings_open
                        && !self.channels.view_open
                        && self.ui.browse_section == BrowseSection::Home
                        && &self.feed.filter == filter
                }
                ExploreNavTarget::Subscriptions => {
                    !self.is_watch_page_active()
                        && !self.ui.settings_open
                        && !self.channels.view_open
                        && self.ui.browse_section == BrowseSection::Subscriptions
                }
            };
            if sidebar_pill_item(ui, icon, label, active, false, &palette).clicked() {
                match target {
                    ExploreNavTarget::Hub(cat) => {
                        self.ui.settings_open = false;
                        self.ui.search_results_mode = false;
                        self.close_channel_view();
                        if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                            self.close_video(ctx);
                        }
                        self.ui.browse_section = BrowseSection::Explore;
                        self.select_explore_category(cat, ctx);
                    }
                    ExploreNavTarget::Filter(filter) => {
                        self.ui.settings_open = false;
                        self.ui.search_results_mode = false;
                        self.ui.browse_section = BrowseSection::Home;
                        self.close_channel_view();
                        if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                            self.close_video(ctx);
                        }
                        self.feed.filter = filter;
                        if self.feed.home.title.is_some() || self.feed.videos.is_empty() {
                            let _ = self.restore_cached_home_feed(ctx);
                        }
                        self.invalidate_feed_cache();
                        self.ensure_feed_cache();
                        if self.feed.cache.ranked.len() < 6
                            && !self.feed.home.batch_task.is_running()
                            && !self.feed.home.stream_task.is_running()
                        {
                            self.start_fast_home_feed(false, ctx);
                        }
                    }
                    ExploreNavTarget::Subscriptions => {
                        self.open_subscriptions_section(false, ctx);
                    }
                }
                self.close_navigation_drawer_if_overlay(ctx);
            }
            ui.add_space(2.0);
        }

        let (toggle_icon, toggle_label) = if self.ui.explore_expanded {
            (
                YtSidebarIcon::ChevronUp,
                self.pick("Mostrar menos", "Show less"),
            )
        } else {
            (
                YtSidebarIcon::ChevronDown,
                self.pick("Mostrar mais", "Show more"),
            )
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
            YtSidebarIcon::Settings,
            self.pick("Configurações", "Settings"),
            self.ui.settings_open,
            false,
            &palette,
        )
        .clicked()
        {
            if !self.ui.settings_open && self.selected.is_some() {
                let _ = self.maybe_enter_auto_mini_player(ctx);
            }
            self.ui.settings_open = !self.ui.settings_open;
            self.sync_player_window_visibility();
            self.close_navigation_drawer_if_overlay(ctx);
        }

        ui.add_space(2.0);
        let report_active = !self.is_watch_page_active()
            && !self.ui.settings_open
            && !self.channels.view_open
            && self.ui.browse_section == BrowseSection::ReportHistory;
        if sidebar_pill_item(
            ui,
            YtSidebarIcon::ReportHistory,
            self.pick("Hist\u{00f3}rico de den\u{00fa}ncias", "Report history"),
            report_active,
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
            self.ui.browse_section = BrowseSection::ReportHistory;
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

        if self.auth.authenticated && !self.feed.home.stream_task.is_running() {
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

    pub(crate) fn compact_navigation_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let palette = self.palette();
        ui.set_min_width(COMPACT_NAVIGATION_WIDTH);
        ui.set_max_width(COMPACT_NAVIGATION_WIDTH);
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);
            let home_active = !self.is_watch_page_active()
                && !self.ui.settings_open
                && !self.channels.view_open
                && self.ui.browse_section == BrowseSection::Home
                && !self.ui.search_results_mode
                && !matches!(
                    self.feed.filter,
                    FeedFilter::Shorts
                        | FeedFilter::RecentlyUploaded
                        | FeedFilter::Music
                        | FeedFilter::Live
                        | FeedFilter::Gaming
                        | FeedFilter::Podcasts
                        | FeedFilter::Topic(_)
                );
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

            let shorts_watch_active = self.is_watch_page_active() && self.is_watch_short_mode();
            let shorts_active = !self.ui.settings_open
                && !self.channels.view_open
                && (shorts_watch_active
                    || (!self.is_watch_page_active()
                        && (self.ui.browse_section == BrowseSection::Shorts
                            || (self.ui.browse_section == BrowseSection::Home
                                && matches!(self.feed.filter, FeedFilter::Shorts)))));
            if compact_sidebar_item(ui, YtSidebarIcon::Shorts, "Shorts", shorts_active, &palette)
                .clicked()
            {
                self.open_shorts_tab(ctx);
            }

            let subscriptions_active = !self.is_watch_page_active()
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

            let you_active = !self.is_watch_page_active()
                && !self.ui.settings_open
                && matches!(
                    self.ui.browse_section,
                    BrowseSection::History
                        | BrowseSection::Library
                        | BrowseSection::Playlists
                        | BrowseSection::WatchLater
                        | BrowseSection::LikedVideos
                        | BrowseSection::Downloads
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
        });
    }

    pub(crate) fn watch_navigation_drawer_ui(&mut self, ctx: &egui::Context) {
        let palette = self.palette();
        let content = ctx.input(|i| i.content_rect());
        let width = layout::compute_youtube_sidebar_width(content.width());
        let top = content.top();
        let height = content.height().max(120.0);
        let open_t = ctx
            .animate_bool_with_time(
                egui::Id::new("watch_nav_drawer_scrim_anim"),
                self.ui.navigation_open,
                0.14,
            )
            .max(0.75);
        let scrim_alpha = (135.0 * open_t).round() as u8;
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
                        Color32::from_rgba_unmultiplied(0, 0, 0, scrim_alpha),
                    );
                    if response.clicked() {
                        self.ui.navigation_open = false;
                    }
                });
        }
        let fill = if self.is_dark_theme() {
            Color32::from_rgb(15, 15, 15)
        } else {
            Color32::from_rgb(255, 255, 255)
        };
        let stroke = if self.is_dark_theme() {
            Color32::from_rgb(48, 48, 48)
        } else {
            Color32::from_rgb(222, 222, 222)
        };
        egui::Area::new(egui::Id::new("watch_navigation_drawer"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(content.left(), top))
            .show(ctx, |ui| {
                let frame_resp = egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::NONE)
                    .inner_margin(egui::Margin::ZERO)
                    .shadow(egui::Shadow {
                        offset: [6, 0],
                        blur: 20,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(0, 0, 0, 120),
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(width, height));
                        ui.set_max_size(egui::vec2(width, height));
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(8.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 40.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.add_space(18.0);
                                let _ = self.header_menu_button_ui(ui);
                                ui.add_space(6.0);
                                if self.header_brand_ui(ui, palette).clicked() {
                                    self.handle_header_home(ctx);
                                    self.ui.navigation_open = false;
                                }
                            },
                        );
                        ui.add_space(8.0);
                        ui.spacing_mut().item_spacing.y = ctx.global_style().spacing.item_spacing.y;
                        self.navigation_ui(ui, ctx);
                    });
                let drawer_rect = frame_resp.response.rect;
                ui.painter().vline(
                    drawer_rect.right() - 0.5,
                    drawer_rect.y_range(),
                    egui::Stroke::new(1.0, stroke),
                );
            });
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
            let hover_t = ctx.animate_bool_with_time(
                ui.make_persistent_id(("watch_rec_hover", &video.url)),
                thumb.hovered(),
                0.14,
            );
            paint_card_hover_highlight(ui, thumb.rect, 8.0, hover_t, palette);
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
        let pt = self.ui_pt();
        self.ensure_feed_cache();
        let innertube_chips =
            if self.ui.browse_section == BrowseSection::Home && !self.feed.home_chips.is_empty() {
                self.feed.home_chips.clone()
            } else {
                Vec::new()
            };
        let use_innertube_chips = !innertube_chips.is_empty();
        let chips = if use_innertube_chips {
            Vec::new()
        } else {
            self.feed.cache.chips.clone()
        };
        let mut selected_chip: Option<FeedFilter> = None;
        let mut selected_innertube_chip: Option<Option<String>> = None;
        let mut clear_channel_filter = false;
        let mut toggle_pin_topic: Option<String> = None;

        let available_w = ui.available_width().max(160.0);
        let reserved_w = 32.0 * 2.0 + 8.0 + ui.spacing().item_spacing.x * 2.0;
        let chips_w = (available_w - reserved_w).max(80.0);
        let can_scroll_left = self.ui.chips_scroll_offset > 0.5;
        let mut prev_clicked = false;
        let mut next_clicked = false;
        let mut scroll_id_and_bounds = None;

        ui.horizontal(|ui| {
            let (prev_rect, prev_resp) =
                ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
            if can_scroll_left {
                if prev_resp.hovered() {
                    ui.painter()
                        .circle_filled(prev_rect.center(), 16.0, palette.header_button_bg);
                }
                let pc = prev_rect.center();
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(pc.x + 2.5, pc.y - 5.0),
                        egui::pos2(pc.x - 3.0, pc.y),
                        egui::pos2(pc.x + 2.5, pc.y + 5.0),
                    ],
                    egui::Stroke::new(1.6, palette.header_button_fg),
                ));
            }
            let prev_resp = prev_resp
                .on_hover_cursor(if can_scroll_left {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text(if can_scroll_left {
                    self.pick("Categorias anteriores", "Previous categories")
                } else {
                    ""
                });
            if can_scroll_left {
                paint_focus_ring_if_focused(ui, &prev_resp, 16.0);
            }
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
                                        let clear_resp = ui
                                            .add_sized([0.0, 32.0], chip_btn)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(if pt {
                                                "Remover filtro de canal e voltar para Tudo"
                                            } else {
                                                "Remove channel filter and return to All"
                                            });
                                        paint_focus_ring_if_focused(ui, &clear_resp, 8.0);
                                        if clear_resp.clicked() {
                                            clear_channel_filter = true;
                                        }
                                    }

                                    if use_innertube_chips {
                                        for chip in &innertube_chips {
                                            let is_active = chip.selected
                                                && !matches!(
                                                    self.feed.filter,
                                                    FeedFilter::Channel(_)
                                                );
                                            let (bg, fg) = if is_active {
                                                (palette.active_button_bg, palette.active_button_fg)
                                            } else {
                                                (palette.header_button_bg, palette.header_button_fg)
                                            };
                                            let text = if is_active {
                                                RichText::new(&chip.title)
                                                    .size(13.5)
                                                    .strong()
                                                    .color(fg)
                                            } else {
                                                RichText::new(&chip.title).size(13.5).color(fg)
                                            };
                                            let btn =
                                                egui::Button::new(text).fill(bg).corner_radius(8);
                                            let chip_resp = ui
                                                .add_sized([0.0, 32.0], btn)
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            paint_focus_ring_if_focused(ui, &chip_resp, 8.0);
                                            if chip_resp.clicked() {
                                                selected_innertube_chip = Some(chip.token.clone());
                                            }
                                        }
                                    } else {
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
                                            let label = chip.label();
                                            let text = if is_active {
                                                RichText::new(&label).size(13.5).strong().color(fg)
                                            } else {
                                                RichText::new(&label).size(13.5).color(fg)
                                            };
                                            let btn =
                                                egui::Button::new(text).fill(bg).corner_radius(8);
                                            let resp = ui
                                                .add_sized([0.0, 32.0], btn)
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            paint_focus_ring_if_focused(ui, &resp, 8.0);
                                            if let Some(topic) = topic_name {
                                                let topic_owned = topic.to_owned();
                                                let hint = if is_pinned {
                                                    if pt {
                                                        "T\u{00f3}pico fixado \u{00b7} Clique direito para desafixar"
                                                    } else {
                                                        "Pinned topic \u{00b7} Right-click to unpin"
                                                    }
                                                } else if pt {
                                                    "Clique direito para fixar t\u{00f3}pico"
                                                } else {
                                                    "Right-click to pin topic"
                                                };
                                                let resp = resp.on_hover_text(hint);
                                                resp.context_menu(|ui| {
                                                    let action_label = if is_pinned {
                                                        if pt {
                                                            "Desafixar t\u{00f3}pico"
                                                        } else {
                                                            "Unpin topic"
                                                        }
                                                    } else if pt {
                                                        "Fixar t\u{00f3}pico"
                                                    } else {
                                                        "Pin topic"
                                                    };
                                                    if ui.button(action_label).clicked() {
                                                        toggle_pin_topic =
                                                            Some(topic_owned.clone());
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
            let (next_rect, next_resp) =
                ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
            if can_scroll_right {
                if next_resp.hovered() {
                    ui.painter()
                        .circle_filled(next_rect.center(), 16.0, palette.header_button_bg);
                }
                let c = next_rect.center();
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(c.x - 2.5, c.y - 5.0),
                        egui::pos2(c.x + 3.0, c.y),
                        egui::pos2(c.x - 2.5, c.y + 5.0),
                    ],
                    egui::Stroke::new(1.6, palette.header_button_fg),
                ));
            }
            let next_resp = next_resp
                .on_hover_cursor(if can_scroll_right {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text(if can_scroll_right {
                    self.pick("Mais categorias", "More categories")
                } else {
                    ""
                });
            if can_scroll_right {
                paint_focus_ring_if_focused(ui, &next_resp, 16.0);
            }
            if next_resp.clicked() && can_scroll_right {
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
        if let Some(token) = selected_innertube_chip {
            self.ui.search_results_mode = false;
            self.ui.browse_section = BrowseSection::Home;
            self.feed.filter = FeedFilter::All;
            self.select_home_innertube_chip(token, ctx);
        } else if let Some(chip) = selected_chip {
            if matches!(chip, FeedFilter::Shorts) {
                self.open_shorts_tab(ctx);
            } else {
                self.ui.search_results_mode = false;
                self.ui.browse_section = BrowseSection::Home;
                self.feed.filter = chip;
                if !self.restore_cached_home_feed(ctx) {
                    self.feed.home.title = None;
                }
                self.invalidate_feed_cache();
                self.ensure_feed_cache();
                if self.feed.cache.ranked.len() < 6
                    && !self.feed.search.task.is_running()
                    && !self.feed.search_page_task.is_running()
                    && !self.feed.home.stream_task.is_running()
                    && !self.feed.home.batch_task.is_running()
                    && !self.feed.home_innertube_task.is_running()
                {
                    self.start_fast_home_feed(false, ctx);
                }
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
        let dark_theme = self.is_dark_theme();
        let is_playlist = self.feed.home.playlist_urls.contains(&video.url)
            || video.url.contains("playlist?list=");
        let is_mix = is_playlist && video.title.to_lowercase().contains("mix");
        let (is_music, channel_verified, subline, channel_target) = {
            let metadata = self.metadata_for(&video.url);
            (
                recommendations::is_music_video(video, metadata),
                is_channel_verified(metadata),
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
        let in_watch_later = self.library.data.is_in_watch_later(&video.url);
        let in_queue = !is_playlist && self.queue.data.is_queued(&video.url);
        let avatar_key = if is_playlist {
            String::new()
        } else {
            recommendations::normalize_channel_key(&video.channel)
        };

        let mut actions = HomeCardActions::default();

        let wash_fill_idx = ui.painter().add(egui::Shape::Noop);
        let wash_stroke_idx = ui.painter().add(egui::Shape::Noop);
        let card_resp = ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let thumb_height = (width * 9.0 / 16.0).round();
            if is_playlist {
                let mut thumb_rect = egui::Rect::NOTHING;
                let mut thumb_clicked = false;
                let mut thumb_hovered = false;
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (tabs_rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 9.0), egui::Sense::hover());
                    paint_mix_top_tabs(ui, tabs_rect, dark_theme);
                    let resp = self.thumbnails.show_cover_rounded(
                        ui,
                        ctx,
                        &video.thumbnail,
                        egui::vec2(width, (thumb_height - 9.0).max(40.0)),
                        12.0,
                    );
                    paint_focus_ring_if_focused(ui, &resp, 12.0);
                    thumb_rect = resp.rect;
                    thumb_clicked = resp.clicked();
                    thumb_hovered = resp.hovered();
                });
                let hover_t = ctx.animate_bool_with_time(
                    ui.make_persistent_id(("home_card_hover", &video.url)),
                    thumb_hovered,
                    0.14,
                );
                paint_card_hover_highlight(ui, thumb_rect, 12.0, hover_t, &palette);
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
                let thumb_resp = self.thumbnails.show_cover_rounded(
                    ui,
                    ctx,
                    &video.thumbnail,
                    egui::vec2(width, thumb_height),
                    12.0,
                );
                paint_focus_ring_if_focused(ui, &thumb_resp, 12.0);
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
                if in_queue {
                    paint_queued_badge(ui, thumb_resp.rect, is_new_followed);
                }
                let pointer_pos = ui.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
                let thumb_hovered = thumb_resp.hovered()
                    || pointer_pos.is_some_and(|pos| thumb_resp.rect.contains(pos));
                let hover_t = ctx.animate_bool_with_time(
                    ui.make_persistent_id(("home_card_hover", &video.url)),
                    thumb_hovered,
                    0.14,
                );
                paint_card_hover_highlight(ui, thumb_resp.rect, 12.0, hover_t, &palette);
                update_and_paint_card_hover_scrub(
                    ui,
                    ctx,
                    &video.url,
                    thumb_resp.rect,
                    thumb_hovered,
                    &video.duration,
                );
                let mut quick_action_interacted = false;
                if thumb_hovered && thumb_resp.rect.height() >= 76.0 {
                    let overlay_alpha = (224.0 * hover_t.max(0.8)).round() as u8;
                    let overlay_bg = Color32::from_rgba_unmultiplied(15, 15, 15, overlay_alpha);
                    let active_bg = Color32::from_rgba_unmultiplied(62, 166, 255, 235);
                    let wl_tip = if in_watch_later {
                        "Remover de Assistir mais tarde"
                    } else {
                        "Assistir mais tarde"
                    };
                    let wl_rect = egui::Rect::from_min_size(
                        egui::pos2(thumb_resp.rect.right() - 36.0, thumb_resp.rect.top() + 8.0),
                        egui::vec2(28.0, 28.0),
                    );
                    let queue_rect = egui::Rect::from_min_size(
                        egui::pos2(thumb_resp.rect.right() - 36.0, thumb_resp.rect.top() + 40.0),
                        egui::vec2(28.0, 28.0),
                    );
                    let wl_resp = ui
                        .interact(
                            wl_rect,
                            ui.make_persistent_id(("thumb_wl", &video.url)),
                            egui::Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(wl_tip);
                    ui.painter().rect_filled(
                        wl_rect,
                        6.0,
                        if in_watch_later {
                            active_bg
                        } else if wl_resp.hovered() {
                            Color32::from_rgba_unmultiplied(35, 35, 35, 245)
                        } else {
                            overlay_bg
                        },
                    );
                    paint_yt_icon(
                        ui.painter(),
                        wl_rect.center(),
                        YtSidebarIcon::WatchLater,
                        if in_watch_later {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        },
                        in_watch_later,
                    );
                    paint_focus_ring_if_focused(ui, &wl_resp, 6.0);
                    if wl_resp.hovered() || wl_resp.is_pointer_button_down_on() {
                        quick_action_interacted = true;
                    }
                    if wl_resp.clicked() {
                        actions.toggle_watch_later = true;
                        quick_action_interacted = true;
                    }

                    let queue_resp = ui
                        .interact(
                            queue_rect,
                            ui.make_persistent_id(("thumb_queue", &video.url)),
                            egui::Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Adicionar \u{00e0} fila");
                    ui.painter().rect_filled(
                        queue_rect,
                        6.0,
                        if in_queue {
                            active_bg
                        } else if queue_resp.hovered() {
                            Color32::from_rgba_unmultiplied(35, 35, 35, 245)
                        } else {
                            overlay_bg
                        },
                    );
                    paint_yt_icon(
                        ui.painter(),
                        queue_rect.center(),
                        YtSidebarIcon::Queue,
                        if in_queue {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        },
                        in_queue,
                    );
                    paint_focus_ring_if_focused(ui, &queue_resp, 6.0);
                    if queue_resp.hovered() || queue_resp.is_pointer_button_down_on() {
                        quick_action_interacted = true;
                    }
                    if queue_resp.clicked() {
                        actions.enqueue_clicked = true;
                        quick_action_interacted = true;
                    }
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
                    paint_focus_ring_if_focused(ui, &avatar_resp, 18.0);
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

                    let title_galley = layout_two_line_clamped_galley(
                        ui,
                        &video.title,
                        text_col_w,
                        egui::FontId::proportional(15.5),
                        palette.channel_fg,
                    );
                    let title_size = egui::vec2(text_col_w, title_galley.size().y.max(18.0));
                    let (title_rect, title_resp) =
                        ui.allocate_exact_size(title_size, egui::Sense::click());
                    ui.painter()
                        .galley(title_rect.min, title_galley, palette.channel_fg);
                    paint_focus_ring_if_focused(ui, &title_resp, 6.0);
                    if title_resp
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
                                    RichText::new(truncate_text_chars(
                                        &video.channel,
                                        if width >= 300.0 { 42 } else { 34 },
                                    ))
                                    .size(13.0)
                                    .color(palette.neutral_status_fg),
                                )
                                .frame(false);
                                let ch_resp = ui
                                    .add(ch_btn)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                paint_focus_ring_if_focused(ui, &ch_resp, 4.0);
                                if ch_resp.clicked() {
                                    if let Some(target) = &channel_target {
                                        actions.open_channel = Some(target.clone());
                                    } else {
                                        actions.filter_channel = Some(video.channel.clone());
                                    }
                                }
                                if channel_verified {
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
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    let pt = self.ui_pt();
                    let more_resp = paint_more_dots_button(ui, &palette);
                    egui::Popup::menu(&more_resp).show(|ui| {
                        ui.set_min_width(220.0);
                        if !is_playlist {
                            if styled_menu_item_button(
                                ui,
                                if pt {
                                    "Adicionar \u{00e0} fila"
                                } else {
                                    "Add to queue"
                                },
                                in_queue,
                                &palette,
                            )
                            .clicked()
                            {
                                actions.enqueue_clicked = true;
                                ui.close();
                            }
                            let wl_label = if in_watch_later {
                                if pt {
                                    "Remover de Assistir mais tarde"
                                } else {
                                    "Remove from Watch later"
                                }
                            } else if pt {
                                "Salvar em Assistir mais tarde"
                            } else {
                                "Save to Watch later"
                            };
                            if styled_menu_item_button(ui, wl_label, in_watch_later, &palette)
                                .clicked()
                            {
                                actions.toggle_watch_later = true;
                                ui.close();
                            }
                            if styled_menu_item_button(
                                ui,
                                if pt {
                                    "Salvar na playlist"
                                } else {
                                    "Save to playlist"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                actions.open_save_modal = true;
                                ui.close();
                            }
                            ui.menu_button(if pt { "⬇ Baixar" } else { "⬇ Download" }, |ui| {
                                for fmt in downloads::DownloadFormat::ALL {
                                    let lbl = if pt { fmt.label_pt() } else { fmt.label_en() };
                                    if styled_menu_item_button(ui, lbl, false, &palette).clicked() {
                                        actions.download_format = Some(fmt);
                                        ui.close();
                                    }
                                }
                            });
                            if styled_menu_item_button(
                                ui,
                                if pt { "Compartilhar" } else { "Share" },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                actions.share_clicked = true;
                                ui.close();
                            }
                        }
                        if let Some(target) = &channel_target {
                            if styled_menu_item_button(
                                ui,
                                if pt {
                                    "Ir para o canal"
                                } else {
                                    "Go to channel"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                actions.open_channel = Some(target.clone());
                                ui.close();
                            }
                        } else if !video.channel.trim().is_empty()
                            && styled_menu_item_button(
                                ui,
                                if pt {
                                    "Filtrar por canal"
                                } else {
                                    "Filter by channel"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                        {
                            actions.filter_channel = Some(video.channel.clone());
                            ui.close();
                        }
                        if !video.channel.trim().is_empty()
                            && styled_menu_item_button(
                                ui,
                                if pt {
                                    "N\u{00e3}o recomendar o canal"
                                } else {
                                    "Don't recommend channel"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                        {
                            actions.mute_channel = Some(video.channel.clone());
                            ui.close();
                        }
                        if let Some(topic_hint) = scored.primary_topic_hint()
                            && styled_menu_item_button(
                                ui,
                                &format!(
                                    "{}: {topic_hint}",
                                    if pt {
                                        "Silenciar t\u{00f3}pico"
                                    } else {
                                        "Mute topic"
                                    }
                                ),
                                false,
                                &palette,
                            )
                            .clicked()
                        {
                            actions.mute_topic = Some(topic_hint);
                            ui.close();
                        }
                        if styled_menu_item_button(
                            ui,
                            if pt { "Copiar link" } else { "Copy link" },
                            false,
                            &palette,
                        )
                        .clicked()
                        {
                            actions.copy_url = Some(video.url.clone());
                            ui.close();
                        }
                    });
                });
            });
        });
        let full_card_hovered = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| card_resp.response.rect.contains(pos));
        let card_wash_t = ctx.animate_bool_with_time(
            ui.make_persistent_id(("home_card_wash", &video.url)),
            full_card_hovered,
            0.14,
        );
        let (wash_fill, wash_stroke) =
            card_hover_wash_shapes(card_resp.response.rect, 12.0, card_wash_t, &palette);
        ui.painter().set(wash_fill_idx, wash_fill);
        ui.painter().set(wash_stroke_idx, wash_stroke);

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
        let is_subscription_feed = is_subscriptions_browse_section(self.ui.browse_section);
        if short_indices.is_empty()
            || self.feed.home.shorts_shelf_hidden
            || (!is_subscription_feed && self.settings.prefs.value.hide_shorts_in_home)
        {
            return (None, None, None, false);
        }
        let palette = self.palette();
        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let mut selected = None;
        let mut enqueue = None;
        let mut copy_url = None;
        let mut hide_shelf = false;
        let expanded = is_subscription_feed && self.ui.subscriptions_shorts_expanded;

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
                if !is_subscription_feed {
                    let shelf_more = paint_more_dots_button(ui, &palette);
                    egui::Popup::menu(&shelf_more).show(|ui| {
                        if styled_menu_item_button(
                            ui,
                            if pt {
                                "Ocultar esta estante"
                            } else {
                                "Hide this shelf"
                            },
                            false,
                            &palette,
                        )
                        .clicked()
                        {
                            hide_shelf = true;
                            ui.close();
                        }
                    });
                }
                let view_all_label = if expanded {
                    if pt { "Mostrar menos" } else { "Show less" }
                } else if pt {
                    "Ver tudo"
                } else {
                    "View all"
                };
                let link_color = if dark_theme {
                    Color32::from_rgb(62, 166, 255)
                } else {
                    Color32::from_rgb(6, 95, 212)
                };
                let view_all_btn = egui::Button::new(
                    RichText::new(view_all_label)
                        .size(13.5)
                        .strong()
                        .color(link_color),
                )
                .fill(Color32::TRANSPARENT)
                .frame(false);
                let view_all_resp = ui
                    .add(view_all_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                paint_focus_ring_if_focused(ui, &view_all_resp, 12.0);
                if view_all_resp.clicked() {
                    if is_subscription_feed {
                        self.ui.subscriptions_shorts_expanded =
                            !self.ui.subscriptions_shorts_expanded;
                    } else {
                        self.feed.filter = FeedFilter::Shorts;
                    }
                }
            });
        });
        ui.add_space(10.0);

        let col_gap = 14.0;
        let columns = ((available_width + col_gap) / 182.0)
            .floor()
            .clamp(2.0, 5.0) as usize;
        let card_w = ((available_width - col_gap * (columns.saturating_sub(1) as f32))
            / columns as f32)
            .max(130.0);
        let card_h = (card_w * 16.0 / 9.0).round();
        let total_items = if expanded {
            short_indices.len()
        } else {
            short_indices.len().min(columns)
        };
        let row_count = total_items.div_ceil(columns);
        let inner_spacing_x = ui.spacing().item_spacing.x;

        for row_idx in 0..row_count {
            let row_start = row_idx * columns;
            let row_end = (row_start + columns).min(total_items);
            let row_len = row_end.saturating_sub(row_start);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let row_left = ui.cursor().left();
                let clip = ui.clip_rect();
                let (visible_cols, left_spacer_w, right_spacer_w) =
                    compute_uniform_visible_row_range(
                        row_len,
                        card_w,
                        col_gap,
                        row_left,
                        clip.left(),
                        clip.right(),
                        1,
                    );
                if left_spacer_w > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(left_spacer_w, card_h + 68.0),
                        egui::Sense::hover(),
                    );
                }
                let rendered_end = visible_cols.end;
                for col_idx in visible_cols {
                    let idx = short_indices[row_start + col_idx];
                    let video = &ranked[idx].video;
                    let (is_recent, view_count) = {
                        let meta = self.metadata_for(&video.url);
                        (
                            recommendations::is_recent_upload(meta, None),
                            meta.and_then(|m| m.view_count),
                        )
                    };
                    ui.push_id(("short_shelf", &video.url), |ui| {
                        ui.spacing_mut().item_spacing.x = inner_spacing_x;
                        ui.vertical(|ui| {
                            ui.set_width(card_w);
                            ui.set_max_width(card_w);
                            let thumb = self.thumbnails.show_cover_rounded(
                                ui,
                                ctx,
                                &video.thumbnail,
                                egui::vec2(card_w, card_h),
                                12.0,
                            );
                            paint_focus_ring_if_focused(ui, &thumb, 12.0);
                            if !is_subscription_feed && is_recent {
                                paint_short_novo_badge(ui, thumb.rect);
                            }
                            let hover_t = ctx.animate_bool_with_time(
                                ui.make_persistent_id(("short_shelf_hover", &video.url)),
                                thumb.hovered(),
                                0.14,
                            );
                            paint_card_hover_highlight(ui, thumb.rect, 12.0, hover_t, &palette);
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
                                        let galley = layout_two_line_clamped_galley(
                                            ui,
                                            &video.title,
                                            title_w,
                                            egui::FontId::proportional(14.0),
                                            palette.channel_fg,
                                        );
                                        let (t_rect, title_resp) = ui.allocate_exact_size(
                                            egui::vec2(title_w, galley.size().y.max(16.0)),
                                            egui::Sense::click(),
                                        );
                                        ui.painter().galley(t_rect.min, galley, palette.channel_fg);
                                        paint_focus_ring_if_focused(ui, &title_resp, 6.0);
                                        if title_resp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .on_hover_text(&video.title)
                                            .clicked()
                                        {
                                            selected = Some(video.clone());
                                        }
                                    },
                                );
                                let more_resp = paint_more_dots_button(ui, &palette);
                                egui::Popup::menu(&more_resp).show(|ui| {
                                    if styled_menu_item_button(
                                        ui,
                                        if pt {
                                            "Adicionar \u{00e0} fila"
                                        } else {
                                            "Add to queue"
                                        },
                                        false,
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        enqueue = Some(video.clone());
                                        ui.close();
                                    }
                                    if styled_menu_item_button(
                                        ui,
                                        if pt { "Copiar link" } else { "Copy link" },
                                        false,
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        copy_url = Some(video.url.clone());
                                        ui.close();
                                    }
                                });
                            });
                            if let Some(views) = view_count {
                                ui.label(
                                    RichText::new(recommendations::format_youtube_compact_views(
                                        views,
                                    ))
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
                    if col_idx + 1 < rendered_end {
                        ui.add_space(col_gap);
                    }
                }
                if right_spacer_w > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(right_spacer_w, card_h + 68.0),
                        egui::Sense::hover(),
                    );
                }
            });
            if row_idx + 1 < row_count {
                ui.add_space(18.0);
            }
        }
        ui.add_space(20.0);
        (selected, enqueue, copy_url, hide_shelf)
    }

    pub(crate) fn subscription_feed_filter_bar_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let palette = self.palette();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
            let mut toggle_chip = |ui: &mut egui::Ui, value: &mut bool, label: &str, tip: &str| {
                let active = *value;
                let (bg, fg) = if active {
                    (palette.active_button_bg, palette.active_button_fg)
                } else {
                    (palette.header_button_bg, palette.header_button_fg)
                };
                let text = if active {
                    RichText::new(label).size(13.0).strong().color(fg)
                } else {
                    RichText::new(label).size(13.0).color(fg)
                };
                let btn = egui::Button::new(text).fill(bg).corner_radius(8);
                if ui
                    .add_sized([0.0, 32.0], btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tip)
                    .clicked()
                {
                    *value = !*value;
                    changed = true;
                }
            };
            let pt = self.ui_pt();
            toggle_chip(
                ui,
                &mut self.feed_preferences.values.newest_first,
                if pt { "Mais recentes" } else { "Latest" },
                if pt {
                    "Ordenar vídeos do mais recente ao mais antigo"
                } else {
                    "Sort videos from newest to oldest"
                },
            );
            toggle_chip(
                ui,
                &mut self.feed_preferences.values.hide_watched,
                if pt { "Não assistidos" } else { "Unwatched" },
                if pt {
                    "Ocultar vídeos já concluídos no histórico"
                } else {
                    "Hide videos already completed in history"
                },
            );
            toggle_chip(
                ui,
                &mut self.feed_preferences.values.hide_shorts,
                if pt { "Sem Shorts" } else { "No Shorts" },
                if pt {
                    "Ocultar vídeos curtos (Shorts)"
                } else {
                    "Hide short videos (Shorts)"
                },
            );
            toggle_chip(
                ui,
                &mut self.feed_preferences.values.hide_lives,
                if pt { "Sem lives" } else { "No live" },
                if pt {
                    "Ocultar transmissões ao vivo"
                } else {
                    "Hide live streams"
                },
            );

            let mut channels: Vec<String> = Vec::new();
            for video in &self.feed.videos {
                let name = video.channel.trim();
                if !name.is_empty() && !channels.iter().any(|c| c.eq_ignore_ascii_case(name)) {
                    channels.push(name.to_owned());
                }
            }
            let all_channels_label = if pt {
                "Todos os canais"
            } else {
                "All channels"
            };
            egui::ComboBox::from_id_salt("subscription_channel_filter")
                .selected_text(
                    self.feed_preferences
                        .values
                        .channel
                        .as_deref()
                        .unwrap_or(all_channels_label),
                )
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.feed_preferences.values.channel.is_none(),
                            all_channels_label,
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
        let show_filter_bar = self.ui.subscriptions_filters_open
            || self.feed_preferences.values.hide_watched
            || self.feed_preferences.values.hide_shorts
            || self.feed_preferences.values.hide_lives
            || self.feed_preferences.values.channel.is_some();
        let changed = if show_filter_bar {
            self.subscription_feed_filter_bar_ui(ui)
        } else {
            false
        };
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
                std::cmp::Reverse(
                    ranked[idx]
                        .metadata
                        .upload_date
                        .as_deref()
                        .and_then(recommendations::parse_upload_date_epoch),
                )
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
        (regular_list, shorts_list)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_home_grid_actions(
        &mut self,
        opened_short: bool,
        enqueue: Option<Video>,
        download_req: Option<(Video, downloads::DownloadFormat)>,
        toggle_watch_later: Option<Video>,
        open_save_modal: Option<Video>,
        open_share_modal: Option<Video>,
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
        if let Some(video) = open_save_modal {
            self.feed.search_save_modal_video = Some(video);
            if self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
                && !self.auth.remote_playlists_task.is_running()
            {
                self.start_fetch_remote_playlists(ctx);
            }
        }
        if let Some(video) = open_share_modal {
            self.open_share_modal(&video);
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

    pub(crate) fn subscriptions_grid_metrics(&self, available: f32) -> (f32, usize) {
        if self.settings.prefs.value.compact_card_density {
            return self.home_grid_metrics(available);
        }
        let columns = if available < layout::SINGLE_COLUMN_BROWSE_MAX_WIDTH {
            1
        } else if available < 820.0 {
            2
        } else if available < 1540.0 {
            3
        } else {
            4
        };
        (18.0, columns)
    }

    pub(crate) fn home_grid_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Video> {
        self.ensure_feed_cache();
        let ranked = std::mem::take(&mut self.feed.cache.ranked);
        let is_subscription_feed = is_subscriptions_browse_section(self.ui.browse_section);
        let mut visible_indices: Vec<usize> = (0..ranked.len()).collect();
        if is_subscription_feed {
            self.apply_subscription_feed_filters(ui, &ranked, &mut visible_indices);
        }

        let (main_indices, short_indices) =
            self.partition_home_feed_indices(&ranked, &visible_indices);
        let has_main_videos = !main_indices.is_empty();

        #[derive(Default)]
        struct HomeGridPending {
            selected: Option<Video>,
            opened_short: bool,
            playlist: Option<String>,
            enqueue: Option<Video>,
            download_req: Option<(Video, downloads::DownloadFormat)>,
            toggle_watch_later: Option<Video>,
            open_save_modal: Option<Video>,
            open_share_modal: Option<Video>,
            copy_url: Option<String>,
            open_channel: Option<ChannelTarget>,
            filter_channel: Option<String>,
            mute_channel: Option<String>,
            mute_topic: Option<String>,
            hide_shorts_shelf: bool,
            load_more_home: bool,
            load_more_subscriptions: bool,
        }
        let mut pending = HomeGridPending::default();
        let palette = self.palette();
        let pt = self.ui_pt();

        self.thumbnails.begin_pass();
        let scroll_out = egui::ScrollArea::vertical()
            .id_salt("home_grid")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available = ui.available_width().max(220.0);
                let width_key = available.round() as i32;
                let (gap, columns) = if is_subscription_feed {
                    self.subscriptions_grid_metrics(available)
                } else {
                    self.home_grid_metrics(available)
                };
                let width = ((available - gap * (columns.saturating_sub(1) as f32))
                    / columns as f32)
                    .max(160.0);
                let row_gap = 24.0;
                let est_row_height = (width * 9.0 / 16.0).round() + 10.0 + 118.0;
                let row0_cache_id = egui::Id::new("home_grid_row0_measured_h");
                let shelf_cache_id = egui::Id::new("home_grid_shorts_shelf_measured_h");
                let cached_row0_h = ctx
                    .data(|d| d.get_temp::<(i32, f32)>(row0_cache_id))
                    .filter(|(wk, h)| *wk == width_key && *h > 40.0)
                    .map(|(_, h)| h)
                    .unwrap_or(est_row_height + row_gap);
                let cached_shelf_h = ctx
                    .data(|d| d.get_temp::<(i32, f32)>(shelf_cache_id))
                    .filter(|(wk, h)| *wk == width_key && *h > 40.0)
                    .map(|(_, h)| h)
                    .unwrap_or_else(|| estimated_home_shorts_shelf_height(available));
                let clip = ui.clip_rect();
                let expanded_clip = clip.expand2(egui::vec2(0.0, 450.0));
                let show_shorts_shelf = !short_indices.is_empty()
                    && !self.feed.home.shorts_shelf_hidden
                    && (is_subscription_feed || !self.settings.prefs.value.hide_shorts_in_home);
                let total_rows = main_indices.len().div_ceil(columns);
                let inner_spacing_y = ui.spacing().item_spacing.y;

                let render_home_row =
                    |app: &mut Self,
                     ui: &mut egui::Ui,
                     row: &[usize],
                     pending: &mut HomeGridPending| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.y = inner_spacing_y;
                            ui.spacing_mut().item_spacing.x = gap;
                            for &idx in row {
                                let scored = &ranked[idx];
                                let video = &scored.video;
                                ui.push_id(&video.url, |ui| {
                                    let is_playlist =
                                        app.feed.home.playlist_urls.contains(&video.url)
                                            || video.url.contains("playlist?list=");
                                    let card_actions =
                                        app.youtube_grid_card_ui(ui, ctx, scored, width);
                                    if card_actions.open_clicked {
                                        if is_playlist {
                                            pending.playlist = Some(video.url.clone());
                                        } else {
                                            pending.selected = Some(video.clone());
                                        }
                                    }
                                    if card_actions.enqueue_clicked {
                                        pending.enqueue = Some(video.clone());
                                    }
                                    if let Some(fmt) = card_actions.download_format {
                                        pending.download_req = Some((video.clone(), fmt));
                                    }
                                    if card_actions.toggle_watch_later {
                                        pending.toggle_watch_later = Some(video.clone());
                                    }
                                    if card_actions.open_save_modal {
                                        pending.open_save_modal = Some(video.clone());
                                    }
                                    if card_actions.share_clicked {
                                        pending.open_share_modal = Some(video.clone());
                                    }
                                    if card_actions.copy_url.is_some() {
                                        pending.copy_url = card_actions.copy_url;
                                    }
                                    if card_actions.open_channel.is_some() {
                                        pending.open_channel = card_actions.open_channel;
                                    }
                                    if card_actions.filter_channel.is_some() {
                                        pending.filter_channel = card_actions.filter_channel;
                                    }
                                    if card_actions.mute_channel.is_some() {
                                        pending.mute_channel = card_actions.mute_channel;
                                    }
                                    if card_actions.mute_topic.is_some() {
                                        pending.mute_topic = card_actions.mute_topic;
                                    }
                                });
                            }
                        });
                    };

                if is_subscription_feed {
                    let live_indices =
                        filter_live_now_subscription_indices(&ranked, &main_indices, |url| {
                            self.metadata_for(url)
                        });
                    if !live_indices.is_empty() {
                        if let Some(live_sel) = self.subscriptions_live_now_shelf_ui(
                            ui,
                            ctx,
                            &ranked,
                            &live_indices,
                            available,
                        ) {
                            pending.selected = Some(live_sel);
                        }
                        ui.add_space(18.0);
                    }
                }

                if is_subscription_feed && self.ui.subscriptions_list_view {
                    let render_list_slice =
                        |app: &mut Self,
                         ui: &mut egui::Ui,
                         slice: &[usize],
                         pending: &mut HomeGridPending| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 16.0;
                                for &idx in slice {
                                    let video = &ranked[idx].video;
                                    let is_playlist =
                                        app.feed.home.playlist_urls.contains(&video.url)
                                            || video.url.contains("playlist?list=");
                                    ui.push_id(("sub_list_row", &video.url), |ui| {
                                        let act = app.youtube_search_result_card_ui(ui, ctx, video);
                                        if act.open_clicked {
                                            if is_playlist {
                                                pending.playlist = Some(video.url.clone());
                                            } else {
                                                pending.selected = Some(video.clone());
                                            }
                                        }
                                        if act.enqueue_clicked {
                                            pending.enqueue = Some(video.clone());
                                        }
                                        if let Some(fmt) = act.download_format {
                                            pending.download_req = Some((video.clone(), fmt));
                                        }
                                        if act.toggle_watch_later {
                                            pending.toggle_watch_later = Some(video.clone());
                                        }
                                        if act.open_save_modal {
                                            pending.open_save_modal = Some(video.clone());
                                        }
                                        if act.share_clicked {
                                            pending.open_share_modal = Some(video.clone());
                                        }
                                        if act.open_channel.is_some() {
                                            pending.open_channel = act.open_channel;
                                        }
                                        if act.filter_channel.is_some() {
                                            pending.filter_channel = act.filter_channel;
                                        }
                                    });
                                }
                            });
                        };
                    let split_at = main_indices.len().min(3);
                    if split_at > 0 {
                        render_list_slice(self, ui, &main_indices[..split_at], &mut pending);
                        ui.add_space(16.0);
                    }
                    if show_shorts_shelf {
                        let (short_sel, short_enq, short_copy, hide_shelf) = ui
                            .scope(|ui| {
                                ui.spacing_mut().item_spacing.y = inner_spacing_y;
                                self.shorts_shelf_ui(ui, ctx, &ranked, &short_indices, available)
                            })
                            .inner;
                        if short_sel.is_some() {
                            pending.opened_short = true;
                            pending.selected = short_sel;
                        }
                        if short_enq.is_some() {
                            pending.enqueue = short_enq;
                        }
                        if short_copy.is_some() {
                            pending.copy_url = short_copy;
                        }
                        pending.hide_shorts_shelf |= hide_shelf;
                    }
                    if split_at < main_indices.len() {
                        render_list_slice(self, ui, &main_indices[split_at..], &mut pending);
                        ui.add_space(row_gap);
                    }
                } else {
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;

                        if is_subscription_feed && has_main_videos {
                            ui.label(
                                RichText::new(if pt {
                                    "Mais relevantes"
                                } else {
                                    "Most relevant"
                                })
                                .size(16.5)
                                .strong()
                                .color(palette.channel_fg),
                            );
                            ui.add_space(10.0);
                        }

                        let top_rows = if total_rows == 0 {
                            0
                        } else if is_subscription_feed && self.ui.subscriptions_top_expanded {
                            total_rows.min(3)
                        } else {
                            1
                        };

                        if top_rows == 1 {
                            let cursor = ui.cursor();
                            let row0_top = cursor.min.y;
                            let row0_rect = egui::Rect::from_min_size(
                                egui::pos2(cursor.min.x, row0_top),
                                egui::vec2(available, cached_row0_h),
                            );
                            if !is_subscription_feed && !row0_rect.intersects(expanded_clip) {
                                ui.allocate_exact_size(
                                    egui::vec2(available, cached_row0_h),
                                    egui::Sense::hover(),
                                );
                            } else {
                                let row0_end = columns.min(main_indices.len());
                                render_home_row(self, ui, &main_indices[..row0_end], &mut pending);
                                ui.add_space(row_gap);
                                if !is_subscription_feed {
                                    let measured_h = (ui.cursor().top() - row0_top).max(40.0);
                                    if (measured_h - cached_row0_h).abs() > 1.0 {
                                        ctx.data_mut(|d| {
                                            d.insert_temp(row0_cache_id, (width_key, measured_h));
                                        });
                                    }
                                }
                            }
                        } else if top_rows > 1 {
                            for row_index in 0..top_rows {
                                let start = row_index * columns;
                                let end = (start + columns).min(main_indices.len());
                                render_home_row(self, ui, &main_indices[start..end], &mut pending);
                                ui.add_space(row_gap);
                            }
                        }

                        if is_subscription_feed
                            && has_main_videos
                            && (show_shorts_shelf
                                || total_rows > 1
                                || self.feed.subscriptions_continuation.is_some())
                        {
                            let show_pill =
                                total_rows > 1 || self.feed.subscriptions_continuation.is_some();
                            let expanded_top = self.ui.subscriptions_top_expanded && total_rows > 1;
                            let divider_h = if show_pill { 30.0 } else { 12.0 };
                            let (divider_rect, _) = ui.allocate_exact_size(
                                egui::vec2(available, divider_h),
                                egui::Sense::hover(),
                            );
                            let line_y = divider_rect.center().y;
                            let line_color = palette.card_alt_stroke;
                            if show_pill {
                                let label = if expanded_top {
                                    if pt { "Mostrar menos" } else { "Show less" }
                                } else if pt {
                                    "Mostrar mais"
                                } else {
                                    "Show more"
                                };
                                let text_galley = ui.painter().layout_no_wrap(
                                    label.to_owned(),
                                    egui::FontId::proportional(13.0),
                                    palette.channel_fg,
                                );
                                let pill_w = (text_galley.size().x + 48.0).min(available * 0.6);
                                let pill_rect = egui::Rect::from_center_size(
                                    egui::pos2(divider_rect.center().x, line_y),
                                    egui::vec2(pill_w, 30.0),
                                );
                                let left_end = (pill_rect.left() - 10.0).max(divider_rect.left());
                                let right_start =
                                    (pill_rect.right() + 10.0).min(divider_rect.right());
                                if left_end > divider_rect.left() {
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(divider_rect.left(), line_y),
                                            egui::pos2(left_end, line_y),
                                        ],
                                        egui::Stroke::new(1.0, line_color),
                                    );
                                }
                                if right_start < divider_rect.right() {
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(right_start, line_y),
                                            egui::pos2(divider_rect.right(), line_y),
                                        ],
                                        egui::Stroke::new(1.0, line_color),
                                    );
                                }
                                let pill_resp = ui.interact(
                                    pill_rect,
                                    ui.make_persistent_id("subs_top_show_more"),
                                    egui::Sense::click(),
                                );
                                let pill_fill = if pill_resp.hovered() {
                                    palette.header_button_bg
                                } else {
                                    palette.card_alt_bg
                                };
                                ui.painter().rect_filled(pill_rect, 15.0, pill_fill);
                                ui.painter().rect_stroke(
                                    pill_rect,
                                    15.0,
                                    egui::Stroke::new(1.0, line_color),
                                    egui::StrokeKind::Inside,
                                );
                                let text_x =
                                    pill_rect.center().x - (text_galley.size().x + 18.0) * 0.5;
                                let text_y = line_y - text_galley.size().y * 0.5;
                                ui.painter().galley(
                                    egui::pos2(text_x, text_y),
                                    text_galley,
                                    palette.channel_fg,
                                );
                                let chev_rect = egui::Rect::from_center_size(
                                    egui::pos2(pill_rect.right() - 18.0, line_y),
                                    egui::vec2(12.0, 12.0),
                                );
                                paint_chevron_down_icon(
                                    ui,
                                    chev_rect,
                                    palette.channel_fg,
                                    expanded_top,
                                );
                                paint_focus_ring_if_focused(ui, &pill_resp, 15.0);
                                if pill_resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    if total_rows > 1 {
                                        self.ui.subscriptions_top_expanded =
                                            !self.ui.subscriptions_top_expanded;
                                    } else if self.feed.subscriptions_continuation.is_some()
                                        && !self.feed.subscriptions_loading_more
                                    {
                                        pending.load_more_subscriptions = true;
                                    }
                                }
                            } else {
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(divider_rect.left(), line_y),
                                        egui::pos2(divider_rect.right(), line_y),
                                    ],
                                    egui::Stroke::new(1.0, line_color),
                                );
                            }
                            ui.add_space(12.0);
                        }

                        if show_shorts_shelf {
                            let shelf_cursor = ui.cursor();
                            let shelf_top = shelf_cursor.min.y;
                            let shelf_rect = egui::Rect::from_min_size(
                                egui::pos2(shelf_cursor.min.x, shelf_top),
                                egui::vec2(available, cached_shelf_h),
                            );
                            if !shelf_rect.intersects(expanded_clip) {
                                ui.allocate_exact_size(
                                    egui::vec2(available, cached_shelf_h),
                                    egui::Sense::hover(),
                                );
                            } else {
                                let (short_sel, short_enq, short_copy, hide_shelf) = ui
                                    .scope(|ui| {
                                        ui.spacing_mut().item_spacing.y = inner_spacing_y;
                                        self.shorts_shelf_ui(
                                            ui,
                                            ctx,
                                            &ranked,
                                            &short_indices,
                                            available,
                                        )
                                    })
                                    .inner;
                                let measured_shelf = (ui.cursor().top() - shelf_top).max(40.0);
                                if (measured_shelf - cached_shelf_h).abs() > 1.0 {
                                    ctx.data_mut(|d| {
                                        d.insert_temp(shelf_cache_id, (width_key, measured_shelf));
                                    });
                                }
                                if short_sel.is_some() {
                                    pending.opened_short = true;
                                    pending.selected = short_sel;
                                }
                                if short_enq.is_some() {
                                    pending.enqueue = short_enq;
                                }
                                if short_copy.is_some() {
                                    pending.copy_url = short_copy;
                                }
                                pending.hide_shorts_shelf |= hide_shelf;
                            }
                        }

                        let tail_rows = total_rows.saturating_sub(top_rows);
                        if tail_rows > 0 {
                            let tail_top = ui.cursor().top();
                            let (visible_tail, top_spacer_h, bottom_spacer_h) =
                                compute_uniform_visible_row_range(
                                    tail_rows,
                                    est_row_height,
                                    row_gap,
                                    tail_top,
                                    clip.top(),
                                    clip.bottom(),
                                    2,
                                );
                            if top_spacer_h > 0.0 {
                                ui.allocate_exact_size(
                                    egui::vec2(available, top_spacer_h),
                                    egui::Sense::hover(),
                                );
                            }
                            let rendered_end = visible_tail.end;
                            for tail_idx in visible_tail {
                                let row_index = tail_idx + top_rows;
                                let start = row_index * columns;
                                let end = (start + columns).min(main_indices.len());
                                render_home_row(self, ui, &main_indices[start..end], &mut pending);
                                if tail_idx + 1 < rendered_end {
                                    ui.add_space(row_gap);
                                }
                            }
                            if bottom_spacer_h > 0.0 {
                                ui.allocate_exact_size(
                                    egui::vec2(available, bottom_spacer_h),
                                    egui::Sense::hover(),
                                );
                            }
                            ui.add_space(row_gap);
                        }
                    });
                }

                if self.feed.home.batch_task.is_running()
                    && (self.ui.browse_section == BrowseSection::Home
                        || self.ui.browse_section == BrowseSection::Shorts)
                    && !self.ui.search_results_mode
                    && self.feed.home.title.is_none()
                {
                    ui.add_space(8.0);
                    let dark_theme = self.is_dark_theme();
                    skeleton::channel_tab_grid_skeleton(
                        ui,
                        available,
                        available < 520.0,
                        self.ui.browse_section == BrowseSection::Shorts,
                        1,
                        dark_theme,
                    );
                    ui.add_space(16.0);
                }

                if is_subscription_feed {
                    if self.feed.subscriptions_loading_more
                        || self.feed.subscriptions_continuation.is_some()
                    {
                        ui.add_space(8.0);
                        if feed_load_more_footer_ui(
                            ui,
                            self.feed.subscriptions_loading_more,
                            pt,
                            &palette,
                        ) {
                            pending.load_more_subscriptions = true;
                        }
                        ui.add_space(16.0);
                    }
                } else if (self.ui.browse_section == BrowseSection::Home
                    || self.ui.browse_section == BrowseSection::Shorts)
                    && !self.ui.search_results_mode
                    && self.feed.home.title.is_none()
                    && (self.feed.home_loading_more || self.feed.home_continuation.is_some())
                {
                    ui.add_space(8.0);
                    if feed_load_more_footer_ui(ui, self.feed.home_loading_more, pt, &palette) {
                        pending.load_more_home = true;
                    }
                    ui.add_space(16.0);
                }
            });
        self.feed.cache.ranked = ranked;
        self.thumbnails.end_pass(ctx);

        if pending.load_more_home {
            self.start_load_more_home(ctx);
        } else if pending.load_more_subscriptions {
            self.start_load_more_subscriptions(ctx);
        }

        let in_fast_home = (self.ui.browse_section == BrowseSection::Home
            || self.ui.browse_section == BrowseSection::Shorts)
            && !self.ui.search_results_mode
            && self.feed.home.title.is_none();
        if in_fast_home
            && has_main_videos
            && self.feed.home_continuation.is_none()
            && !self.feed.home_loading_more
            && self.feed.home.has_more
            && !self.feed.home.batch_task.is_running()
            && !self.feed.home.stream_task.is_running()
            && !self.feed.home_innertube_task.is_running()
            && !self.feed.search.task.is_running()
            && !self.feed.search_page_task.is_running()
        {
            let max_scroll = (scroll_out.content_size.y - scroll_out.inner_rect.height()).max(0.0);
            if scroll_out.content_size.y > 100.0
                && (scroll_out.content_size.y <= scroll_out.inner_rect.height() + 260.0
                    || scroll_out.state.offset.y >= (max_scroll - 260.0).max(0.0))
            {
                self.start_fast_home_feed(true, ctx);
            }
        }

        if let Some(video) = pending.selected.as_ref() {
            let cid = self.channel_target_for_video(video).map(|t| match t {
                ChannelTarget::ChannelId(id) => id,
                ChannelTarget::Handle(h) => h,
            });
            self.mark_followed_video_seen(cid.as_deref(), &video.url);
        }

        self.apply_home_grid_actions(
            pending.opened_short,
            pending.enqueue,
            pending.download_req,
            pending.toggle_watch_later,
            pending.open_save_modal,
            pending.open_share_modal,
            pending.copy_url,
            pending.playlist,
            pending.open_channel,
            pending.filter_channel,
            pending.mute_channel,
            pending.mute_topic,
            pending.hide_shorts_shelf,
            ctx,
        );
        pending.selected
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
        let dark_theme = self.is_dark_theme();
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

        let in_watch_later = self.library.data.is_in_watch_later(&video.url);

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
                    let resp = app.thumbnails.show_cover_rounded(
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
                        .show_cover_rounded(ui, ctx, &video.thumbnail, thumb_size, 12.0);
                paint_focus_ring_if_focused(ui, &thumb, 12.0);
                let in_queue = app.queue.data.is_queued(&video.url);
                let hover_t = ctx.animate_bool_with_time(
                    ui.make_persistent_id(("search_card_hover", &video.url)),
                    thumb.hovered(),
                    0.14,
                );
                paint_card_hover_highlight(ui, thumb.rect, 12.0, hover_t, &palette);
                let overlay_dur = if is_live { "" } else { video.duration.as_str() };
                paint_thumbnail_overlays(ui, thumb.rect, overlay_dur, progress_ratio, is_music);
                update_and_paint_card_hover_scrub(
                    ui,
                    ctx,
                    &video.url,
                    thumb.rect,
                    thumb.hovered(),
                    overlay_dur,
                );
                if in_queue {
                    paint_queued_badge(ui, thumb.rect, false);
                }
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

                        let title_galley = layout_two_line_clamped_galley(
                            ui,
                            &video.title,
                            title_width,
                            egui::FontId::proportional(if wide { 18.5 } else { 16.5 }),
                            palette.header_button_fg,
                        );
                        let (t_rect, title_resp) = ui.allocate_exact_size(
                            egui::vec2(title_width, title_galley.size().y.max(20.0)),
                            egui::Sense::click(),
                        );
                        ui.painter()
                            .galley(t_rect.min, title_galley, palette.header_button_fg);
                        paint_focus_ring_if_focused(ui, &title_resp, 6.0);
                        if title_resp
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

                            if is_channel_verified(metadata.as_ref()) {
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

        let wash_fill_idx = ui.painter().add(egui::Shape::Noop);
        let wash_stroke_idx = ui.painter().add(egui::Shape::Noop);
        let card_resp = ui.vertical(|ui| {
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
        });
        let full_card_hovered = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| card_resp.response.rect.contains(pos));
        let card_wash_t = ctx.animate_bool_with_time(
            ui.make_persistent_id(("search_card_wash", &video.url)),
            full_card_hovered,
            0.14,
        );
        let (wash_fill, wash_stroke) =
            card_hover_wash_shapes(card_resp.response.rect, 12.0, card_wash_t, &palette);
        ui.painter().set(wash_fill_idx, wash_fill);
        ui.painter().set(wash_stroke_idx, wash_stroke);

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
        let id = history::youtube_video_id(url)?;
        let entry = self.history.data.get(&id)?;
        let duration_ms = entry.duration_ms? as f32;
        if duration_ms <= 0.0 || entry.position_ms == 0 {
            None
        } else {
            Some((entry.position_ms as f32 / duration_ms).clamp(0.04, 1.0))
        }
    }

    pub(crate) fn search_results_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        let palette = self.palette();
        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let mut trigger_search_query: Option<String> = None;
        let mut selected_chip: Option<search::SearchChip> = None;

        let max_search_w = ui.available_width().min(1260.0);
        ui.set_max_width(max_search_w);

        let active_filter_count = count_active_search_filters(&self.feed.search_filters);
        let modal_open = self.feed.search_filters_modal_open || self.feed.search_filters_open;

        ui.horizontal(|ui| {
            let avail_w = ui.available_width().max(180.0);
            let filters_btn_w = if active_filter_count > 0 {
                126.0
            } else {
                104.0
            };
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
                                    let label = search_chip_label(chip, pt);
                                    let text = if is_active {
                                        RichText::new(label).size(13.5).strong().color(fg)
                                    } else {
                                        RichText::new(label).size(13.5).color(fg)
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
                let resp = search_filters_pill_button_ui(
                    ui,
                    modal_open,
                    active_filter_count,
                    pt,
                    &palette,
                );
                if resp.clicked() {
                    let next = !modal_open;
                    self.feed.search_filters_modal_open = next;
                    self.feed.search_filters_open = next;
                }
            });
        });

        if let Some(banner) = self
            .feed
            .search_spelling
            .as_ref()
            .and_then(|s| format_search_spelling_banner(s, pt))
        {
            ui.add_space(8.0);
            let link_color = if dark_theme {
                Color32::from_rgb(62, 166, 255)
            } else {
                Color32::from_rgb(6, 95, 212)
            };
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width().min(max_search_w));
                    if banner.is_auto_corrected {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(
                                RichText::new(&banner.primary_prefix)
                                    .size(14.0)
                                    .color(palette.header_button_fg),
                            );
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(&banner.primary_query)
                                            .size(14.0)
                                            .strong()
                                            .italics()
                                            .color(link_color),
                                    )
                                    .frame(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                trigger_search_query = Some(banner.primary_query.clone());
                            }
                        });
                        if let (Some(sec_prefix), Some(sec_query)) =
                            (&banner.secondary_prefix, &banner.secondary_query)
                        {
                            ui.add_space(2.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 5.0;
                                ui.label(
                                    RichText::new(sec_prefix)
                                        .size(13.0)
                                        .color(palette.neutral_status_fg),
                                );
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(sec_query)
                                                .size(13.0)
                                                .strong()
                                                .color(link_color),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    trigger_search_query = Some(sec_query.clone());
                                }
                            });
                        }
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(
                                RichText::new(&banner.primary_prefix)
                                    .size(14.0)
                                    .color(palette.header_button_fg),
                            );
                            let prompt_label = format!("{}?", banner.primary_query);
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(prompt_label)
                                            .size(14.0)
                                            .strong()
                                            .italics()
                                            .color(link_color),
                                    )
                                    .frame(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                trigger_search_query = Some(banner.primary_query.clone());
                            }
                        });
                    }
                });
        }

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
            self.feed.search_spelling = None;
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
            if self.feed.search.task.is_running() || self.feed.search_page_task.is_running() {
                ui.add_space(10.0);
                skeleton::search_results_skeleton(ui, ui.available_width(), dark_theme);
            } else {
                ui.add_space(36.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(if pt {
                            "Nenhum resultado encontrado."
                        } else {
                            "No results found."
                        })
                        .size(18.0)
                        .strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(if pt {
                            "Tente outra pesquisa ou remova os filtros."
                        } else {
                            "Try another search or clear filters."
                        })
                        .size(13.0)
                        .color(palette.neutral_status_fg),
                    );
                    if active_filter_count > 0 || self.feed.search_chip != search::SearchChip::All {
                        ui.add_space(10.0);
                        if styled_pill_button(
                            ui,
                            if pt {
                                "Limpar filtros"
                            } else {
                                "Clear filters"
                            },
                            false,
                            &palette,
                        )
                        .clicked()
                        {
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
                                RichText::new(if pt {
                                    "Buscas recentes:"
                                } else {
                                    "Recent searches:"
                                })
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
                });
            }
            if let Some(term) = trigger_search_query {
                self.feed.query = term.clone();
                self.feed.search_spelling = None;
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
        let mut load_more_clicked = false;
        let generation = self.feed.generation;
        let videos = std::mem::take(&mut self.feed.videos);
        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt("youtube_search_results")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let panel_width = ui.available_width().max(240.0);
                let width_key = panel_width.round() as i32;
                let est_h = estimated_search_card_height(panel_width);
                let cache_id = egui::Id::new("youtube_search_results_height_cache");
                let mut height_cache = ctx
                    .data(|d| d.get_temp::<SearchResultsHeightCache>(cache_id))
                    .unwrap_or_default();
                let mut cache_dirty = false;
                let active_chip = Some(self.feed.search_chip);
                if height_cache.generation != generation
                    || height_cache.width_key != width_key
                    || height_cache.search_chip != active_chip
                    || height_cache.heights.len() != visible_indices.len()
                {
                    height_cache.generation = generation;
                    height_cache.width_key = width_key;
                    height_cache.search_chip = active_chip;
                    height_cache.heights = vec![est_h; visible_indices.len()];
                    cache_dirty = true;
                }

                let inner_spacing_y = ui.spacing().item_spacing.y;
                let card_gap = 20.0;
                ui.spacing_mut().item_spacing.y = 0.0;
                let list_top = ui.cursor().top();
                let clip = ui.clip_rect();
                let (visible_range, top_spacer_h, bottom_spacer_h) = compute_variable_visible_range(
                    &height_cache.heights,
                    card_gap,
                    list_top,
                    clip.top(),
                    clip.bottom(),
                    2,
                );

                if top_spacer_h > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(panel_width, top_spacer_h),
                        egui::Sense::hover(),
                    );
                }

                let rendered_end = visible_range.end;
                for visible_pos in visible_range {
                    let idx = visible_indices[visible_pos];
                    let video = &videos[idx];
                    let is_playlist = self.feed.home.playlist_urls.contains(&video.url)
                        || video.url.contains("playlist?list=");
                    let card_start_y = ui.cursor().top();
                    let card_action = ui
                        .push_id(("search_card", idx, &video.url), |ui| {
                            ui.spacing_mut().item_spacing.y = inner_spacing_y;
                            self.youtube_search_result_card_ui(ui, ctx, video)
                        })
                        .inner;
                    let measured_h = (ui.cursor().top() - card_start_y).max(40.0);
                    if (height_cache.heights[visible_pos] - measured_h).abs() > 1.0 {
                        height_cache.heights[visible_pos] = measured_h;
                        cache_dirty = true;
                    }
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
                    if visible_pos + 1 < rendered_end {
                        ui.add_space(card_gap);
                    }
                }

                if bottom_spacer_h > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(panel_width, bottom_spacer_h),
                        egui::Sense::hover(),
                    );
                }

                if self.feed.search_loading_more || self.feed.search_continuation.is_some() {
                    ui.spacing_mut().item_spacing.y = inner_spacing_y;
                    ui.add_space(16.0);
                    if feed_load_more_footer_ui(ui, self.feed.search_loading_more, pt, &palette) {
                        load_more_clicked = true;
                    }
                    ui.add_space(16.0);
                }

                if cache_dirty {
                    ctx.data_mut(|d| d.insert_temp(cache_id, height_cache));
                }
            });
        self.thumbnails.end_pass(ctx);
        if self.feed.generation == generation {
            self.feed.videos = videos;
        }
        if load_more_clicked {
            self.start_load_more_search(ctx);
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
                && !self.auth.remote_playlists_task.is_running()
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
        use crate::innertube::{SearchDuration, SearchFeature, SearchResultType, SearchUploadDate};

        let mut is_open = self.feed.search_filters_modal_open || self.feed.search_filters_open;
        let pt = self.ui_pt();
        let palette = self.palette();
        let dark_theme = self.is_dark_theme();
        let header_line = if dark_theme {
            Color32::from_rgb(58, 58, 58)
        } else {
            Color32::from_rgb(225, 225, 225)
        };

        let screen = ctx.content_rect();
        let mut apply_clicked = false;
        let mut clear_clicked = false;
        let max_w = (screen.width() - 48.0).clamp(320.0, 880.0);
        let max_h = (screen.height() - 64.0).clamp(260.0, 620.0);

        let upload_date_items: [(&str, SearchUploadDate); 6] = [
            (
                if pt { "Qualquer data" } else { "Any" },
                SearchUploadDate::Any,
            ),
            (
                if pt {
                    "\u{00da}ltima hora"
                } else {
                    "Last hour"
                },
                SearchUploadDate::LastHour,
            ),
            (if pt { "Hoje" } else { "Today" }, SearchUploadDate::Today),
            (
                if pt { "Esta semana" } else { "This week" },
                SearchUploadDate::ThisWeek,
            ),
            (
                if pt { "Este m\u{00ea}s" } else { "This month" },
                SearchUploadDate::ThisMonth,
            ),
            (
                if pt { "Este ano" } else { "This year" },
                SearchUploadDate::ThisYear,
            ),
        ];
        let type_items: [(&str, SearchResultType); 5] = [
            (
                if pt { "Qualquer tipo" } else { "Any" },
                SearchResultType::Any,
            ),
            (
                if pt { "V\u{00ed}deo" } else { "Video" },
                SearchResultType::Video,
            ),
            (
                if pt { "Canal" } else { "Channel" },
                SearchResultType::Channel,
            ),
            ("Playlist", SearchResultType::Playlist),
            (if pt { "Filme" } else { "Movie" }, SearchResultType::Movie),
        ];
        let duration_items: [(&str, SearchDuration); 4] = [
            (
                if pt {
                    "Qualquer dura\u{00e7}\u{00e3}o"
                } else {
                    "Any"
                },
                SearchDuration::Any,
            ),
            (
                if pt { "Menos de 4 min" } else { "< 4 min" },
                SearchDuration::Under4Min,
            ),
            (
                if pt { "4 a 20 min" } else { "4\u{2013}20 min" },
                SearchDuration::Between4And20Min,
            ),
            (
                if pt { "Mais de 20 min" } else { "> 20 min" },
                SearchDuration::Over20Min,
            ),
        ];
        let feature_items: [(&str, SearchFeature); 9] = [
            (if pt { "Ao vivo" } else { "Live" }, SearchFeature::Live),
            ("4K", SearchFeature::FourK),
            ("HD", SearchFeature::Hd),
            (
                if pt { "Legendas/CC" } else { "Subtitles/CC" },
                SearchFeature::Subtitles,
            ),
            ("Creative Commons", SearchFeature::CreativeCommons),
            ("HDR", SearchFeature::Hdr),
            ("360\u{00b0}", SearchFeature::ThreeSixty),
            ("VR180", SearchFeature::Vr180),
            ("3D", SearchFeature::ThreeD),
        ];
        let sort_items: [(&str, search::SearchSort); 4] = [
            (
                if pt { "Relev\u{00e2}ncia" } else { "Relevance" },
                search::SearchSort::Relevance,
            ),
            (
                if pt { "Data de envio" } else { "Upload date" },
                search::SearchSort::Date,
            ),
            (
                if pt {
                    "Contagem de visualiza\u{00e7}\u{00f5}es"
                } else {
                    "View count"
                },
                search::SearchSort::Views,
            ),
            (
                if pt {
                    "Avalia\u{00e7}\u{00e3}o"
                } else {
                    "Rating"
                },
                search::SearchSort::Rating,
            ),
        ];

        let modal_title = if pt {
            "Filtros de pesquisa"
        } else {
            "Search filters"
        };
        app_modal_window(
            ctx,
            egui::Id::new("search_filters_modal_window"),
            modal_title,
            &mut is_open,
            max_w,
            |ui| {
                let compact = max_w < 720.0;
                let col_count = if compact { 2 } else { 5 };
                let col_gap = 16.0;
                let col_w = ((ui.available_width() - col_gap * (col_count as f32 - 1.0))
                    / col_count as f32)
                    .max(118.0);

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

                let radio_option = |ui: &mut egui::Ui, label: &str, active: bool| -> bool {
                    let text = if active {
                        RichText::new(format!("{label}  \u{2713}"))
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
                    paint_focus_ring_if_focused(ui, &resp, 6.0);
                    resp.clicked()
                };

                let feature_chip = |ui: &mut egui::Ui, label: &str, active: bool| -> bool {
                    let (fill, fg) = if active {
                        (palette.active_button_bg, palette.active_button_fg)
                    } else {
                        (palette.header_button_bg, palette.header_button_fg)
                    };
                    let text = if active {
                        RichText::new(format!("\u{2713} {label}"))
                            .size(12.5)
                            .strong()
                            .color(fg)
                    } else {
                        RichText::new(label).size(12.5).color(fg)
                    };
                    let resp = ui
                        .add(
                            egui::Button::new(text)
                                .fill(fill)
                                .stroke(egui::Stroke::NONE)
                                .corner_radius(14),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    paint_focus_ring_if_focused(ui, &resp, 14.0);
                    resp.clicked()
                };

                let col1_title = if pt { "DATA DE ENVIO" } else { "UPLOAD DATE" };
                let col2_title = if pt { "TIPO" } else { "TYPE" };
                let col3_title = if pt {
                    "DURA\u{00c7}\u{00c3}O"
                } else {
                    "DURATION"
                };
                let col4_title = if pt { "RECURSOS" } else { "FEATURES" };
                let col5_title = if pt { "ORDENAR POR" } else { "SORT BY" };

                egui::ScrollArea::vertical()
                    .id_salt("search_filters_modal_body")
                    .max_height((max_h - 132.0).max(140.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if !compact {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = col_gap;

                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col1_title);
                                    for (label, date) in upload_date_items {
                                        let active = self.feed.search_filters.upload_date == date;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.upload_date =
                                                if active && date != SearchUploadDate::Any {
                                                    SearchUploadDate::Any
                                                } else {
                                                    date
                                                };
                                        }
                                    }
                                });

                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col2_title);
                                    for (label, kind) in type_items {
                                        let active = self.feed.search_filters.result_type == kind;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.result_type =
                                                if active && kind != SearchResultType::Any {
                                                    SearchResultType::Any
                                                } else {
                                                    kind
                                                };
                                            self.feed.search_chip = search::SearchChip::All;
                                        }
                                    }
                                });

                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col3_title);
                                    for (label, dur) in duration_items {
                                        let active = self.feed.search_filters.duration == dur;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.duration =
                                                if active && dur != SearchDuration::Any {
                                                    SearchDuration::Any
                                                } else {
                                                    dur
                                                };
                                        }
                                    }
                                });

                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    render_col_header(ui, col4_title);
                                    for (label, feature) in feature_items {
                                        let active =
                                            self.feed.search_filters.features.contains(&feature);
                                        if feature_chip(ui, label, active) {
                                            toggle_search_feature(
                                                &mut self.feed.search_filters.features,
                                                feature,
                                            );
                                        }
                                    }
                                });

                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col5_title);
                                    for (label, sort) in sort_items {
                                        let active = self.feed.search_filters.sort == sort;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.sort = sort;
                                        }
                                    }
                                });
                            });
                        } else {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = col_gap;
                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col1_title);
                                    for (label, date) in upload_date_items {
                                        let active = self.feed.search_filters.upload_date == date;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.upload_date =
                                                if active && date != SearchUploadDate::Any {
                                                    SearchUploadDate::Any
                                                } else {
                                                    date
                                                };
                                        }
                                    }
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col2_title);
                                    for (label, kind) in type_items {
                                        let active = self.feed.search_filters.result_type == kind;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.result_type =
                                                if active && kind != SearchResultType::Any {
                                                    SearchResultType::Any
                                                } else {
                                                    kind
                                                };
                                            self.feed.search_chip = search::SearchChip::All;
                                        }
                                    }
                                });
                            });
                            ui.add_space(12.0);
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = col_gap;
                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col3_title);
                                    for (label, dur) in duration_items {
                                        let active = self.feed.search_filters.duration == dur;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.duration =
                                                if active && dur != SearchDuration::Any {
                                                    SearchDuration::Any
                                                } else {
                                                    dur
                                                };
                                        }
                                    }
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    render_col_header(ui, col5_title);
                                    for (label, sort) in sort_items {
                                        let active = self.feed.search_filters.sort == sort;
                                        if radio_option(ui, label, active) {
                                            self.feed.search_filters.sort = sort;
                                        }
                                    }
                                });
                            });
                            ui.add_space(12.0);
                            render_col_header(ui, col4_title);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                                for (label, feature) in feature_items {
                                    let active =
                                        self.feed.search_filters.features.contains(&feature);
                                    if feature_chip(ui, label, active) {
                                        toggle_search_feature(
                                            &mut self.feed.search_filters.features,
                                            feature,
                                        );
                                    }
                                }
                            });
                        }
                    });

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(10.0);

                let active_count = count_active_search_filters(&self.feed.search_filters);
                ui.horizontal(|ui| {
                    let can_clear =
                        active_count > 0 || self.feed.search_chip != search::SearchChip::All;
                    let clear_btn = egui::Button::new(
                        RichText::new(if pt {
                            "Limpar filtros"
                        } else {
                            "Clear filters"
                        })
                        .size(13.0)
                        .color(palette.header_button_fg),
                    )
                    .fill(palette.header_button_bg)
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(0.0, 34.0))
                    .corner_radius(17);
                    let clear_resp = ui
                        .add_enabled(can_clear, clear_btn)
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    paint_focus_ring_if_focused(ui, &clear_resp, 17.0);
                    if clear_resp.clicked() {
                        clear_clicked = true;
                    }

                    if active_count > 0 {
                        ui.add_space(8.0);
                        let count_text = if pt {
                            format!(
                                "{active_count} {}",
                                if active_count == 1 {
                                    "filtro ativo"
                                } else {
                                    "filtros ativos"
                                }
                            )
                        } else {
                            format!(
                                "{active_count} active {}",
                                if active_count == 1 {
                                    "filter"
                                } else {
                                    "filters"
                                }
                            )
                        };
                        ui.label(
                            RichText::new(count_text)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let apply_btn = egui::Button::new(
                            RichText::new(if pt { "Aplicar" } else { "Apply" })
                                .size(13.5)
                                .strong()
                                .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .stroke(egui::Stroke::NONE)
                        .min_size(egui::vec2(96.0, 34.0))
                        .corner_radius(17);
                        let apply_resp = ui
                            .add(apply_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        paint_focus_ring_if_focused(ui, &apply_resp, 17.0);
                        if apply_resp.clicked() {
                            apply_clicked = true;
                        }
                    });
                });
            },
        );

        if clear_clicked {
            self.feed.search_filters = search::SearchFilters::default();
            self.feed.search_chip = search::SearchChip::All;
            self.feed.search_filters_modal_open = false;
            self.feed.search_filters_open = false;
            if !self.feed.query.trim().is_empty() {
                self.start_search(ctx);
            }
        } else if apply_clicked {
            self.feed.search_filters_modal_open = false;
            self.feed.search_filters_open = false;
            if !self.feed.query.trim().is_empty() {
                self.start_search(ctx);
            }
        } else if !is_open {
            self.feed.search_filters_modal_open = false;
            self.feed.search_filters_open = false;
        }
    }

    pub(crate) fn search_save_modal_ui(&mut self, ctx: &egui::Context) {
        let open_t = ctx.animate_bool_with_time(
            egui::Id::new("search_save_modal_anim"),
            self.feed.search_save_modal_video.is_some(),
            0.14,
        );
        let Some(video) = self.feed.search_save_modal_video.clone() else {
            return;
        };
        let screen = ctx.content_rect();
        let scrim_alpha = (135.0 * open_t.max(0.75)).round() as u8;
        let backdrop = egui::Area::new(egui::Id::new("search_save_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, scrim_alpha),
                );
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
        let open_t = ctx.animate_bool_with_time(
            egui::Id::new("search_report_modal_anim"),
            self.feed.search_report_modal_video.is_some(),
            0.14,
        );
        let Some(video) = self.feed.search_report_modal_video.clone() else {
            return;
        };
        let palette = self.palette();
        let dark_theme = self.is_dark_theme();
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
        let scrim_alpha = (135.0 * open_t.max(0.75)).round() as u8;
        let backdrop = egui::Area::new(egui::Id::new("search_report_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, scrim_alpha),
                );
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
                    .corner_radius(14.0)
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
            let pt = self.ui_pt();
            let video_id = history::youtube_video_id(&video.url);
            let reason_str = if self.feed.search_report_details.trim().is_empty() {
                reason.label().to_owned()
            } else {
                format!(
                    "{} > {}",
                    reason.label(),
                    self.feed.search_report_details.trim()
                )
            };
            self.feed
                .report_history
                .push(crate::app_state::ReportedContentEntry {
                    kind: self.pick("Vídeo", "Video").to_owned(),
                    title: video.title.clone(),
                    channel: video.channel.clone(),
                    video_id: video_id.clone(),
                    reason: reason_str,
                    timestamp_label: format_report_date_today(pt),
                    status: self.pick("Ativo", "Active").to_owned(),
                });
            self.feed.search_report_modal_video = None;
            self.feed.search_report_reason = None;
            self.feed.search_report_details.clear();
            if self.auth.session.is_some()
                && let Some(video_id) = video_id
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
        let open_t = ctx.animate_bool_with_time(
            egui::Id::new("share_modal_anim"),
            self.feed.share_modal_video.is_some(),
            0.14,
        );
        let Some(video) = self.feed.share_modal_video.clone() else {
            return;
        };
        let palette = self.palette();
        let dark_theme = self.is_dark_theme();
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
        let scrim_alpha = (135.0 * open_t.max(0.75)).round() as u8;
        let backdrop = egui::Area::new(egui::Id::new("share_modal_backdrop"))
            .fixed_pos(screen.min)
            .order(egui::Order::Middle)
            .interactable(true)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, scrim_alpha),
                );
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
                    .corner_radius(14.0)
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
                    egui::pos2(header_rect.right() - 14.0, header_rect.center().y),
                    egui::vec2(28.0, 26.0),
                );
                let close_resp = ui
                    .scope_builder(egui::UiBuilder::new().max_rect(close_rect), |ui| {
                        styled_icon_chip_button(ui, true, "\u{2715}", &palette)
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

    #[allow(dead_code)]
    pub(crate) fn explore_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        _available_width: f32,
    ) {
        self.explore_hub_page_ui(ui, ctx);
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
            self.explore_hub_page_ui(ui, ctx);
            return None;
        }

        if self.ui.browse_section == BrowseSection::Subscriptions
            && self.ui.manage_subscriptions_open
        {
            self.manage_subscriptions_page_ui(ui, ctx);
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

        if self.ui.browse_section == BrowseSection::ReportHistory {
            self.report_history_page_ui(ui, ctx, palette);
            return None;
        }

        if self.ui.browse_section == BrowseSection::YouTubeLabs {
            self.youtube_labs_page_ui(ui, ctx, palette);
            return None;
        }

        if self.ui.browse_section != BrowseSection::Home {
            self.queue.remote_playlist_id = None;
        }

        if let Some(list_id) = self.queue.remote_playlist_id.clone() {
            let pt = self.ui_pt();
            let loading = self.queue.remote_playlist_task.is_running();
            let count = self.feed.videos.len();
            let mut close_banner = false;
            let mut play_all = false;
            let mut enqueue_all = false;
            let mut save_collection = false;

            let dark_theme = self.is_dark_theme();
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                        if loading {
                            let _ = skeleton::skeleton_box(
                                ui,
                                egui::vec2(190.0, 16.0),
                                8.0,
                                dark_theme,
                            );
                        } else {
                            ui.label(
                                RichText::new(format!(
                                    "{} ({list_id}) · {count} {}",
                                    if pt {
                                        "Playlist pública"
                                    } else {
                                        "Public playlist"
                                    },
                                    if pt { "vídeo(s)" } else { "video(s)" }
                                ))
                                .size(13.5)
                                .strong()
                                .color(palette.channel_fg),
                            );
                        }

                        if !loading && count > 0 {
                            let play_label = if pt {
                                "▶ Reproduzir tudo"
                            } else {
                                "▶ Play All"
                            };
                            if styled_compact_button(ui, play_label, true, &palette).clicked() {
                                play_all = true;
                            }

                            let queue_label = if pt { "+ Fila tudo" } else { "+ Queue All" };
                            if styled_compact_button(ui, queue_label, false, &palette).clicked() {
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
                            let save_label = if pt {
                                "Salvar como coleção"
                            } else {
                                "Save as Collection"
                            };
                            if styled_compact_button(ui, save_label, false, &palette).clicked() {
                                save_collection = true;
                            }
                        }

                        if styled_icon_chip_button(ui, true, "✕", &palette)
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
            if loading && count == 0 {
                let avail = ui.available_width();
                skeleton::home_grid_skeleton(ui, avail, 3, 16.0, dark_theme);
            }

            if close_banner {
                self.queue.remote_playlist_id = None;
                self.queue.remote_playlist_task.cancel();
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

        if self.ui.browse_section == BrowseSection::Playlists {
            return self.playlists_page_ui(ui, ctx, palette);
        }

        if self.ui.browse_section == BrowseSection::WatchLater {
            return self.watch_later_page_ui(ui, ctx, palette);
        }

        if self.ui.browse_section == BrowseSection::LikedVideos {
            return self.liked_videos_page_ui(ui, ctx, palette);
        }

        if self.ui.browse_section == BrowseSection::Shorts && !self.ui.search_results_mode {
            if let Some(first_short) = self.shorts_feed_candidates().first().map(|v| (*v).clone()) {
                return Some(first_short);
            }
            self.shorts_loading_stage_ui(ui, ctx, palette);
            return None;
        }

        if self.ui.browse_section == BrowseSection::Home
            && !matches!(self.feed.filter, FeedFilter::Shorts)
        {
            self.filter_chips_bar_ui(ui, ctx);
            ui.add_space(12.0);
        }

        self.browse_home_content_ui(ui, ctx, palette)
    }

    pub(crate) fn report_history_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let pt = self.ui_pt();
        let avail_w = ui.available_width().max(320.0);
        let max_w = avail_w.min(980.0);

        egui::ScrollArea::vertical()
            .id_salt("browse_report_history_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(max_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(max_w);
                        let compact = max_w < 680.0;
                        ui.horizontal_top(|ui| {
                            let left_w = if compact {
                                max_w
                            } else {
                                (max_w - 220.0).max(320.0)
                            };
                            ui.vertical(|ui| {
                                ui.set_max_width(left_w);
                                ui.label(
                                    RichText::new(if pt {
                                        "Agradecemos pela denúncia"
                                    } else {
                                        "Thanks for reporting"
                                    })
                                    .size(24.0)
                                    .strong()
                                    .color(palette.channel_fg),
                                );
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new(if pt {
                                        "Qualquer membro da comunidade do YouTube pode sinalizar conteúdo que pareça violar as diretrizes da comunidade. Quando um conteúdo é sinalizado, ele não é removido automaticamente. Primeiro, analisamos com base nas seguintes diretrizes:"
                                    } else {
                                        "Any member of the YouTube community can flag content that appears to violate Community Guidelines. Flagged content isn't automatically removed. First, we review it against these guidelines:"
                                    })
                                    .size(13.5)
                                    .color(palette.channel_fg),
                                );
                                ui.add_space(8.0);
                                let bullets = if pt {
                                    [
                                        "Vídeos que violam nossas diretrizes da comunidade são removidos do YouTube.",
                                        "Vídeos inadequados para o público mais jovem podem receber uma restrição de idade.",
                                        "Denúncias de vídeos removidos pelo criador de conteúdo não serão exibidas.",
                                    ]
                                } else {
                                    [
                                        "Videos that violate our Community Guidelines are removed from YouTube.",
                                        "Videos inappropriate for younger audiences may be age-restricted.",
                                        "Reports for videos removed by the creator are not shown here.",
                                    ]
                                };
                                for line in bullets {
                                    ui.horizontal_top(|ui| {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(format!("•  {line}"))
                                                .size(13.0)
                                                .color(palette.channel_fg),
                                        );
                                    });
                                }
                                ui.add_space(10.0);
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(if pt {
                                                "Saiba mais sobre como denunciar conteúdo no YouTube."
                                            } else {
                                                "Learn more about reporting content on YouTube."
                                            })
                                            .size(13.5)
                                            .color(palette.info_accent_fg),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    open_external_url(
                                        "https://support.google.com/youtube/answer/2802027",
                                    );
                                }
                            });
                            if !compact {
                                ui.add_space(24.0);
                                paint_report_history_hero_illustration(
                                    ui,
                                    egui::vec2(172.0, 128.0),
                                );
                            }
                        });

                        ui.add_space(18.0);
                        crate::library_ui::paint_section_hairline(ui, &palette);
                        ui.add_space(12.0);

                        let filter_labels = if pt {
                            ["Todos", "Ativo", "Removido"]
                        } else {
                            ["All", "Active", "Removed"]
                        };
                        let cur_idx = (self.feed.report_history_filter as usize).min(2);
                        egui::ComboBox::from_id_salt("report_history_filter_combo")
                            .selected_text(
                                RichText::new(filter_labels[cur_idx])
                                    .size(14.0)
                                    .strong(),
                            )
                            .show_ui(ui, |ui| {
                                for (idx, lbl) in filter_labels.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut self.feed.report_history_filter,
                                        idx as u8,
                                        *lbl,
                                    );
                                }
                            });

                        ui.add_space(14.0);
                        let table_w = ui.available_width();
                        let col_type = 92.0;
                        let col_status = 84.0;
                        let flex_w = (table_w - col_type - col_status - 24.0).max(200.0);
                        let col_content = (flex_w * 0.52).max(120.0);
                        let col_reason = (flex_w - col_content).max(100.0);

                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [col_type, 22.0],
                                egui::Label::new(
                                    RichText::new(if pt { "Tipo" } else { "Type" })
                                        .size(12.5)
                                        .color(palette.neutral_status_fg),
                                ),
                            );
                            ui.add_sized(
                                [col_content, 22.0],
                                egui::Label::new(
                                    RichText::new(if pt { "Conteúdo" } else { "Content" })
                                        .size(12.5)
                                        .color(palette.neutral_status_fg),
                                ),
                            );
                            ui.add_sized(
                                [col_reason, 22.0],
                                egui::Label::new(
                                    RichText::new(if pt {
                                        "Motivo da denúncia"
                                    } else {
                                        "Report reason"
                                    })
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                                ),
                            );
                            ui.add_sized(
                                [col_status, 22.0],
                                egui::Label::new(
                                    RichText::new("Status")
                                        .size(12.5)
                                        .color(palette.neutral_status_fg),
                                ),
                            );
                        });
                        ui.add_space(4.0);
                        crate::library_ui::paint_section_hairline(ui, &palette);

                        let filtered_entries: Vec<crate::app_state::ReportedContentEntry> = self
                            .feed
                            .report_history
                            .iter()
                            .rev()
                            .filter(|entry| match cur_idx {
                                1 => entry.status == "Ativo" || entry.status == "Active",
                                2 => entry.status != "Ativo" && entry.status != "Active",
                                _ => true,
                            })
                            .cloned()
                            .collect();

                        if filtered_entries.is_empty() {
                            ui.add_space(18.0);
                            ui.label(
                                RichText::new(if pt {
                                    "Nenhuma denúncia registrada ainda. Use o menu ⋮ -> Denunciar em qualquer vídeo."
                                } else {
                                    "No reports recorded yet. Use the ⋮ -> Report menu on any video."
                                })
                                .size(13.0)
                                .color(palette.neutral_status_fg),
                            );
                        } else {
                            let mut open_url: Option<String> = None;
                            for entry in &filtered_entries {
                                ui.add_space(10.0);
                                ui.horizontal_top(|ui| {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(col_type, 36.0),
                                        egui::Layout::left_to_right(egui::Align::Min),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!("🎬  {}", entry.kind))
                                                    .size(13.0)
                                                    .color(palette.channel_fg),
                                            );
                                        },
                                    );
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(col_content, 44.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_max_width(col_content - 8.0);
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(&entry.title)
                                                            .size(13.5)
                                                            .color(palette.info_accent_fg),
                                                    )
                                                    .frame(false),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                                && let Some(vid) = &entry.video_id
                                            {
                                                open_url = history::youtube_watch_url(vid);
                                            }
                                            ui.label(
                                                RichText::new(&entry.channel)
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                            );
                                        },
                                    );
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(col_reason, 44.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_max_width(col_reason - 8.0);
                                            ui.label(
                                                RichText::new(&entry.reason)
                                                    .size(13.0)
                                                    .color(palette.channel_fg),
                                            );
                                            ui.label(
                                                RichText::new(&entry.timestamp_label)
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                            );
                                        },
                                    );
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(col_status, 36.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.label(
                                                RichText::new(&entry.status)
                                                    .size(13.0)
                                                    .color(palette.channel_fg),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(8.0);
                                crate::library_ui::paint_section_hairline(ui, &palette);
                            }
                            if let Some(url) = open_url {
                                open_external_url(&url);
                            }
                        }
                    },
                );
            });
    }

    pub(crate) fn youtube_labs_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let pt = self.ui_pt();
        let avail_w = ui.available_width().max(320.0);
        let max_w = avail_w.min(980.0);

        egui::ScrollArea::vertical()
            .id_salt("browse_youtube_labs_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(max_w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(max_w);
                        ui.label(RichText::new("YouTube Labs").size(28.0).strong());
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(if pt {
                                "Teste recursos avançados de reprodução e privacidade sem anúncios ou assinaturas."
                            } else {
                                "Try advanced playback and privacy experiments without subscriptions."
                            })
                            .size(14.0)
                            .color(palette.neutral_status_fg),
                        );
                        ui.add_space(16.0);
                        crate::library_ui::paint_section_hairline(ui, &palette);
                        ui.add_space(16.0);

                        let mut prefs = self.settings.prefs.value.clone();
                        let mut changed = false;
                        let experiments = [
                            (
                                "SponsorBlock",
                                if pt {
                                    "Pular automaticamente patrocinadores, intros e outros segmentos da comunidade."
                                } else {
                                    "Automatically skip sponsors, intros, and other community segments."
                                },
                                prefs.sponsorblock.enabled,
                                0_u8,
                            ),
                            (
                                "Return YouTube Dislike",
                                if pt {
                                    "Exibir contagem estimada de \"Não gostei\" em vídeos assistidos."
                                } else {
                                    "Display community-estimated dislike counts while watching videos."
                                },
                                prefs.ryd_enabled,
                                1_u8,
                            ),
                            (
                                "DeArrow",
                                if pt {
                                    "Substituir títulos sensacionalistas e miniaturas por sugestões da comunidade."
                                } else {
                                    "Replace clickbait titles and thumbnails with community-sourced alternatives."
                                },
                                prefs.dearrow_enabled,
                                2_u8,
                            ),
                            (
                                if pt {
                                    "Miniplayer automático"
                                } else {
                                    "Auto mini-player"
                                },
                                if pt {
                                    "Fixar o vídeo no miniplayer interno no canto inferior direito ao voltar ao menu ou outra aba (sem minimizar o programa)."
                                } else {
                                    "Keep the active video docked in FastTube's bottom-right in-app miniplayer when returning to browse tabs without shrinking the window."
                                },
                                prefs.auto_mini_player,
                                3_u8,
                            ),
                        ];

                        for (title, desc, enabled, kind) in experiments {
                            egui::Frame::new()
                                .fill(palette.card_alt_bg)
                                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(14.0)
                                .inner_margin(egui::Margin::symmetric(18, 14))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.set_max_width((ui.available_width() - 150.0).max(160.0));
                                            ui.label(
                                                RichText::new(title)
                                                    .size(16.0)
                                                    .strong()
                                                    .color(palette.channel_fg),
                                            );
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(desc)
                                                    .size(13.0)
                                                    .color(palette.neutral_status_fg),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let btn_label = if enabled {
                                                    if pt { "Ativado" } else { "Enabled" }
                                                } else if pt {
                                                    "Experimentar"
                                                } else {
                                                    "Try it out"
                                                };
                                                if styled_pill_button(
                                                    ui,
                                                    btn_label,
                                                    enabled,
                                                    &palette,
                                                )
                                                .clicked()
                                                {
                                                    match kind {
                                                        0 => {
                                                            prefs.sponsorblock.enabled = !enabled
                                                        }
                                                        1 => prefs.ryd_enabled = !enabled,
                                                        2 => prefs.dearrow_enabled = !enabled,
                                                        _ => prefs.auto_mini_player = !enabled,
                                                    }
                                                    changed = true;
                                                }
                                            },
                                        );
                                    });
                                });
                            ui.add_space(10.0);
                        }
                        if changed {
                            self.apply_app_preferences(prefs);
                        }
                    },
                );
            });
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
                self.feed.home.stream_task.is_running()
            } else {
                self.followed_channels.feed_task.is_running()
            };
            let has_local_followed = !self.followed_channels.data.channels.is_empty();
            let mut refresh_clicked = false;
            let title = if pt { "Mais recentes" } else { "Latest" };
            let subtitle = if self.auth.authenticated || has_local_followed {
                None
            } else if pt {
                Some(
                    "Siga canais na página do canal (sem login) ou conecte sua conta nas Configurações.",
                )
            } else {
                Some(
                    "Follow channels on any channel page (no login required) or sign in from Settings.",
                )
            };
            let auth_authenticated = self.auth.authenticated;
            let subs_cached_loaded = self.feed.home.subscriptions_cached_loaded;
            let subs_cached_at = self.feed.home.subscriptions_cached_at;
            let local_loaded_at = self.followed_channels.loaded_at;
            let mut list_view = self.ui.subscriptions_list_view;
            let mut open_manage = false;
            let mut toggle_filters = false;
            let filters_active = self.ui.subscriptions_filters_open
                || self.feed_preferences.values.hide_watched
                || self.feed_preferences.values.hide_shorts
                || self.feed_preferences.values.hide_lives
                || self.feed_preferences.values.channel.is_some();

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(title)
                            .size(20.0)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    if let Some(sub) = subtitle {
                        ui.label(
                            RichText::new(sub)
                                .size(13.0)
                                .color(palette.neutral_status_fg),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let manage_label = if pt {
                        "Todas as inscri\u{00e7}\u{00f5}es"
                    } else {
                        "All subscriptions"
                    };
                    let manage_fill = if self.ui.manage_subscriptions_open {
                        palette.active_button_bg
                    } else {
                        palette.header_button_bg
                    };
                    let manage_fg = if self.ui.manage_subscriptions_open {
                        palette.active_button_fg
                    } else {
                        palette.header_button_fg
                    };
                    let manage_resp = ui
                        .scope(|ui| {
                            ui.spacing_mut().button_padding = egui::vec2(14.0, 7.0);
                            let manage_btn = egui::Button::new(
                                RichText::new(manage_label)
                                    .size(13.5)
                                    .strong()
                                    .color(manage_fg),
                            )
                            .fill(manage_fill)
                            .corner_radius(18)
                            .min_size(egui::vec2(0.0, 32.0));
                            ui.add(manage_btn)
                        })
                        .inner
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(if pt {
                            "Gerenciar canais inscritos (/feed/channels)"
                        } else {
                            "Manage subscribed channels (/feed/channels)"
                        });
                    paint_focus_ring_if_focused(ui, &manage_resp, 18.0);
                    if manage_resp.clicked() {
                        open_manage = true;
                    }

                    let filter_tip = if pt { "Filtros" } else { "Filters" };
                    let (filter_rect, filter_resp) =
                        ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
                    let filter_bg = if filters_active {
                        palette.active_button_bg
                    } else if filter_resp.hovered() {
                        palette.header_button_bg
                    } else {
                        Color32::TRANSPARENT
                    };
                    if filter_bg != Color32::TRANSPARENT {
                        ui.painter()
                            .circle_filled(filter_rect.center(), 15.0, filter_bg);
                    }
                    paint_filter_sliders_icon(
                        ui.painter(),
                        filter_rect.center(),
                        if filters_active {
                            palette.active_button_fg
                        } else {
                            palette.header_button_fg
                        },
                    );
                    paint_focus_ring_if_focused(ui, &filter_resp, 15.0);
                    if filter_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(filter_tip)
                        .clicked()
                    {
                        toggle_filters = true;
                    }

                    if auth_authenticated || has_local_followed {
                        let status_tip = if is_refreshing {
                            if pt {
                                "Atualizando inscrições...".to_owned()
                            } else {
                                "Refreshing subscriptions...".to_owned()
                            }
                        } else if auth_authenticated
                            && subs_cached_loaded
                            && let Some(cached_at) = subs_cached_at
                        {
                            let mins = cached_at.elapsed().as_secs() / 60;
                            if mins == 0 {
                                if pt {
                                    "Atualizar inscrições • Atualizado agora".to_owned()
                                } else {
                                    "Refresh subscriptions • Updated just now".to_owned()
                                }
                            } else if pt {
                                format!("Atualizar inscrições • há {mins} min")
                            } else {
                                format!("Refresh subscriptions • {mins} min ago")
                            }
                        } else if !auth_authenticated && let Some(loaded_at) = local_loaded_at {
                            let mins = loaded_at.elapsed().as_secs() / 60;
                            if mins == 0 {
                                if pt {
                                    "Atualizar RSS • Atualizado agora".to_owned()
                                } else {
                                    "Refresh RSS • Updated just now".to_owned()
                                }
                            } else if pt {
                                format!("Atualizar RSS • há {mins} min")
                            } else {
                                format!("Refresh RSS • {mins}m ago")
                            }
                        } else if pt {
                            "Atualizar inscrições".to_owned()
                        } else {
                            "Refresh subscriptions".to_owned()
                        };
                        let glyph = if is_refreshing { "..." } else { "\u{21bb}" };
                        let (ref_rect, ref_resp) = ui.allocate_exact_size(
                            egui::vec2(30.0, 30.0),
                            if is_refreshing {
                                egui::Sense::hover()
                            } else {
                                egui::Sense::click()
                            },
                        );
                        if ref_resp.hovered() && !is_refreshing {
                            ui.painter().circle_filled(
                                ref_rect.center(),
                                15.0,
                                palette.header_button_bg,
                            );
                        }
                        ui.painter().text(
                            ref_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            glyph,
                            egui::FontId::proportional(14.5),
                            if is_refreshing {
                                palette.neutral_status_fg
                            } else {
                                palette.header_button_fg
                            },
                        );
                        paint_focus_ring_if_focused(ui, &ref_resp, 15.0);
                        let ref_resp = ref_resp.on_hover_text(status_tip);
                        if !is_refreshing
                            && ref_resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                        {
                            refresh_clicked = true;
                        }
                    }

                    let list_tip = if pt {
                        "Visualização em lista"
                    } else {
                        "List view"
                    };
                    let (list_rect, list_resp) =
                        ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
                    let list_bg = if list_view {
                        palette.active_button_bg
                    } else if list_resp.hovered() {
                        palette.header_button_bg
                    } else {
                        Color32::TRANSPARENT
                    };
                    if list_bg != Color32::TRANSPARENT {
                        ui.painter()
                            .circle_filled(list_rect.center(), 15.0, list_bg);
                    }
                    paint_list_view_icon(
                        ui.painter(),
                        list_rect.center(),
                        if list_view {
                            palette.active_button_fg
                        } else {
                            palette.header_button_fg
                        },
                    );
                    paint_focus_ring_if_focused(ui, &list_resp, 15.0);
                    if list_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(list_tip)
                        .clicked()
                    {
                        list_view = true;
                    }

                    let grid_tip = if pt {
                        "Visualização em grade"
                    } else {
                        "Grid view"
                    };
                    let (grid_rect, grid_resp) =
                        ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::click());
                    let grid_bg = if !list_view {
                        palette.active_button_bg
                    } else if grid_resp.hovered() {
                        palette.header_button_bg
                    } else {
                        Color32::TRANSPARENT
                    };
                    if grid_bg != Color32::TRANSPARENT {
                        ui.painter()
                            .circle_filled(grid_rect.center(), 15.0, grid_bg);
                    }
                    paint_grid_view_icon(
                        ui.painter(),
                        grid_rect.center(),
                        if !list_view {
                            palette.active_button_fg
                        } else {
                            palette.header_button_fg
                        },
                    );
                    paint_focus_ring_if_focused(ui, &grid_resp, 15.0);
                    if grid_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(grid_tip)
                        .clicked()
                    {
                        list_view = false;
                    }
                });
            });

            if !self.auth.authenticated
                && has_local_followed
                && (self.ui.subscriptions_filters_open || self.ui.manage_subscriptions_open)
            {
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

            ui.add_space(10.0);
            self.ui.subscriptions_list_view = list_view;
            if open_manage {
                self.ui.manage_subscriptions_open = !self.ui.manage_subscriptions_open;
            }
            if toggle_filters {
                self.ui.subscriptions_filters_open = !self.ui.subscriptions_filters_open;
            }
            if refresh_clicked {
                if self.auth.authenticated {
                    self.open_subscriptions_section(true, ctx);
                } else {
                    self.start_local_followed_feed(true, ctx);
                }
            }
        }
    }

    pub(crate) fn home_skeleton_grid_ui(&self, ui: &mut egui::Ui, _palette: ThemePalette) {
        let dark_theme = self.is_dark_theme();
        let available = ui.available_width().max(220.0);
        if self.ui.browse_section == BrowseSection::Shorts
            || matches!(self.feed.filter, FeedFilter::Shorts)
        {
            skeleton::shorts_grid_skeleton(ui, available, dark_theme);
        } else {
            let (gap, columns) = if is_subscriptions_browse_section(self.ui.browse_section) {
                self.subscriptions_grid_metrics(available)
            } else {
                self.home_grid_metrics(available)
            };
            skeleton::home_grid_skeleton(ui, available, columns, gap, dark_theme);
        }
    }

    pub(crate) fn browse_home_content_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> Option<Video> {
        let is_fast_home_loading = self.feed.videos.is_empty()
            && (self.feed.home.batch_task.is_running()
                || self.feed.home_innertube_task.is_running()
                || self.feed.search.task.is_running()
                || self.feed.search_page_task.is_running()
                || (self.ui.browse_section == BrowseSection::Subscriptions
                    && (self.feed.home.stream_task.is_running()
                        || self.feed.subs_innertube_task.is_running()
                        || self.followed_channels.feed_task.is_running())));
        if !is_fast_home_loading
            && self.feed.videos.is_empty()
            && is_actionable_inline_message(&self.message)
        {
            if inline_error_banner_ui(ui, &self.message, &palette) {
                self.message.clear();
            }
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
            let pt = self.ui_pt();
            let has_followed =
                self.auth.authenticated || !self.followed_channels.data.channels.is_empty();
            let (title, subtitle, cta) = if has_followed {
                if pt {
                    (
                        "Nenhum envio recente carregado",
                        "Atualize as inscrições para buscar os vídeos mais recentes dos canais acompanhados.",
                        "Atualizar agora",
                    )
                } else {
                    (
                        "No recent uploads loaded",
                        "Refresh subscriptions to fetch the latest videos from channels you follow.",
                        "Refresh now",
                    )
                }
            } else if pt {
                (
                    "Nenhuma inscrição ativa ainda",
                    "Siga canais localmente na página do canal (via RSS, sem login) ou entre com Google nas Configurações.",
                    "Explorar vídeos",
                )
            } else {
                (
                    "No active subscriptions yet",
                    "Follow channels locally from any channel page (via RSS, no sign-in) or sign in with Google in Settings.",
                    "Explore videos",
                )
            };
            if empty_state_ui(ui, Some(Icon::Play), title, subtitle, Some(cta), &palette) {
                if has_followed {
                    if self.auth.authenticated {
                        self.open_subscriptions_section(true, ctx);
                    } else {
                        self.start_local_followed_feed(true, ctx);
                    }
                } else {
                    self.handle_header_home(ctx);
                }
            }
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

    pub(crate) fn shorts_loading_stage_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let dark_theme = self.is_dark_theme();
        let avail_w = ui.available_width().max(320.0);
        let avail_h = ui.available_height().max(440.0);
        let rail_w = 76.0_f32;
        let rail_gap = 16.0_f32;
        let stage_avail_h = (avail_h - 24.0).max(380.0);
        let max_stage_w = (avail_w - rail_w - rail_gap - 80.0).max(220.0);
        let stage_size = layout::compute_player_size_with_theater(
            max_stage_w,
            stage_avail_h,
            false,
            false,
            layout::WatchLayoutMode::Short,
        );
        let cluster_w = stage_size.x + rail_gap + rail_w;
        let left_offset = ((avail_w - cluster_w) * 0.5).max(12.0);
        let top_pad = ((avail_h - stage_size.y) * 0.5).clamp(8.0, 20.0);

        let is_loading = self.feed.shorts_task.is_running()
            || self.feed.home.batch_task.is_running()
            || self.feed.home.stream_task.is_running()
            || self.feed.home_innertube_task.is_running()
            || self.feed.search.task.is_running()
            || self.feed.search_page_task.is_running();

        ui.vertical(|ui| {
            ui.add_space(top_pad);
            ui.horizontal_top(|ui| {
                ui.add_space(left_offset);
                let stage_rect = skeleton::skeleton_box(ui, stage_size, 14.0, dark_theme);
                ui.add_space(rail_gap);
                ui.vertical(|ui| {
                    ui.set_width(rail_w);
                    let rail_pad = (stage_size.y - 252.0).max(24.0);
                    ui.add_space(rail_pad);
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 14.0;
                        for _ in 0..3 {
                            skeleton::skeleton_circle(ui, 48.0, dark_theme);
                            skeleton::skeleton_text_line(ui, 36.0, 10.0, dark_theme);
                        }
                        let _ =
                            skeleton::skeleton_box(ui, egui::vec2(40.0, 40.0), 10.0, dark_theme);
                    });
                });

                if !is_loading && stage_rect.width() > 140.0 {
                    let pt = self.ui_pt();
                    let overlay_rect = stage_rect.shrink(20.0);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(overlay_rect), |ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.add_space((overlay_rect.height() * 0.38).max(24.0));
                            ui.label(
                                RichText::new(if pt {
                                    "Carregando Shorts..."
                                } else {
                                    "Loading Shorts..."
                                })
                                .size(15.0)
                                .strong()
                                .color(palette.channel_fg),
                            );
                            ui.add_space(10.0);
                            if styled_pill_button(
                                ui,
                                if pt { "Tentar novamente" } else { "Retry" },
                                true,
                                &palette,
                            )
                            .clicked()
                            {
                                self.start_fast_home_feed(false, ctx);
                                self.start_shorts_sequence_fetch(false, ctx);
                            }
                        });
                    });
                }
            });
        });
    }

    pub(crate) fn browse_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        if !self.channels.view_open
            && !self.ui.search_results_mode
            && (self.ui.browse_section == BrowseSection::History
                || self.ui.browse_section == BrowseSection::Shorts)
        {
            return self.browse_ui(ui, ctx);
        }
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

fn format_report_date_today(pt: bool) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Howard Hinnant's civil_from_days algorithm (UTC calendar day).
    let z = secs.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    let pt_months = [
        "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.",
        "dez.",
    ];
    let en_months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let idx = (m as usize).saturating_sub(1).min(11);
    if pt {
        format!("{d} de {} de {year}", pt_months[idx])
    } else {
        format!("{} {d}, {year}", en_months[idx])
    }
}

pub(crate) fn count_active_search_filters(filters: &search::SearchFilters) -> usize {
    let mut count = 0;
    if filters.upload_date != crate::innertube::SearchUploadDate::Any {
        count += 1;
    }
    if filters.result_type != crate::innertube::SearchResultType::Any {
        count += 1;
    }
    if filters.duration != crate::innertube::SearchDuration::Any {
        count += 1;
    }
    count += filters.features.len();
    if filters.sort != search::SearchSort::Relevance {
        count += 1;
    }
    count
}

pub(crate) fn toggle_search_feature(
    features: &mut Vec<crate::innertube::SearchFeature>,
    feature: crate::innertube::SearchFeature,
) {
    if let Some(pos) = features.iter().position(|f| *f == feature) {
        features.remove(pos);
    } else {
        features.push(feature);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SearchSpellingBannerCopy {
    pub primary_prefix: String,
    pub primary_query: String,
    pub secondary_prefix: Option<String>,
    pub secondary_query: Option<String>,
    pub is_auto_corrected: bool,
}

pub(crate) fn format_search_spelling_banner(
    spelling: &crate::innertube::SearchSpellingCorrection,
    pt: bool,
) -> Option<SearchSpellingBannerCopy> {
    let corrected = spelling.corrected_query.trim();
    if corrected.is_empty() {
        return None;
    }
    if spelling.is_auto_corrected {
        let original = spelling
            .original_query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_owned);
        Some(SearchSpellingBannerCopy {
            primary_prefix: if pt {
                "Mostrando resultados para".to_owned()
            } else {
                "Showing results for".to_owned()
            },
            primary_query: corrected.to_owned(),
            secondary_prefix: original.as_ref().map(|_| {
                if pt {
                    "Pesquisar em vez disso por".to_owned()
                } else {
                    "Search instead for".to_owned()
                }
            }),
            secondary_query: original,
            is_auto_corrected: true,
        })
    } else {
        Some(SearchSpellingBannerCopy {
            primary_prefix: if pt {
                "Voc\u{00ea} quis dizer:".to_owned()
            } else {
                "Did you mean:".to_owned()
            },
            primary_query: corrected.to_owned(),
            secondary_prefix: None,
            secondary_query: None,
            is_auto_corrected: false,
        })
    }
}

fn search_chip_label(chip: search::SearchChip, pt: bool) -> &'static str {
    match chip {
        search::SearchChip::All => {
            if pt {
                "Tudo"
            } else {
                "All"
            }
        }
        search::SearchChip::Shorts => "Shorts",
        search::SearchChip::Unwatched => {
            if pt {
                "N\u{00e3}o assistidos"
            } else {
                "Unwatched"
            }
        }
        search::SearchChip::Videos => {
            if pt {
                "V\u{00ed}deos"
            } else {
                "Videos"
            }
        }
        search::SearchChip::RecentlyUploaded => {
            if pt {
                "Enviados recentemente"
            } else {
                "Recently uploaded"
            }
        }
        search::SearchChip::Live => {
            if pt {
                "Ao vivo"
            } else {
                "Live"
            }
        }
    }
}

fn search_filters_pill_button_ui(
    ui: &mut egui::Ui,
    open: bool,
    active_count: usize,
    pt: bool,
    palette: &ThemePalette,
) -> egui::Response {
    let active = active_count > 0;
    let label = if pt { "Filtros" } else { "Filters" };
    let desired_w = if active { 118.0 } else { 96.0 };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(desired_w, 32.0), egui::Sense::click());
    let fill = if active {
        palette.active_button_bg
    } else if open || response.hovered() {
        palette.header_button_bg
    } else {
        Color32::TRANSPARENT
    };
    let fg = if active {
        palette.active_button_fg
    } else {
        palette.header_button_fg
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 16.0, fill);
    }

    let painter = ui.painter();
    painter.text(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        fg,
    );

    let icon_x = if active {
        let badge_center = egui::pos2(rect.right() - 18.0, rect.center().y);
        painter.circle_filled(badge_center, 9.5, palette.header_button_bg);
        painter.text(
            badge_center,
            egui::Align2::CENTER_CENTER,
            active_count.to_string(),
            egui::FontId::proportional(11.0),
            palette.header_button_fg,
        );
        rect.right() - 40.0
    } else {
        rect.right() - 20.0
    };

    let icon_c = egui::pos2(icon_x, rect.center().y);
    let stroke = egui::Stroke::new(1.5, fg);
    let top_y = icon_c.y - 3.5;
    let bot_y = icon_c.y + 3.5;
    painter.line_segment(
        [
            egui::pos2(icon_c.x - 6.5, top_y),
            egui::pos2(icon_c.x + 6.5, top_y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(icon_c.x - 6.5, bot_y),
            egui::pos2(icon_c.x + 6.5, bot_y),
        ],
        stroke,
    );
    let knob_bg = if fill != Color32::TRANSPARENT {
        fill
    } else {
        palette.card_bg
    };
    painter.circle_filled(egui::pos2(icon_c.x + 2.2, top_y), 2.3, knob_bg);
    painter.circle_stroke(egui::pos2(icon_c.x + 2.2, top_y), 2.3, stroke);
    painter.circle_filled(egui::pos2(icon_c.x - 2.2, bot_y), 2.3, knob_bg);
    painter.circle_stroke(egui::pos2(icon_c.x - 2.2, bot_y), 2.3, stroke);

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn feed_load_more_footer_ui(
    ui: &mut egui::Ui,
    loading_more: bool,
    pt: bool,
    palette: &ThemePalette,
) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        if loading_more {
            ui.horizontal(|ui| {
                skeleton::visible_spinner(ui);
                ui.add_space(6.0);
                ui.label(
                    RichText::new(if pt {
                        "Carregando mais..."
                    } else {
                        "Loading more..."
                    })
                    .size(13.0)
                    .color(palette.neutral_status_fg),
                );
            });
        } else {
            let btn = egui::Button::new(
                RichText::new(if pt { "Carregar mais" } else { "Load more" })
                    .size(13.5)
                    .strong()
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .min_size(egui::vec2(144.0, 36.0))
            .corner_radius(18);
            if ui
                .add(btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                clicked = true;
            }
        }
    });
    clicked
}

fn is_subscriptions_browse_section(section: BrowseSection) -> bool {
    section == BrowseSection::Subscriptions
}

fn is_channel_verified(metadata: Option<&VideoMetadata>) -> bool {
    metadata.is_some_and(|m| m.channel_verified)
}

fn channel_matches_video(
    channel_id: &str,
    channel_title: &str,
    video: &Video,
    metadata: Option<&VideoMetadata>,
) -> bool {
    let id = channel_id.trim();
    if !id.is_empty()
        && let Some(meta_id) = metadata
            .and_then(|m| m.channel_id.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        return meta_id.eq_ignore_ascii_case(id);
    }
    let title_key = recommendations::normalize_channel_key(channel_title);
    !title_key.is_empty() && recommendations::normalize_channel_key(&video.channel) == title_key
}

fn sidebar_channel_status(
    channel_id: &str,
    channel_title: &str,
    new_video_urls: &std::collections::HashSet<String>,
    sources: &[&[SearchResult]],
    feed_videos: &[Video],
    video_metadata: &std::collections::HashMap<String, VideoMetadata>,
) -> (bool, bool) {
    let mut has_unseen = false;
    let mut is_live = false;
    for items in sources {
        for item in *items {
            if !channel_matches_video(channel_id, channel_title, &item.video, Some(&item.metadata))
            {
                continue;
            }
            if !has_unseen && new_video_urls.contains(&item.video.url) {
                has_unseen = true;
            }
            if !is_live && recommendations::is_live_video(&item.video, Some(&item.metadata)) {
                is_live = true;
            }
            if has_unseen && is_live {
                return (true, true);
            }
        }
    }
    for video in feed_videos {
        let meta = video_metadata.get(&video.url);
        if !channel_matches_video(channel_id, channel_title, video, meta) {
            continue;
        }
        if !has_unseen && new_video_urls.contains(&video.url) {
            has_unseen = true;
        }
        if !is_live && recommendations::is_live_video(video, meta) {
            is_live = true;
        }
        if has_unseen && is_live {
            return (true, true);
        }
    }
    (has_unseen, is_live)
}

pub(crate) const CARD_HOVER_SCRUB_DELAY_MS: u64 = 400;

pub(crate) fn compute_card_hover_scrub_ratio(
    hover_elapsed_ms: u64,
    pointer_x: Option<f32>,
    thumb_left: f32,
    thumb_right: f32,
) -> Option<f32> {
    if hover_elapsed_ms < CARD_HOVER_SCRUB_DELAY_MS {
        return None;
    }
    let width = thumb_right - thumb_left;
    if let Some(px) = pointer_x
        && width > 1.0
    {
        return Some(((px - thumb_left) / width).clamp(0.0, 1.0));
    }
    let cycle_ms = (hover_elapsed_ms - CARD_HOVER_SCRUB_DELAY_MS) % 4000;
    Some((cycle_ms as f32 / 4000.0).clamp(0.0, 1.0))
}

pub(crate) fn parse_duration_text_secs(duration_text: &str) -> Option<u64> {
    let trimmed = duration_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.as_slice() {
        [m, s] => {
            let mins: u64 = m.trim().parse().ok()?;
            let secs: u64 = s.trim().parse().ok()?;
            (secs < 60).then(|| mins * 60 + secs)
        }
        [h, m, s] => {
            let hours: u64 = h.trim().parse().ok()?;
            let mins: u64 = m.trim().parse().ok()?;
            let secs: u64 = s.trim().parse().ok()?;
            (mins < 60 && secs < 60).then(|| hours * 3600 + mins * 60 + secs)
        }
        _ => None,
    }
}

pub(crate) fn format_hover_scrub_timestamp(ratio: f32, duration_text: &str) -> Option<String> {
    let total_secs = parse_duration_text_secs(duration_text)?;
    if total_secs == 0 {
        return None;
    }
    let clamped = ratio.clamp(0.0, 1.0);
    let target_secs = ((total_secs as f32) * clamped).round() as u64;
    Some(format!(
        "{} / {}",
        format_time(target_secs as f64),
        duration_text.trim()
    ))
}

pub(crate) fn update_and_paint_card_hover_scrub(
    ui: &egui::Ui,
    ctx: &egui::Context,
    card_id: &str,
    thumb_rect: egui::Rect,
    hovered: bool,
    duration_text: &str,
) {
    let hover_key = egui::Id::new(("video_card_hover_scrub_start", card_id));
    if !hovered {
        ctx.data_mut(|d| {
            d.remove::<f64>(hover_key);
        });
        return;
    }

    let now = ctx.input(|i| i.time);
    let start = ctx.data_mut(|d| *d.get_temp_mut_or(hover_key, now));
    let elapsed_ms = ((now - start).max(0.0) * 1000.0).round() as u64;
    if elapsed_ms < CARD_HOVER_SCRUB_DELAY_MS {
        let remain_ms = (CARD_HOVER_SCRUB_DELAY_MS - elapsed_ms).max(16);
        ctx.request_repaint_after(Duration::from_millis(remain_ms));
        return;
    }

    let pointer_x = ui
        .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()))
        .map(|p| p.x);
    let Some(ratio) = compute_card_hover_scrub_ratio(
        elapsed_ms,
        pointer_x,
        thumb_rect.left(),
        thumb_rect.right(),
    ) else {
        return;
    };

    let track_rect = egui::Rect::from_min_max(
        egui::pos2(
            thumb_rect.left(),
            (thumb_rect.bottom() - 3.5).max(thumb_rect.top()),
        ),
        thumb_rect.max,
    );
    ui.painter().rect_filled(
        track_rect,
        0.0,
        Color32::from_rgba_unmultiplied(15, 15, 15, 185),
    );
    let fill_w = (thumb_rect.width() * ratio).clamp(2.0, thumb_rect.width());
    let fill_rect =
        egui::Rect::from_min_size(track_rect.min, egui::vec2(fill_w, track_rect.height()));
    let red = Color32::from_rgb(255, 0, 51);
    ui.painter().rect_filled(fill_rect, 0.0, red);

    for step in 1..10 {
        let tick_x = thumb_rect.left() + thumb_rect.width() * (step as f32 / 10.0);
        ui.painter().line_segment(
            [
                egui::pos2(tick_x, track_rect.top()),
                egui::pos2(tick_x, track_rect.bottom()),
            ],
            egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120)),
        );
    }

    let knob_x =
        (thumb_rect.left() + fill_w).clamp(thumb_rect.left() + 3.0, thumb_rect.right() - 3.0);
    ui.painter()
        .circle_filled(egui::pos2(knob_x, track_rect.center().y), 3.6, red);

    if let Some(ts_label) = format_hover_scrub_timestamp(ratio, duration_text) {
        let pill_w = (ts_label.len() as f32 * 6.2 + 12.0).clamp(58.0, 128.0);
        let pill_rect = egui::Rect::from_min_size(
            egui::pos2(
                (knob_x - pill_w * 0.5).clamp(
                    thumb_rect.left() + 6.0,
                    (thumb_rect.right() - pill_w - 6.0).max(thumb_rect.left() + 6.0),
                ),
                (track_rect.top() - 22.0).max(thumb_rect.top() + 6.0),
            ),
            egui::vec2(pill_w, 18.0),
        );
        ui.painter().rect_filled(
            pill_rect,
            4.0,
            Color32::from_rgba_unmultiplied(15, 15, 15, 228),
        );
        ui.painter().text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            ts_label,
            egui::FontId::proportional(10.5),
            Color32::WHITE,
        );
    }
}

pub(crate) fn is_video_currently_live(video: &Video, metadata: Option<&VideoMetadata>) -> bool {
    if metadata.is_some_and(|m| {
        m.badges.iter().any(|b| {
            b.eq_ignore_ascii_case("upcoming")
                || b.eq_ignore_ascii_case("premiere")
                || b.eq_ignore_ascii_case("estreia")
        })
    }) {
        return false;
    }
    if metadata.is_some_and(|m| {
        m.is_live
            || m.badges
                .iter()
                .any(|b| b.eq_ignore_ascii_case("ao vivo") || b.eq_ignore_ascii_case("live"))
    }) {
        return true;
    }
    let dur = video.duration.trim();
    dur.eq_ignore_ascii_case("ao vivo") || dur.eq_ignore_ascii_case("live")
}

pub(crate) fn filter_live_now_subscription_indices<'a>(
    ranked: &'a [ScoredVideo],
    candidate_indices: &[usize],
    mut lookup_meta: impl FnMut(&'a str) -> Option<&'a VideoMetadata>,
) -> Vec<usize> {
    candidate_indices
        .iter()
        .copied()
        .filter(|&idx| {
            ranked.get(idx).is_some_and(|scored| {
                is_video_currently_live(&scored.video, lookup_meta(&scored.video.url))
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManageSubscriptionEntry {
    pub(crate) channel_id: String,
    pub(crate) title: String,
    pub(crate) handle: Option<String>,
    pub(crate) url: String,
    pub(crate) avatar_url: Option<String>,
    pub(crate) is_rss_followed: bool,
    pub(crate) is_remote_subscribed: bool,
}

pub(crate) fn filter_manage_subscriptions_channels<'a>(
    entries: &'a [ManageSubscriptionEntry],
    query: &str,
) -> Vec<&'a ManageSubscriptionEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return entries.iter().collect();
    }
    entries
        .iter()
        .filter(|entry| {
            entry.title.to_lowercase().contains(&q)
                || entry.channel_id.to_lowercase().contains(&q)
                || entry
                    .handle
                    .as_deref()
                    .is_some_and(|h| h.to_lowercase().contains(&q))
        })
        .collect()
}

impl FastTube {
    pub(crate) fn subscriptions_live_now_shelf_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        ranked: &[ScoredVideo],
        live_indices: &[usize],
        available_width: f32,
    ) -> Option<Video> {
        if live_indices.is_empty() {
            return None;
        }
        let pt = self.ui_pt();
        let palette = self.palette();
        let pulse = ((ctx.input(|i| i.time) * 3.2).sin() * 0.5 + 0.5) as f32;
        ctx.request_repaint_after(Duration::from_millis(80));

        let mut selected = None;
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(12.0)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let pill_text = if pt { "AO VIVO" } else { "LIVE" };
                    let pill_w = if pt { 74.0 } else { 58.0 };
                    let (pill_rect, _) =
                        ui.allocate_exact_size(egui::vec2(pill_w, 22.0), egui::Sense::hover());
                    let red_alpha = (195.0 + 60.0 * pulse).round().clamp(0.0, 255.0) as u8;
                    ui.painter().rect_filled(
                        pill_rect,
                        11.0,
                        Color32::from_rgba_unmultiplied(224, 0, 36, red_alpha),
                    );
                    let dot_center = egui::pos2(pill_rect.left() + 11.0, pill_rect.center().y);
                    ui.painter()
                        .circle_filled(dot_center, 3.2 + 0.8 * pulse, Color32::WHITE);
                    ui.painter().text(
                        egui::pos2(pill_rect.left() + 20.0, pill_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        pill_text,
                        egui::FontId::proportional(11.0),
                        Color32::WHITE,
                    );

                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(if pt {
                            "Transmissões ao vivo das suas inscrições"
                        } else {
                            "Live now from your subscriptions"
                        })
                        .size(15.0)
                        .strong()
                        .color(palette.channel_fg),
                    );
                });

                ui.add_space(10.0);
                let card_w = (available_width * 0.28).clamp(210.0, 280.0);
                egui::ScrollArea::horizontal()
                    .id_salt("subscriptions_live_now_shelf_scroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            for &idx in live_indices.iter().take(12) {
                                let scored = &ranked[idx];
                                let video = &scored.video;
                                ui.push_id(("sub_live_now_card", &video.url), |ui| {
                                    let act = self.youtube_grid_card_ui(ui, ctx, scored, card_w);
                                    if act.open_clicked {
                                        selected = Some(video.clone());
                                    }
                                });
                            }
                        });
                    });
            });
        selected
    }

    pub(crate) fn notifications_inbox_panel_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pt = self.ui_pt();
        let palette = self.palette();
        ui.set_min_width(420.0);
        ui.set_max_width(480.0);

        let mut merged: Vec<innertube::InnerTubeNotification> = self.feed.notifications.clone();
        let existing_urls: std::collections::HashSet<String> =
            merged.iter().filter_map(|n| n.video_url.clone()).collect();

        for url in &self.followed_channels.new_video_urls {
            if existing_urls.contains(url) {
                continue;
            }
            if let Some(video) = self.feed.videos.iter().find(|v| &v.url == url) {
                let meta = self.metadata_for(&video.url);
                let avatar = meta.and_then(|m| m.channel_avatar_url.clone()).or_else(|| {
                    self.lookup_channel_avatar(&video.channel)
                        .map(str::to_owned)
                });
                let published_text = meta
                    .and_then(|m| m.published_time_text.clone())
                    .unwrap_or_else(|| {
                        if pt {
                            "Novo envio (RSS)".to_owned()
                        } else {
                            "New upload (RSS)".to_owned()
                        }
                    });
                merged.push(innertube::InnerTubeNotification {
                    id: format!("rss:{url}"),
                    title: video.title.clone(),
                    channel: video.channel.clone(),
                    thumbnail_url: (!video.thumbnail.trim().is_empty())
                        .then(|| video.thumbnail.clone()),
                    channel_avatar_url: avatar,
                    video_url: Some(video.url.clone()),
                    published_text,
                    is_read: false,
                    mark_read_token: None,
                    ..Default::default()
                });
            }
        }

        let unread_count = merged.iter().filter(|n| !n.is_read).count();
        let mut mark_all_clicked = false;
        let mut refresh_clicked = false;
        let mut open_settings_clicked = false;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if pt {
                    "Notificações"
                } else {
                    "Notifications"
                })
                .size(16.0)
                .strong()
                .color(palette.channel_fg),
            );
            if unread_count > 0 {
                ui.label(
                    RichText::new(format!("({unread_count})"))
                        .size(13.0)
                        .strong()
                        .color(palette.info_accent_fg),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (gear_rect, gear_resp) =
                    ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
                if gear_resp.hovered() {
                    ui.painter()
                        .circle_filled(gear_rect.center(), 14.0, palette.header_button_bg);
                }
                paint_settings_gear_icon(
                    ui.painter(),
                    gear_rect.center(),
                    palette.header_button_fg,
                );
                paint_focus_ring_if_focused(ui, &gear_resp, 14.0);
                if gear_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Configurações de notificações"
                    } else {
                        "Notification settings"
                    })
                    .clicked()
                {
                    open_settings_clicked = true;
                }
                if styled_icon_chip_button(ui, !self.feed.notifications_loading, "↻", &palette)
                    .on_hover_text(if pt {
                        "Atualizar notificações"
                    } else {
                        "Refresh notifications"
                    })
                    .clicked()
                {
                    refresh_clicked = true;
                }
                if unread_count > 0
                    && styled_compact_button(
                        ui,
                        if pt {
                            "Marcar todas como lidas"
                        } else {
                            "Mark all as read"
                        },
                        false,
                        &palette,
                    )
                    .clicked()
                {
                    mark_all_clicked = true;
                }
            });
        });

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        if open_settings_clicked {
            if !self.ui.settings_open && self.selected.is_some() {
                let _ = self.maybe_enter_auto_mini_player(ctx);
            }
            self.ui.settings_open = true;
            self.settings.active_tab = app_state::SettingsTab::General;
            self.sync_player_window_visibility();
            ui.close();
            return;
        }
        if refresh_clicked {
            self.start_notifications_inbox_fetch(false, None, ctx);
        }
        if mark_all_clicked {
            self.mark_all_notifications_read(ctx);
            for item in &mut merged {
                item.is_read = true;
            }
        }

        if self.feed.notifications_loading && merged.is_empty() {
            let dark_theme = self.is_dark_theme();
            skeleton::transcript_skeleton(ui, 420.0, dark_theme);
            ui.add_space(6.0);
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(if pt {
                        "Carregando notificações..."
                    } else {
                        "Loading notifications..."
                    })
                    .size(12.5)
                    .color(palette.neutral_status_fg),
                );
            });
            return;
        }

        if merged.is_empty() && self.feed.notifications_needs_connect {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(if pt {
                                "Conecte sua conta do YouTube"
                            } else {
                                "Connect your YouTube account"
                            })
                            .size(14.5)
                            .strong()
                            .color(palette.channel_fg),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(if pt {
                                "Conecte sua sessão do YouTube para exibir a mesma caixa de notificações do YouTube oficial."
                            } else {
                                "Connect your YouTube session to load the same notifications inbox as official YouTube."
                            })
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                        );
                        ui.add_space(12.0);
                        let connect_btn = egui::Button::new(
                            RichText::new(if pt {
                                "Conectar YouTube"
                            } else {
                                "Connect YouTube"
                            })
                            .size(13.0)
                            .strong()
                            .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .corner_radius(18)
                        .min_size(egui::vec2(160.0, 34.0));
                        if ui
                            .add(connect_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.start_notifications_inbox_fetch(true, None, ctx);
                        }
                    });
                });
            return;
        }

        if merged.is_empty()
            && let Some(err) = self.feed.notifications_error.clone()
        {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(err).size(12.5).color(palette.warning_fg));
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if styled_compact_button(
                                ui,
                                if pt { "Tentar novamente" } else { "Try again" },
                                true,
                                &palette,
                            )
                            .clicked()
                            {
                                self.start_notifications_inbox_fetch(false, None, ctx);
                            }
                            if styled_compact_button(
                                ui,
                                if pt {
                                    "Conectar YouTube"
                                } else {
                                    "Connect YouTube"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                self.start_notifications_inbox_fetch(true, None, ctx);
                            }
                        });
                    });
                });
            return;
        }

        if merged.is_empty() {
            let _ = empty_state_ui(
                ui,
                None,
                if pt {
                    "Nenhuma notificação no momento"
                } else {
                    "No notifications right now"
                },
                if pt {
                    "Novos vídeos dos canais seguidos e inscritos aparecerão aqui."
                } else {
                    "New uploads from your followed and subscribed channels will appear here."
                },
                None,
                &palette,
            );
            return;
        }

        if self.feed.notifications_needs_connect || self.feed.notifications_error.is_some() {
            let notice_text = if self.feed.notifications_needs_connect {
                if pt {
                    "Conecte o YouTube para sincronizar todas as notificações."
                } else {
                    "Connect YouTube to sync all official notifications."
                }
                .to_owned()
            } else {
                self.feed.notifications_error.clone().unwrap_or_default()
            };
            let needs_connect = self.feed.notifications_needs_connect;
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(notice_text)
                                .size(11.5)
                                .color(palette.neutral_status_fg),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let label = if needs_connect {
                                if pt {
                                    "Conectar YouTube"
                                } else {
                                    "Connect YouTube"
                                }
                            } else if pt {
                                "Tentar novamente"
                            } else {
                                "Try again"
                            };
                            if styled_compact_button(ui, label, true, &palette).clicked() {
                                self.start_notifications_inbox_fetch(needs_connect, None, ctx);
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }

        let menu_state_id = ui.make_persistent_id("notifications_open_row_menu_id");
        let mut open_row_menu_id = ctx.data(|d| d.get_temp::<String>(menu_state_id));

        let mut clicked_notification: Option<innertube::InnerTubeNotification> = None;
        let mut hide_notif: Option<innertube::InnerTubeNotification> = None;
        let mut mark_read_notif: Option<innertube::InnerTubeNotification> = None;
        let mut enqueue_notif_video: Option<Video> = None;
        let mut watch_later_notif_video: Option<Video> = None;
        let mut copy_notif_url: Option<String> = None;
        let mut load_more_token: Option<String> = None;

        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt("notifications_inbox_scroll")
            .max_height(520.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let mut last_section: Option<&str> = None;
                for notif in &merged {
                    if let Some(sec) = notif
                        .section_title
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        && last_section != Some(sec)
                    {
                        if last_section.is_some() {
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }
                        last_section = Some(sec);
                        ui.label(
                            RichText::new(sec)
                                .size(12.5)
                                .strong()
                                .color(palette.neutral_status_fg),
                        );
                        ui.add_space(2.0);
                    }

                    ui.push_id(&notif.id, |ui| {
                        let row_w = ui.available_width().max(380.0);
                        let row_interact_id = ui.make_persistent_id(("notif_row_click", &notif.id));
                        let prev_hovered = ctx
                            .read_response(row_interact_id)
                            .is_some_and(|r| r.hovered());
                        let row_bg = if prev_hovered {
                            palette.card_alt_bg
                        } else {
                            Color32::TRANSPARENT
                        };
                        let mut menu_btn_rect = egui::Rect::NOTHING;

                        let frame_resp = egui::Frame::new()
                            .fill(row_bg)
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(8, 8))
                            .show(ui, |ui| {
                                let inner_w = (row_w - 16.0).max(340.0);
                                ui.set_width(inner_w);
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let (dot_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(8.0, 40.0),
                                        egui::Sense::hover(),
                                    );
                                    if !notif.is_read {
                                        ui.painter().circle_filled(
                                            dot_rect.center(),
                                            2.5,
                                            Color32::from_rgb(62, 166, 255),
                                        );
                                    }

                                    let avatar_label = if notif.channel.trim().is_empty() {
                                        "YT"
                                    } else {
                                        notif.channel.as_str()
                                    };
                                    let _ = Self::paint_avatar_with_cache(
                                        &mut self.avatars,
                                        ui,
                                        ctx,
                                        notif.channel_avatar_url.as_deref(),
                                        avatar_label,
                                        40.0,
                                        false,
                                    );

                                    let has_thumb = notif
                                        .thumbnail_url
                                        .as_deref()
                                        .is_some_and(|u| !u.trim().is_empty());
                                    let thumb_block_w = if has_thumb { 94.0 } else { 0.0 };
                                    let fixed_w =
                                        8.0 + 8.0 + 40.0 + 8.0 + thumb_block_w + 8.0 + 26.0;
                                    let text_w = (inner_w - fixed_w).max(120.0);

                                    ui.vertical(|ui| {
                                        ui.set_width(text_w);
                                        ui.set_max_width(text_w);
                                        let galley = layout_clamped_galley(
                                            ui,
                                            &notif.title,
                                            text_w,
                                            egui::FontId::proportional(13.5),
                                            palette.channel_fg,
                                            3,
                                        );
                                        let (g_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(text_w, galley.size().y.max(16.0)),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().galley(g_rect.min, galley, palette.channel_fg);

                                        let ch_trimmed = notif.channel.trim();
                                        let pub_trimmed = notif.published_text.trim();
                                        let subtitle = if ch_trimmed.is_empty()
                                            || notif
                                                .title
                                                .to_lowercase()
                                                .contains(&ch_trimmed.to_lowercase())
                                        {
                                            pub_trimmed.to_owned()
                                        } else if pub_trimmed.is_empty() {
                                            ch_trimmed.to_owned()
                                        } else {
                                            format!("{ch_trimmed} \u{2022} {pub_trimmed}")
                                        };
                                        if !subtitle.is_empty() {
                                            ui.add_space(3.0);
                                            ui.label(
                                                RichText::new(subtitle)
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                            );
                                        }
                                    });

                                    if let Some(thumb_url) = notif
                                        .thumbnail_url
                                        .as_deref()
                                        .filter(|u| !u.trim().is_empty())
                                    {
                                        let _ = self.thumbnails.show_rounded(
                                            ui,
                                            ctx,
                                            thumb_url,
                                            egui::vec2(86.0, 48.0),
                                            6.0,
                                        );
                                    }

                                    let (m_rect, m_resp) = ui.allocate_exact_size(
                                        egui::vec2(26.0, 28.0),
                                        egui::Sense::click(),
                                    );
                                    menu_btn_rect = m_rect;
                                    let menu_is_open =
                                        open_row_menu_id.as_deref() == Some(notif.id.as_str());
                                    if m_resp.hovered() || menu_is_open {
                                        ui.painter().circle_filled(
                                            m_rect.center(),
                                            13.0,
                                            palette.header_button_bg,
                                        );
                                    }
                                    let dot_fg = if m_resp.hovered() || menu_is_open {
                                        palette.channel_fg
                                    } else {
                                        palette.neutral_status_fg
                                    };
                                    for dy in [-4.5_f32, 0.0, 4.5] {
                                        ui.painter().circle_filled(
                                            egui::pos2(m_rect.center().x, m_rect.center().y + dy),
                                            1.4,
                                            dot_fg,
                                        );
                                    }
                                    paint_focus_ring_if_focused(ui, &m_resp, 13.0);
                                    if m_resp
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(if pt {
                                            "Mais opções"
                                        } else {
                                            "More options"
                                        })
                                        .clicked()
                                    {
                                        if menu_is_open {
                                            open_row_menu_id = None;
                                        } else {
                                            open_row_menu_id = Some(notif.id.clone());
                                        }
                                    }
                                });
                            });

                        let row_rect = frame_resp.response.rect;
                        let click_rect = if menu_btn_rect.is_positive() {
                            egui::Rect::from_min_max(
                                row_rect.min,
                                egui::pos2(
                                    (menu_btn_rect.left() - 2.0).max(row_rect.left() + 1.0),
                                    row_rect.max.y,
                                ),
                            )
                        } else {
                            row_rect
                        };
                        let row_interact = ui
                            .interact(click_rect, row_interact_id, egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        paint_focus_ring_if_focused(ui, &row_interact, 8.0);
                        if row_interact.clicked() {
                            open_row_menu_id = None;
                            clicked_notification = Some(notif.clone());
                        }

                        if open_row_menu_id.as_deref() == Some(notif.id.as_str()) {
                            let video_opt = notif
                                .video_url
                                .as_deref()
                                .filter(|u| history::youtube_video_id(u).is_some())
                                .map(|url| {
                                    self.feed
                                        .videos
                                        .iter()
                                        .find(|v| v.url == url)
                                        .cloned()
                                        .unwrap_or_else(|| Video {
                                            title: notif.title.clone(),
                                            channel: notif.channel.clone(),
                                            url: url.to_owned(),
                                            thumbnail: notif
                                                .thumbnail_url
                                                .clone()
                                                .unwrap_or_default(),
                                            duration: String::new(),
                                        })
                                });
                            egui::Frame::new()
                                .fill(palette.card_alt_bg)
                                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(8.0)
                                .inner_margin(egui::Margin::symmetric(8, 6))
                                .show(ui, |ui| {
                                    ui.set_width((row_w - 28.0).max(240.0));
                                    if (notif.hide_feedback_token.is_some()
                                        || notif.hide_interaction_token.is_some()
                                        || notif.id.starts_with("rss:"))
                                        && styled_menu_item_button(
                                            ui,
                                            if pt {
                                                "Ocultar esta notificação"
                                            } else {
                                                "Hide this notification"
                                            },
                                            false,
                                            &palette,
                                        )
                                        .clicked()
                                    {
                                        hide_notif = Some(notif.clone());
                                        open_row_menu_id = None;
                                    }
                                    if !notif.is_read
                                        && styled_menu_item_button(
                                            ui,
                                            if pt {
                                                "Marcar como lida"
                                            } else {
                                                "Mark as read"
                                            },
                                            false,
                                            &palette,
                                        )
                                        .clicked()
                                    {
                                        mark_read_notif = Some(notif.clone());
                                        open_row_menu_id = None;
                                    }
                                    if let Some(video) = video_opt {
                                        if styled_menu_item_button(
                                            ui,
                                            if pt {
                                                "Adicionar à fila"
                                            } else {
                                                "Add to queue"
                                            },
                                            false,
                                            &palette,
                                        )
                                        .clicked()
                                        {
                                            enqueue_notif_video = Some(video.clone());
                                            open_row_menu_id = None;
                                        }
                                        if styled_menu_item_button(
                                            ui,
                                            if pt {
                                                "Assistir mais tarde"
                                            } else {
                                                "Watch Later"
                                            },
                                            false,
                                            &palette,
                                        )
                                        .clicked()
                                        {
                                            watch_later_notif_video = Some(video);
                                            open_row_menu_id = None;
                                        }
                                    }
                                    if let Some(url) = notif.video_url.as_deref()
                                        && styled_menu_item_button(
                                            ui,
                                            if pt { "Copiar link" } else { "Copy link" },
                                            false,
                                            &palette,
                                        )
                                        .clicked()
                                    {
                                        copy_notif_url = Some(url.to_owned());
                                        open_row_menu_id = None;
                                    }
                                });
                        }
                    });
                }

                if let Some(next_tok) = self.feed.notifications_continuation.clone() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            let label = if self.feed.notifications_loading_more {
                                if pt { "Carregando..." } else { "Loading..." }
                            } else if pt {
                                "Carregar mais"
                            } else {
                                "Load more"
                            };
                            let btn = egui::Button::new(
                                RichText::new(label)
                                    .size(12.5)
                                    .strong()
                                    .color(palette.header_button_fg),
                            )
                            .fill(palette.header_button_bg)
                            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                            .corner_radius(14)
                            .min_size(egui::vec2(140.0, 30.0));
                            let resp = ui.add_enabled(!self.feed.notifications_loading_more, btn);
                            if !self.feed.notifications_loading_more
                                && resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                            {
                                load_more_token = Some(next_tok);
                            }
                        });
                    });
                }
            });
        self.thumbnails.end_pass(ctx);

        ctx.data_mut(|d| {
            if let Some(id) = open_row_menu_id {
                d.insert_temp(menu_state_id, id);
            } else {
                d.remove::<String>(menu_state_id);
            }
        });

        if let Some(notif) = hide_notif {
            self.hide_notification(
                &notif.id,
                notif.hide_feedback_token.as_deref(),
                notif.hide_interaction_token.as_deref(),
                ctx,
            );
        }
        if let Some(notif) = mark_read_notif {
            self.mark_notification_read(&notif.id, notif.mark_read_token.as_deref(), ctx);
        }
        if let Some(video) = enqueue_notif_video
            && self.enqueue_video(video).is_ok()
        {
            let msg = self.pick("Adicionado à fila.", "Added to queue.");
            self.message = msg.into();
            self.toasts.success(msg);
        }
        if let Some(video) = watch_later_notif_video {
            self.library.data.toggle_watch_later(video);
            self.save_library();
            let msg = self.pick(
                "Lista \"Assistir mais tarde\" atualizada.",
                "\"Watch Later\" list updated.",
            );
            self.message = msg.into();
            self.toasts.success(msg);
        }
        if let Some(url) = copy_notif_url {
            let _ = copy_text_to_clipboard(ctx, &url);
            self.toasts
                .success(self.pick("Link copiado.", "Link copied."));
        }
        if let Some(tok) = load_more_token {
            self.start_notifications_inbox_fetch(false, Some(tok), ctx);
        }
        if let Some(notif) = clicked_notification {
            self.mark_notification_read(&notif.id, notif.mark_read_token.as_deref(), ctx);
            if let Some(video_url) = notif.video_url {
                if history::youtube_video_id(&video_url).is_some() {
                    let video = self
                        .feed
                        .videos
                        .iter()
                        .find(|v| v.url == video_url)
                        .cloned()
                        .unwrap_or(Video {
                            title: notif.title,
                            channel: notif.channel,
                            url: video_url,
                            thumbnail: notif.thumbnail_url.unwrap_or_default(),
                            duration: String::new(),
                        });
                    self.playback.pending_open_video = Some(video);
                    ui.close();
                } else if let Some(target) =
                    command_palette::parse_direct_youtube_target(&video_url)
                {
                    self.open_direct_youtube_target(target, ctx);
                    ui.close();
                } else if video_url.starts_with("http://") || video_url.starts_with("https://") {
                    open_external_url(&video_url);
                    ui.close();
                }
            }
        }
    }

    pub(crate) fn account_menu_contents_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pt = self.ui_pt();
        let palette = self.palette();
        ui.set_min_width(296.0);
        ui.set_max_width(330.0);

        let display_name = self
            .auth
            .my_channel_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                self.channels
                    .page
                    .as_ref()
                    .filter(|_| self.auth.authenticated && self.auth.my_channel.is_some())
                    .and_then(|p| p.title.as_deref())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| {
                if self.auth.authenticated {
                    if pt {
                        "Conta do YouTube".to_owned()
                    } else {
                        "YouTube Account".to_owned()
                    }
                } else if pt {
                    "Perfil Local".to_owned()
                } else {
                    "Local Profile".to_owned()
                }
            });
        let handle_label = match self.auth.my_channel.as_ref() {
            Some(ChannelTarget::Handle(h)) => h.clone(),
            Some(ChannelTarget::ChannelId(id)) => format!("@{id}"),
            None => {
                if self.auth.authenticated {
                    "@youtube".to_owned()
                } else {
                    "@local".to_owned()
                }
            }
        };

        let incognito_active = self.ui.incognito_mode
            || (self.settings.prefs.value.pause_watch_history
                && self.settings.prefs.value.pause_recommendations);

        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if incognito_active {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(rect.center(), 19.0, palette.header_button_bg);
                        paint_incognito_icon(ui.painter(), rect.center(), palette.channel_fg);
                    } else {
                        let avatar_url = self.auth.my_channel_avatar_url.clone();
                        let _ = Self::paint_avatar_with_cache(
                            &mut self.avatars,
                            ui,
                            ctx,
                            avatar_url.as_deref(),
                            &display_name,
                            38.0,
                            false,
                        );
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(
                            RichText::new(&display_name)
                                .size(14.0)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        ui.label(
                            RichText::new(&handle_label)
                                .size(12.0)
                                .color(palette.neutral_status_fg),
                        );
                    });
                });

                if incognito_active {
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(palette.info_accent_fg.gamma_multiply(0.18))
                        .stroke(egui::Stroke::new(
                            1.0,
                            palette.info_accent_fg.gamma_multiply(0.42),
                        ))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (icon_r, _) = ui.allocate_exact_size(
                                    egui::vec2(18.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                paint_incognito_icon(
                                    ui.painter(),
                                    icon_r.center(),
                                    palette.info_accent_fg,
                                );
                                ui.label(
                                    RichText::new(if pt {
                                        "MODO ANÔNIMO ATIVO"
                                    } else {
                                        "INCOGNITO ACTIVE"
                                    })
                                    .size(11.0)
                                    .strong()
                                    .color(palette.info_accent_fg),
                                );
                            });
                        });
                }
            });

        ui.add_space(8.0);

        let incognito_label = if incognito_active {
            if pt {
                "Desativar modo anônimo"
            } else {
                "Turn off Incognito"
            }
        } else if pt {
            "Ativar modo anônimo"
        } else {
            "Turn on Incognito"
        };
        if styled_menu_item_button(ui, incognito_label, incognito_active, &palette)
            .on_hover_text(if pt {
                "Pausa histórico de exibição e sincronização remota"
            } else {
                "Pauses watch history and remote sync"
            })
            .clicked()
        {
            self.set_incognito_mode(!incognito_active, ctx);
        }

        let restricted = self.settings.prefs.value.restricted_mode;
        let restricted_label = if pt {
            format!(
                "Modo restrito: {}",
                if restricted { "Ativado" } else { "Desativado" }
            )
        } else {
            format!("Restricted Mode: {}", if restricted { "On" } else { "Off" })
        };
        if styled_menu_item_button(ui, &restricted_label, restricted, &palette)
            .on_hover_text(if pt {
                "Ajuda a ocultar vídeos potencialmente destinados a adultos"
            } else {
                "Helps hide potentially mature videos"
            })
            .clicked()
        {
            self.set_restricted_mode(!restricted, ctx);
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let current_country = self
            .settings
            .prefs
            .value
            .country_code
            .clone()
            .unwrap_or_else(|| self.settings.prefs.value.language.region_code().to_owned());
        let mut selected_country = current_country.clone();
        ui.horizontal(|ui| {
            paint_globe_icon(ui, palette.neutral_status_fg);
            ui.label(
                RichText::new(if pt { "Local (gl):" } else { "Location (gl):" })
                    .size(12.5)
                    .color(palette.neutral_status_fg),
            );
            egui::ComboBox::from_id_salt("account_menu_country_gl")
                .width(156.0)
                .selected_text(
                    settings::COUNTRY_REGION_OPTIONS
                        .iter()
                        .find(|(code, _, _)| code.eq_ignore_ascii_case(&selected_country))
                        .map(|(_, l_pt, l_en)| if pt { *l_pt } else { *l_en })
                        .unwrap_or(selected_country.as_str()),
                )
                .show_ui(ui, |ui| {
                    for &(code, label_pt, label_en) in settings::COUNTRY_REGION_OPTIONS {
                        let label = if pt { label_pt } else { label_en };
                        ui.selectable_value(&mut selected_country, code.to_owned(), label);
                    }
                });
        });
        if selected_country != current_country {
            self.set_country_region(Some(selected_country), ctx);
        }

        ui.add_space(4.0);
        let cur_lang = self.settings.prefs.value.language;
        let mut next_lang = cur_lang;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if pt { "Idioma:" } else { "Language:" })
                    .size(12.5)
                    .color(palette.neutral_status_fg),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if styled_compact_button(ui, "EN", cur_lang == i18n::UiLanguage::En, &palette)
                    .clicked()
                {
                    next_lang = i18n::UiLanguage::En;
                }
                if styled_compact_button(ui, "PT-BR", cur_lang == i18n::UiLanguage::PtBr, &palette)
                    .clicked()
                {
                    next_lang = i18n::UiLanguage::PtBr;
                }
            });
        });
        if next_lang != cur_lang {
            self.set_ui_language(next_lang, ctx);
        }

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        if styled_menu_item_button(
            ui,
            if pt { "Configurações" } else { "Settings" },
            self.ui.settings_open,
            &palette,
        )
        .clicked()
        {
            self.ui.settings_open = true;
            ui.close();
        }
    }

    pub(crate) fn manage_subscriptions_page_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pt = self.ui_pt();
        let palette = self.palette();

        let mut entries: Vec<ManageSubscriptionEntry> = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        let mut remote_sub_ids = std::collections::HashSet::new();

        for ch in &self.feed.home.subscription_channels {
            let key = ch.channel_id.trim();
            if !key.is_empty() {
                remote_sub_ids.insert(key.to_owned());
            }
        }
        for (cid, status) in &self.auth.subscriptions {
            if status.subscription_id.is_some() && !cid.trim().is_empty() {
                remote_sub_ids.insert(cid.trim().to_owned());
            }
        }

        for ch in &self.followed_channels.data.channels {
            let key = ch.channel_id.trim().to_owned();
            if key.is_empty() || !seen_ids.insert(key.clone()) {
                continue;
            }
            let is_remote = remote_sub_ids.contains(&key);
            entries.push(ManageSubscriptionEntry {
                channel_id: key,
                title: ch.title.clone(),
                handle: ch.handle.clone(),
                url: ch.url.clone(),
                avatar_url: ch.avatar_url.clone(),
                is_rss_followed: true,
                is_remote_subscribed: is_remote,
            });
        }

        for ch in &self.feed.home.subscription_channels {
            let key = ch.channel_id.trim().to_owned();
            if key.is_empty() || !seen_ids.insert(key.clone()) {
                continue;
            }
            entries.push(ManageSubscriptionEntry {
                url: format!("https://www.youtube.com/channel/{key}"),
                channel_id: key,
                title: ch.title.clone(),
                handle: None,
                avatar_url: ch.avatar_url.clone(),
                is_rss_followed: false,
                is_remote_subscribed: true,
            });
        }

        for cid in &remote_sub_ids {
            let key = cid.trim().to_owned();
            if key.is_empty() || !seen_ids.insert(key.clone()) {
                continue;
            }
            let title = self
                .feed
                .videos
                .iter()
                .find(|v| {
                    self.metadata_for(&v.url)
                        .and_then(|m| m.channel_id.as_deref())
                        == Some(key.as_str())
                })
                .map(|v| v.channel.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| key.clone());
            let avatar_url = self.lookup_channel_avatar(&title).map(str::to_owned);
            entries.push(ManageSubscriptionEntry {
                url: format!("https://www.youtube.com/channel/{key}"),
                channel_id: key,
                title,
                handle: None,
                avatar_url,
                is_rss_followed: false,
                is_remote_subscribed: true,
            });
        }

        entries.sort_by_key(|a| a.title.to_lowercase());

        let mut back_to_feed = false;
        page_header_ui(
            ui,
            if pt {
                "Todas as inscrições"
            } else {
                "All subscriptions"
            },
            Some(if pt {
                "Gerencie notificações por canal, acompanhamento via RSS local e inscrições (/feed/channels)."
            } else {
                "Manage per-channel notifications, local RSS following, and subscriptions (/feed/channels)."
            }),
            &palette,
            |ui| {
                if styled_compact_button(
                    ui,
                    if pt {
                        "← Voltar às inscrições"
                    } else {
                        "← Back to subscriptions"
                    },
                    false,
                    &palette,
                )
                .clicked()
                {
                    back_to_feed = true;
                }
            },
        );
        if back_to_feed {
            self.ui.manage_subscriptions_open = false;
            return;
        }

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let hint = if pt {
                "Filtrar canais por nome, @handle ou ID..."
            } else {
                "Filter channels by name, @handle, or ID..."
            };
            ui.add_sized(
                [320.0_f32.min(ui.available_width() - 90.0).max(160.0), 32.0],
                egui::TextEdit::singleline(&mut self.ui.manage_subscriptions_query)
                    .id_salt("manage_subscriptions_search_input")
                    .hint_text(hint),
            );
            if !self.ui.manage_subscriptions_query.is_empty()
                && styled_compact_button(ui, if pt { "Limpar" } else { "Clear" }, false, &palette)
                    .clicked()
            {
                self.ui.manage_subscriptions_query.clear();
            }
        });
        ui.add_space(12.0);

        let filtered =
            filter_manage_subscriptions_channels(&entries, &self.ui.manage_subscriptions_query);
        if filtered.is_empty() {
            let _ = empty_state_ui(
                ui,
                None,
                if pt {
                    "Nenhum canal encontrado"
                } else {
                    "No channels found"
                },
                if pt {
                    "Siga canais via RSS ou inscreva-se para gerenciá-los aqui."
                } else {
                    "Follow channels via RSS or subscribe to manage them here."
                },
                None,
                &palette,
            );
            return;
        }

        let mut open_target: Option<ChannelTarget> = None;
        let mut notify_change: Option<(String, settings::ChannelNotifyLevel)> = None;
        let mut toggle_rss: Option<ManageSubscriptionEntry> = None;
        let mut ask_unsubscribe: Option<ManageSubscriptionEntry> = None;

        egui::ScrollArea::vertical()
            .id_salt("manage_subscriptions_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for entry in filtered {
                    ui.push_id(("manage_sub_row", &entry.channel_id), |ui| {
                        let current_level = self
                            .settings
                            .prefs
                            .value
                            .per_channel_notifications
                            .get(&entry.channel_id)
                            .copied()
                            .unwrap_or_default();

                        egui::Frame::new()
                            .fill(palette.card_alt_bg)
                            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                            .corner_radius(12.0)
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 12.0;
                                    let avatar_resp = Self::paint_avatar_with_cache(
                                        &mut self.avatars,
                                        ui,
                                        ctx,
                                        entry.avatar_url.as_deref(),
                                        &entry.title,
                                        44.0,
                                        true,
                                    );
                                    paint_focus_ring_if_focused(ui, &avatar_resp, 22.0);
                                    if avatar_resp.clicked() {
                                        open_target = Some(if entry.channel_id.starts_with("UC") {
                                            ChannelTarget::ChannelId(entry.channel_id.clone())
                                        } else if let Some(h) = entry.handle.clone() {
                                            ChannelTarget::Handle(h)
                                        } else {
                                            ChannelTarget::ChannelId(entry.channel_id.clone())
                                        });
                                    }

                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        let title_btn = ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(&entry.title)
                                                        .size(15.0)
                                                        .strong()
                                                        .color(palette.channel_fg),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        paint_focus_ring_if_focused(ui, &title_btn, 4.0);
                                        if title_btn.clicked() {
                                            open_target =
                                                Some(if entry.channel_id.starts_with("UC") {
                                                    ChannelTarget::ChannelId(
                                                        entry.channel_id.clone(),
                                                    )
                                                } else if let Some(h) = entry.handle.clone() {
                                                    ChannelTarget::Handle(h)
                                                } else {
                                                    ChannelTarget::ChannelId(
                                                        entry.channel_id.clone(),
                                                    )
                                                });
                                        }

                                        let sub_meta = entry
                                            .handle
                                            .as_deref()
                                            .unwrap_or(entry.channel_id.as_str());
                                        ui.label(
                                            RichText::new(sub_meta)
                                                .size(12.0)
                                                .color(palette.neutral_status_fg),
                                        );
                                    });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.x = 8.0;

                                            if styled_danger_button(
                                                ui,
                                                if pt {
                                                    "Cancelar inscrição"
                                                } else {
                                                    "Unsubscribe"
                                                },
                                                &palette,
                                            )
                                            .clicked()
                                            {
                                                ask_unsubscribe = Some(entry.clone());
                                            }

                                            let rss_label = if entry.is_rss_followed {
                                                if pt {
                                                    "✓ RSS ativo"
                                                } else {
                                                    "✓ RSS active"
                                                }
                                            } else if pt {
                                                "+ Seguir RSS"
                                            } else {
                                                "+ Follow RSS"
                                            };
                                            if styled_compact_button(
                                                ui,
                                                rss_label,
                                                entry.is_rss_followed,
                                                &palette,
                                            )
                                            .clicked()
                                            {
                                                toggle_rss = Some(entry.clone());
                                            }

                                            let level_label = match current_level {
                                                settings::ChannelNotifyLevel::All => {
                                                    if pt {
                                                        "Todas"
                                                    } else {
                                                        "All"
                                                    }
                                                }
                                                settings::ChannelNotifyLevel::Personalized => {
                                                    if pt {
                                                        "Personalizadas"
                                                    } else {
                                                        "Personalized"
                                                    }
                                                }
                                                settings::ChannelNotifyLevel::None => {
                                                    if pt {
                                                        "Nenhuma"
                                                    } else {
                                                        "None"
                                                    }
                                                }
                                            };
                                            let (bell_rect, bell_resp) = ui.allocate_exact_size(
                                                egui::vec2(138.0, 28.0),
                                                egui::Sense::click(),
                                            );
                                            let bell_bg = if bell_resp.hovered() {
                                                palette.card_bg
                                            } else {
                                                palette.header_button_bg
                                            };
                                            ui.painter().rect_filled(bell_rect, 14.0, bell_bg);
                                            ui.painter().rect_stroke(
                                                bell_rect,
                                                14.0,
                                                egui::Stroke::new(1.0, palette.card_alt_stroke),
                                                egui::StrokeKind::Inside,
                                            );
                                            paint_channel_notify_bell_icon(
                                                ui.painter(),
                                                egui::pos2(
                                                    bell_rect.left() + 16.0,
                                                    bell_rect.center().y,
                                                ),
                                                palette.header_button_fg,
                                                current_level,
                                            );
                                            ui.painter().text(
                                                egui::pos2(
                                                    bell_rect.left() + 30.0,
                                                    bell_rect.center().y,
                                                ),
                                                egui::Align2::LEFT_CENTER,
                                                format!("{level_label} ▾"),
                                                egui::FontId::proportional(12.0),
                                                palette.header_button_fg,
                                            );
                                            let bell_resp = bell_resp
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            paint_focus_ring_if_focused(ui, &bell_resp, 14.0);
                                            egui::Popup::menu(&bell_resp).show(|ui| {
                                                ui.set_min_width(170.0);
                                                for (lvl, pt_l, en_l) in [
                                                    (
                                                        settings::ChannelNotifyLevel::All,
                                                        "Todas",
                                                        "All",
                                                    ),
                                                    (
                                                        settings::ChannelNotifyLevel::Personalized,
                                                        "Personalizadas",
                                                        "Personalized",
                                                    ),
                                                    (
                                                        settings::ChannelNotifyLevel::None,
                                                        "Nenhuma",
                                                        "None",
                                                    ),
                                                ] {
                                                    if styled_menu_item_button(
                                                        ui,
                                                        if pt { pt_l } else { en_l },
                                                        current_level == lvl,
                                                        &palette,
                                                    )
                                                    .clicked()
                                                    {
                                                        notify_change =
                                                            Some((entry.channel_id.clone(), lvl));
                                                        ui.close();
                                                    }
                                                }
                                            });
                                        },
                                    );
                                });
                            });
                    });
                }
            });

        if let Some(target) = open_target {
            self.ui.manage_subscriptions_open = false;
            self.open_channel(target, ctx);
        }
        if let Some((cid, level)) = notify_change {
            self.set_channel_notification_level(&cid, level, ctx);
        }
        if let Some(ch) = toggle_rss {
            self.toggle_local_follow_channel(
                ch.channel_id,
                ch.title,
                ch.handle,
                ch.url,
                ch.avatar_url,
            );
        }
        if let Some(ch) = ask_unsubscribe {
            self.ui.pending_unsubscribe_channel = Some((ch.channel_id, ch.title));
        }

        if let Some((pending_cid, pending_title)) = self.ui.pending_unsubscribe_channel.clone() {
            let mut modal_open = true;
            let mut confirmed = false;
            let mut cancelled = false;
            let title = if pt {
                "Cancelar inscrição"
            } else {
                "Unsubscribe"
            };
            app_modal_window(
                ctx,
                egui::Id::new("manage_subscriptions_unsub_confirm_modal"),
                title,
                &mut modal_open,
                380.0,
                |ui| {
                    ui.label(
                        RichText::new(if pt {
                            format!("Deseja cancelar a inscrição de \"{pending_title}\"?")
                        } else {
                            format!("Unsubscribe from \"{pending_title}\"?")
                        })
                        .size(14.0)
                        .color(palette.channel_fg),
                    );
                    ui.add_space(14.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if styled_danger_button(
                            ui,
                            if pt {
                                "Cancelar inscrição"
                            } else {
                                "Unsubscribe"
                            },
                            &palette,
                        )
                        .clicked()
                        {
                            confirmed = true;
                        }
                        if styled_pill_button(
                            ui,
                            if pt { "Manter" } else { "Keep" },
                            false,
                            &palette,
                        )
                        .clicked()
                        {
                            cancelled = true;
                        }
                    });
                },
            );
            if confirmed {
                self.unsubscribe_channel(&pending_cid, ctx);
                self.ui.pending_unsubscribe_channel = None;
            } else if cancelled || !modal_open {
                self.ui.pending_unsubscribe_channel = None;
            }
        }
    }

    pub(crate) fn explore_hub_page_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pt = self.ui_pt();
        let palette = self.palette();
        let active_category = self.feed.explore_category;
        if !cfg!(test)
            && self.feed.explore_loaded_category != Some(active_category)
            && !self.feed.explore_loading
        {
            self.select_explore_category(active_category, ctx);
        }

        page_header_ui(
            ui,
            if pt { "Explorar" } else { "Explore" },
            Some(if pt {
                "Em alta, música, jogos, notícias, esportes e podcasts"
            } else {
                "Trending, music, gaming, news, sports, and podcasts"
            }),
            &palette,
            |_| {},
        );

        let mut clicked_category: Option<innertube::ExploreCategory> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            for category in innertube::ExploreCategory::ALL {
                let is_active = category == active_category;
                let resp = styled_pill_button(ui, category.label(pt), is_active, &palette);
                if resp.clicked() && !is_active {
                    clicked_category = Some(category);
                }
            }
        });
        ui.add_space(14.0);

        if let Some(cat) = clicked_category {
            self.select_explore_category(cat, ctx);
        }

        let is_loading = self.feed.explore_loading && self.feed.videos.is_empty();
        if is_loading {
            let dark_theme = self.is_dark_theme();
            skeleton::home_grid_skeleton(ui, ui.available_width().max(240.0), 2, 16.0, dark_theme);
            return;
        }

        if let Some(video) = self.home_grid_ui(ui, ctx) {
            self.playback.pending_open_video = Some(video);
        }

        if self.feed.explore_loading_more || self.feed.explore_continuation.is_some() {
            ui.add_space(8.0);
            if feed_load_more_footer_ui(ui, self.feed.explore_loading_more, pt, &palette) {
                self.start_load_more_explore(ctx);
            }
            ui.add_space(16.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_uniform_visible_row_range, compute_variable_visible_range,
        count_active_search_filters, estimated_home_shorts_shelf_height,
        estimated_search_card_height, format_search_spelling_banner, is_channel_verified,
        is_subscriptions_browse_section, sidebar_channel_status, toggle_search_feature,
    };
    use crate::BrowseSection;
    use crate::innertube::{
        SearchDuration, SearchFeature, SearchResultType, SearchSpellingCorrection, SearchUploadDate,
    };
    use crate::search::{SearchFilters, SearchResult, SearchSort, Video, VideoMetadata};
    use std::collections::{HashMap, HashSet};

    fn sample_video(url: &str, channel: &str) -> Video {
        Video {
            title: "Sample".into(),
            channel: channel.into(),
            url: url.into(),
            thumbnail: String::new(),
            duration: "10:00".into(),
        }
    }

    #[test]
    fn subscriptions_section_check_is_independent_of_localized_title() {
        assert!(is_subscriptions_browse_section(
            BrowseSection::Subscriptions
        ));
        assert!(!is_subscriptions_browse_section(BrowseSection::Home));
        assert!(!is_subscriptions_browse_section(BrowseSection::Shorts));
    }

    #[test]
    fn channel_verified_badge_requires_explicit_verified_flag() {
        assert!(!is_channel_verified(None));
        let high_views_unverified = VideoMetadata {
            view_count: Some(5_000_000),
            channel_id: Some("UC1234567890123456789012".into()),
            channel_verified: false,
            ..VideoMetadata::default()
        };
        assert!(!is_channel_verified(Some(&high_views_unverified)));

        let verified = VideoMetadata {
            channel_verified: true,
            ..VideoMetadata::default()
        };
        assert!(is_channel_verified(Some(&verified)));
    }

    #[test]
    fn sidebar_channel_status_tracks_unseen_and_live_per_channel() {
        let unseen_url = "https://www.youtube.com/watch?v=unseen00001";
        let live_url = "https://www.youtube.com/watch?v=live0000001";
        let quiet_url = "https://www.youtube.com/watch?v=quiet000001";

        let sources = vec![
            SearchResult {
                video: sample_video(unseen_url, "Alpha"),
                metadata: VideoMetadata {
                    channel_id: Some("UC_ALPHA".into()),
                    ..VideoMetadata::default()
                },
            },
            SearchResult {
                video: sample_video(live_url, "Beta"),
                metadata: VideoMetadata {
                    channel_id: Some("UC_BETA".into()),
                    is_live: true,
                    ..VideoMetadata::default()
                },
            },
            SearchResult {
                video: sample_video(quiet_url, "Gamma"),
                metadata: VideoMetadata {
                    channel_id: Some("UC_GAMMA".into()),
                    ..VideoMetadata::default()
                },
            },
        ];
        let new_urls = HashSet::from([unseen_url.to_owned()]);
        let empty_meta = HashMap::new();

        assert_eq!(
            sidebar_channel_status(
                "UC_ALPHA",
                "Alpha",
                &new_urls,
                &[sources.as_slice()],
                &[],
                &empty_meta
            ),
            (true, false)
        );
        assert_eq!(
            sidebar_channel_status(
                "UC_BETA",
                "Beta",
                &new_urls,
                &[sources.as_slice()],
                &[],
                &empty_meta
            ),
            (false, true)
        );
        assert_eq!(
            sidebar_channel_status(
                "UC_GAMMA",
                "Gamma",
                &new_urls,
                &[sources.as_slice()],
                &[],
                &empty_meta
            ),
            (false, false)
        );
    }

    #[test]
    fn uniform_visible_row_range_culls_offscreen_rows_and_preserves_total_height() {
        let total_rows = 40;
        let row_h = 200.0;
        let gap = 20.0;
        let expected_total = total_rows as f32 * row_h + (total_rows - 1) as f32 * gap;

        let (range, top_h, bottom_h) =
            compute_uniform_visible_row_range(total_rows, row_h, gap, 100.0, 1100.0, 1760.0, 1);
        assert!(range.start > 0);
        assert!(range.end < total_rows);
        let rendered_count = range.end - range.start;
        let rendered_h =
            rendered_count as f32 * row_h + rendered_count.saturating_sub(1) as f32 * gap;
        assert!((top_h + rendered_h + bottom_h - expected_total).abs() < 0.01);

        // Viewport above the list returns only overscan rows at the top.
        let (top_range, t0, b0) =
            compute_uniform_visible_row_range(total_rows, row_h, gap, 1000.0, 0.0, 400.0, 2);
        assert_eq!(top_range, 0..2);
        assert_eq!(t0, 0.0);
        assert!(b0 > 0.0);
    }

    #[test]
    fn uniform_visible_row_range_handles_empty_partial_bottom_clamp_and_overscan() {
        assert_eq!(
            compute_uniform_visible_row_range(0, 200.0, 20.0, 0.0, 0.0, 800.0, 2),
            (0..0, 0.0, 0.0)
        );

        let total_rows = 20;
        let row_h = 180.0;
        let gap = 20.0;
        let expected_total = total_rows as f32 * row_h + (total_rows - 1) as f32 * gap;

        // Partial viewport at the top with overscan=0 vs overscan=2.
        let (r0, t0, b0) =
            compute_uniform_visible_row_range(total_rows, row_h, gap, 0.0, 0.0, 450.0, 0);
        assert_eq!(r0, 0..3);
        assert_eq!(t0, 0.0);
        let h0 = (r0.end - r0.start) as f32 * row_h + (r0.end - r0.start - 1) as f32 * gap;
        assert!((t0 + h0 + b0 - expected_total).abs() < 0.01);

        let (r2, t2, b2) =
            compute_uniform_visible_row_range(total_rows, row_h, gap, 0.0, 0.0, 450.0, 2);
        assert_eq!(r2, 0..5);
        assert_eq!(t2, 0.0);
        let h2 = (r2.end - r2.start) as f32 * row_h + (r2.end - r2.start - 1) as f32 * gap;
        assert!((t2 + h2 + b2 - expected_total).abs() < 0.01);

        // Bottom clamp when scrolled to/past the end of the grid.
        let (rb, tb, bb) = compute_uniform_visible_row_range(
            total_rows,
            row_h,
            gap,
            0.0,
            expected_total + 300.0,
            expected_total + 1000.0,
            2,
        );
        assert_eq!(rb, 18..20);
        assert_eq!(bb, 0.0);
        let hb = (rb.end - rb.start) as f32 * row_h + (rb.end - rb.start - 1) as f32 * gap;
        assert!((tb + hb + bb - expected_total).abs() < 0.01);
    }

    #[test]
    fn variable_visible_range_culls_search_items_with_exact_spacers() {
        assert_eq!(
            compute_variable_visible_range(&[], 20.0, 0.0, 0.0, 800.0, 2),
            (0..0, 0.0, 0.0)
        );

        let heights = vec![180.0, 240.0, 200.0, 190.0, 220.0, 185.0, 210.0, 200.0];
        let gap = 20.0;
        let expected_total: f32 = heights.iter().sum::<f32>() + (heights.len() - 1) as f32 * gap;

        // Scrolled middle with overscan=1.
        let (range, top_h, bottom_h) =
            compute_variable_visible_range(&heights, gap, 0.0, 500.0, 950.0, 1);
        assert!(range.start >= 1);
        assert!(range.end < heights.len());
        let rendered_sum: f32 = heights[range.clone()].iter().sum();
        let rendered_gaps = (range.end - range.start).saturating_sub(1) as f32 * gap;
        assert!((top_h + rendered_sum + rendered_gaps + bottom_h - expected_total).abs() < 0.01);

        // Partial top viewport with overscan=0 vs overscan=2.
        let (p0, pt0, pb0) = compute_variable_visible_range(&heights, gap, 0.0, 0.0, 300.0, 0);
        assert_eq!(p0, 0..2);
        assert_eq!(pt0, 0.0);
        assert!(pb0 > 0.0);
        let (p2, pt2, pb2) = compute_variable_visible_range(&heights, gap, 0.0, 0.0, 300.0, 2);
        assert_eq!(p2, 0..4);
        assert_eq!(pt2, 0.0);
        let p2_sum: f32 = heights[p2.clone()].iter().sum();
        let p2_gaps = (p2.end - p2.start).saturating_sub(1) as f32 * gap;
        assert!((pt2 + p2_sum + p2_gaps + pb2 - expected_total).abs() < 0.01);

        // Bottom clamp past the end of the list.
        let (rb, tb, bb) = compute_variable_visible_range(
            &heights,
            gap,
            0.0,
            expected_total + 400.0,
            expected_total + 900.0,
            2,
        );
        assert_eq!(rb, 6..8);
        assert_eq!(bb, 0.0);
        let rb_sum: f32 = heights[rb.clone()].iter().sum();
        let rb_gaps = (rb.end - rb.start).saturating_sub(1) as f32 * gap;
        assert!((tb + rb_sum + rb_gaps + bb - expected_total).abs() < 0.01);
    }

    #[test]
    fn estimated_home_shorts_shelf_height_scales_with_viewport_width() {
        let narrow = estimated_home_shorts_shelf_height(420.0);
        let wide = estimated_home_shorts_shelf_height(1280.0);
        assert!(narrow > 250.0);
        assert!(wide > 250.0);
        assert!(estimated_search_card_height(900.0) >= 176.0);
        assert!(estimated_search_card_height(400.0) > 250.0);
    }

    #[test]
    fn active_search_filter_count_tracks_all_dimensions_and_multi_features() {
        let mut filters = SearchFilters::default();
        assert_eq!(count_active_search_filters(&filters), 0);

        filters.upload_date = SearchUploadDate::LastHour;
        assert_eq!(count_active_search_filters(&filters), 1);

        filters.result_type = SearchResultType::Video;
        filters.duration = SearchDuration::Between4And20Min;
        filters.sort = SearchSort::Date;
        assert_eq!(count_active_search_filters(&filters), 4);

        toggle_search_feature(&mut filters.features, SearchFeature::FourK);
        toggle_search_feature(&mut filters.features, SearchFeature::Subtitles);
        toggle_search_feature(&mut filters.features, SearchFeature::Hdr);
        assert_eq!(count_active_search_filters(&filters), 7);

        // Toggling an existing feature removes it without leaving duplicates.
        toggle_search_feature(&mut filters.features, SearchFeature::Subtitles);
        assert_eq!(count_active_search_filters(&filters), 6);
        assert!(!filters.features.contains(&SearchFeature::Subtitles));
    }

    #[test]
    fn search_spelling_banner_copy_covers_auto_corrected_and_did_you_mean_bilingual() {
        let auto_corr = SearchSpellingCorrection {
            corrected_query: "rust async tutorial".into(),
            original_query: Some("rsut asynk tutoral".into()),
            is_auto_corrected: true,
        };
        let pt_auto = format_search_spelling_banner(&auto_corr, true).expect("pt auto banner");
        assert_eq!(pt_auto.primary_prefix, "Mostrando resultados para");
        assert_eq!(pt_auto.primary_query, "rust async tutorial");
        assert_eq!(
            pt_auto.secondary_prefix.as_deref(),
            Some("Pesquisar em vez disso por")
        );
        assert_eq!(
            pt_auto.secondary_query.as_deref(),
            Some("rsut asynk tutoral")
        );
        assert!(pt_auto.is_auto_corrected);

        let en_auto = format_search_spelling_banner(&auto_corr, false).expect("en auto banner");
        assert_eq!(en_auto.primary_prefix, "Showing results for");
        assert_eq!(
            en_auto.secondary_prefix.as_deref(),
            Some("Search instead for")
        );

        let did_you_mean = SearchSpellingCorrection {
            corrected_query: "tokio runtime".into(),
            original_query: None,
            is_auto_corrected: false,
        };
        let pt_dym = format_search_spelling_banner(&did_you_mean, true).expect("pt dym banner");
        assert_eq!(pt_dym.primary_prefix, "Voc\u{00ea} quis dizer:");
        assert_eq!(pt_dym.primary_query, "tokio runtime");
        assert!(pt_dym.secondary_prefix.is_none());
        assert!(!pt_dym.is_auto_corrected);

        let en_dym = format_search_spelling_banner(&did_you_mean, false).expect("en dym banner");
        assert_eq!(en_dym.primary_prefix, "Did you mean:");
        assert_eq!(en_dym.primary_query, "tokio runtime");

        let empty = SearchSpellingCorrection {
            corrected_query: "   ".into(),
            original_query: None,
            is_auto_corrected: false,
        };
        assert!(format_search_spelling_banner(&empty, true).is_none());
    }

    #[test]
    fn card_hover_scrub_triggers_after_400ms_and_tracks_pointer_x() {
        use super::{
            CARD_HOVER_SCRUB_DELAY_MS, compute_card_hover_scrub_ratio, format_hover_scrub_timestamp,
        };

        assert_eq!(CARD_HOVER_SCRUB_DELAY_MS, 400);
        assert_eq!(
            compute_card_hover_scrub_ratio(0, Some(150.0), 100.0, 300.0),
            None
        );
        assert_eq!(
            compute_card_hover_scrub_ratio(399, Some(150.0), 100.0, 300.0),
            None
        );

        let r_start = compute_card_hover_scrub_ratio(400, Some(100.0), 100.0, 300.0).unwrap();
        assert!((r_start - 0.0).abs() < 1e-5);

        let r_mid = compute_card_hover_scrub_ratio(450, Some(200.0), 100.0, 300.0).unwrap();
        assert!((r_mid - 0.5).abs() < 1e-5);

        let r_end = compute_card_hover_scrub_ratio(900, Some(350.0), 100.0, 300.0).unwrap();
        assert!((r_end - 1.0).abs() < 1e-5);

        assert_eq!(
            format_hover_scrub_timestamp(0.5, "10:00").as_deref(),
            Some("5:00 / 10:00")
        );
    }

    #[test]
    fn live_now_shelf_filter_selects_only_live_subscription_videos() {
        use super::filter_live_now_subscription_indices;
        use crate::recommendations::ScoredVideo;

        let ranked = vec![
            ScoredVideo {
                video: sample_video("https://www.youtube.com/watch?v=vod11111111", "Alpha"),
                metadata: VideoMetadata::default(),
                score: 1.0,
                is_short: false,
                reasons: Vec::new(),
                matched_topic: None,
            },
            ScoredVideo {
                video: sample_video("https://www.youtube.com/watch?v=live2222222", "Beta"),
                metadata: VideoMetadata::default(),
                score: 1.0,
                is_short: false,
                reasons: Vec::new(),
                matched_topic: None,
            },
            ScoredVideo {
                video: sample_video("https://www.youtube.com/watch?v=upcm3333333", "Gamma"),
                metadata: VideoMetadata::default(),
                score: 1.0,
                is_short: false,
                reasons: Vec::new(),
                matched_topic: None,
            },
        ];
        let mut meta_map = HashMap::new();
        meta_map.insert(
            "https://www.youtube.com/watch?v=live2222222".to_owned(),
            VideoMetadata {
                is_live: true,
                ..VideoMetadata::default()
            },
        );
        meta_map.insert(
            "https://www.youtube.com/watch?v=upcm3333333".to_owned(),
            VideoMetadata {
                is_live: true,
                badges: vec!["Upcoming".to_owned()],
                ..VideoMetadata::default()
            },
        );

        let candidates = vec![0, 1, 2];
        let live_only =
            filter_live_now_subscription_indices(&ranked, &candidates, |url| meta_map.get(url));
        assert_eq!(live_only, vec![1]);
    }

    #[test]
    fn manage_subscriptions_filter_matches_title_handle_and_id_case_insensitively() {
        use super::{ManageSubscriptionEntry, filter_manage_subscriptions_channels};

        let entries = vec![
            ManageSubscriptionEntry {
                channel_id: "UC1111111111111111111111".into(),
                title: "Rust Systems Lab".into(),
                handle: Some("@rustlab".into()),
                url: "https://www.youtube.com/@rustlab".into(),
                avatar_url: None,
                is_rss_followed: true,
                is_remote_subscribed: true,
            },
            ManageSubscriptionEntry {
                channel_id: "UC2222222222222222222222".into(),
                title: "Retro Gaming Brasil".into(),
                handle: Some("@retrogamingbr".into()),
                url: "https://www.youtube.com/@retrogamingbr".into(),
                avatar_url: None,
                is_rss_followed: true,
                is_remote_subscribed: false,
            },
        ];

        assert_eq!(filter_manage_subscriptions_channels(&entries, "").len(), 2);
        let by_title = filter_manage_subscriptions_channels(&entries, "SYSTEMS");
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].channel_id, "UC1111111111111111111111");

        let by_handle = filter_manage_subscriptions_channels(&entries, "@retrogaming");
        assert_eq!(by_handle.len(), 1);
        assert_eq!(by_handle[0].channel_id, "UC2222222222222222222222");

        let by_id = filter_manage_subscriptions_channels(&entries, "uc11111");
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[0].title, "Rust Systems Lab");
    }

    #[test]
    fn partition_home_feed_indices_always_separates_shorts_and_subscriptions_use_three_desktop_cols()
     {
        let mut app = crate::FastTube::default();
        let regular_meta = crate::VideoMetadata::default();
        let short_meta = crate::VideoMetadata {
            kind: crate::search::SearchResultKind::Short,
            ..Default::default()
        };
        let ranked = vec![
            crate::recommendations::ScoredVideo {
                video: crate::Video {
                    url: "https://www.youtube.com/shorts/s1".into(),
                    title: "Short 1".into(),
                    channel: "Creator".into(),
                    thumbnail: String::new(),
                    duration: "0:30".into(),
                },
                metadata: short_meta.clone(),
                score: 1.0,
                is_short: true,
                reasons: Vec::new(),
                matched_topic: None,
            },
            crate::recommendations::ScoredVideo {
                video: crate::Video {
                    url: "https://www.youtube.com/shorts/s2".into(),
                    title: "Short 2".into(),
                    channel: "Creator".into(),
                    thumbnail: String::new(),
                    duration: "0:45".into(),
                },
                metadata: short_meta,
                score: 0.9,
                is_short: true,
                reasons: Vec::new(),
                matched_topic: None,
            },
        ];
        // Even with zero regular videos, Home/Subscriptions keep Shorts in the separated shelf list.
        let (main_only_shorts, shorts_only) = app.partition_home_feed_indices(&ranked, &[0, 1]);
        assert!(main_only_shorts.is_empty());
        assert_eq!(shorts_only, vec![0, 1]);

        let mixed = vec![
            crate::recommendations::ScoredVideo {
                video: crate::Video {
                    url: "https://www.youtube.com/watch?v=v1".into(),
                    title: "Long Video".into(),
                    channel: "Creator".into(),
                    thumbnail: String::new(),
                    duration: "14:20".into(),
                },
                metadata: regular_meta,
                score: 1.2,
                is_short: false,
                reasons: Vec::new(),
                matched_topic: None,
            },
            ranked[0].clone(),
        ];
        let (main_mixed, shorts_mixed) = app.partition_home_feed_indices(&mixed, &[0, 1]);
        assert_eq!(main_mixed, vec![0]);
        assert_eq!(shorts_mixed, vec![1]);

        app.settings.prefs.value.compact_card_density = false;
        assert_eq!(app.subscriptions_grid_metrics(480.0).1, 1);
        assert_eq!(app.subscriptions_grid_metrics(760.0).1, 2);
        assert_eq!(app.subscriptions_grid_metrics(1180.0), (18.0, 3));
        assert_eq!(app.subscriptions_grid_metrics(1600.0).1, 4);
    }
}
