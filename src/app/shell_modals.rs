use crate::*;

impl FastTube {
    #[inline]
    pub(crate) fn ui_pt(&self) -> bool {
        self.settings.prefs.value.language == i18n::UiLanguage::PtBr
    }

    #[inline]
    pub(crate) fn pick<'a>(&self, pt: &'a str, en: &'a str) -> &'a str {
        self.settings.prefs.value.language.pick(pt, en)
    }

    pub(crate) fn palette(&self) -> ThemePalette {
        self.settings.palette_for(self.system_is_dark())
    }

    pub(crate) fn overlay_style(&self) -> overlay::OverlayStyle {
        self.settings.overlay_style()
    }

    pub(crate) fn viewport_resize_direction(
        rect: egui::Rect,
        pos: egui::Pos2,
        edge_thickness: f32,
        corner_thickness: f32,
    ) -> Option<egui::viewport::ResizeDirection> {
        use egui::viewport::ResizeDirection;
        if !rect.contains(pos)
            || rect.width() <= edge_thickness * 2.0
            || rect.height() <= edge_thickness * 2.0
        {
            return None;
        }
        let left = (pos.x - rect.left()) <= edge_thickness;
        let right = (rect.right() - pos.x) <= edge_thickness;
        let top = (pos.y - rect.top()) <= edge_thickness;
        let bottom = (rect.bottom() - pos.y) <= edge_thickness;
        if !(left || right || top || bottom) {
            return None;
        }
        let near_left = (pos.x - rect.left()) <= corner_thickness;
        let near_right = (rect.right() - pos.x) <= corner_thickness;
        let near_top = (pos.y - rect.top()) <= corner_thickness;
        let near_bottom = (rect.bottom() - pos.y) <= corner_thickness;
        Some(if (top && near_left) || (left && near_top) {
            ResizeDirection::NorthWest
        } else if (top && near_right) || (right && near_top) {
            ResizeDirection::NorthEast
        } else if (bottom && near_left) || (left && near_bottom) {
            ResizeDirection::SouthWest
        } else if (bottom && near_right) || (right && near_bottom) {
            ResizeDirection::SouthEast
        } else if top {
            ResizeDirection::North
        } else if bottom {
            ResizeDirection::South
        } else if left {
            ResizeDirection::West
        } else {
            ResizeDirection::East
        })
    }

    pub(crate) fn handle_frameless_window_resize(&self, ctx: &egui::Context) -> bool {
        use egui::viewport::ResizeDirection;
        let (is_maximized, is_fullscreen, content_rect, hover_pos, primary_pressed) =
            ctx.input(|input| {
                let vp = input.viewport();
                (
                    vp.maximized.unwrap_or(false),
                    self.playback.fullscreen || vp.fullscreen.unwrap_or(false),
                    input.content_rect(),
                    input.pointer.hover_pos(),
                    input.pointer.primary_pressed(),
                )
            });
        if is_maximized || is_fullscreen || ctx.egui_is_using_pointer() {
            return false;
        }
        let Some(pos) = hover_pos else {
            return false;
        };
        let Some(direction) = Self::viewport_resize_direction(content_rect, pos, 4.0, 10.0) else {
            return false;
        };
        let cursor = match direction {
            ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
            ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
        };
        ctx.set_cursor_icon(cursor);
        if primary_pressed {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
        true
    }

    pub(crate) fn preprocess_raw_input(&mut self, raw_input: &mut egui::RawInput) {
        let chromeless =
            self.playback.fullscreen || (self.playback.mini_player && self.is_watch_page_active());
        let allow_activate = !chromeless;
        let header_guard_y = if chromeless { 0.0 } else { 56.0 };
        self.autoscroll
            .preprocess_raw_input(raw_input, allow_activate, header_guard_y);
        if self.autoscroll.is_active() {
            self.playback.player_context_menu = None;
        }
    }

    pub(crate) fn apply_middle_autoscroll(&mut self, ctx: &egui::Context) {
        if self.playback.fullscreen || self.playback.mini_player {
            self.autoscroll.cancel();
            return;
        }
        if let Some((vel, _)) = self.autoscroll.velocity_and_cursor()
            && vel != egui::Vec2::ZERO
        {
            let dt = ctx.input(|i| i.stable_dt.clamp(0.0, 0.05));
            ctx.input_mut(|i| i.smooth_scroll_delta += vel * dt);
            ctx.request_repaint();
        }
    }

    pub(crate) fn paint_middle_autoscroll_overlay(&self, ctx: &egui::Context) {
        let Some(anchor) = self.autoscroll.anchor() else {
            return;
        };
        let Some((_, cursor)) = self.autoscroll.velocity_and_cursor() else {
            return;
        };
        let dark = self.is_dark_theme();
        let bg = if dark {
            Color32::from_rgba_unmultiplied(32, 32, 36, 232)
        } else {
            Color32::from_rgba_unmultiplied(248, 248, 250, 240)
        };
        let border = if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 110)
        } else {
            Color32::from_rgba_unmultiplied(40, 40, 44, 140)
        };
        let fg = if dark {
            Color32::from_rgb(240, 240, 244)
        } else {
            Color32::from_rgb(36, 36, 40)
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("fasttube_autoscroll_anchor"),
        ));
        painter.circle(anchor, 13.5, bg, egui::Stroke::new(1.2, border));
        painter.circle_filled(anchor, 2.2, fg);
        let arrow_offsets = [
            (
                egui::vec2(0.0, -8.5),
                egui::vec2(-2.6, -5.2),
                egui::vec2(2.6, -5.2),
            ),
            (
                egui::vec2(0.0, 8.5),
                egui::vec2(-2.6, 5.2),
                egui::vec2(2.6, 5.2),
            ),
            (
                egui::vec2(-8.5, 0.0),
                egui::vec2(-5.2, -2.6),
                egui::vec2(-5.2, 2.6),
            ),
            (
                egui::vec2(8.5, 0.0),
                egui::vec2(5.2, -2.6),
                egui::vec2(5.2, 2.6),
            ),
        ];
        for (tip, a, b) in arrow_offsets {
            painter.add(egui::Shape::convex_polygon(
                vec![anchor + tip, anchor + a, anchor + b],
                fg,
                egui::Stroke::NONE,
            ));
        }
        ctx.set_cursor_icon(cursor);
    }

    pub(crate) fn header_menu_button_ui(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let nav_visible = self.ui.navigation_open && !self.playback.fullscreen;
        let nav_tip = if nav_visible {
            self.pick("Ocultar navega\u{00e7}\u{00e3}o", "Hide navigation")
        } else {
            self.pick("Mostrar navega\u{00e7}\u{00e3}o", "Show navigation")
        };
        let palette = self.palette();
        let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
        if response.hovered() {
            ui.painter()
                .circle_filled(rect.center(), 18.0, palette.header_button_bg);
        }
        let c = rect.center();
        let stroke = egui::Stroke::new(1.6, palette.icon_fg);
        for dy in [-5.0_f32, 0.0, 5.0] {
            ui.painter().line_segment(
                [
                    egui::pos2(c.x - 8.5, c.y + dy),
                    egui::pos2(c.x + 8.5, c.y + dy),
                ],
                stroke,
            );
        }
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(nav_tip);
        if response.clicked() {
            self.ui.navigation_open = !self.ui.navigation_open;
        }
        response
    }

    pub(crate) fn header_brand_ui(
        &mut self,
        ui: &mut egui::Ui,
        palette: ThemePalette,
    ) -> egui::Response {
        let region_code = self.settings.prefs.value.language.region_code();
        let title_galley = egui::WidgetText::from(
            RichText::new("FastTube")
                .strong()
                .size(19.5)
                .color(palette.header_button_fg),
        )
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::FontSelection::Default,
        );
        let badge_galley = egui::WidgetText::from(
            RichText::new(region_code)
                .size(9.5)
                .color(palette.neutral_status_fg),
        )
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::FontSelection::Default,
        );

        let logo_w = 30.0;
        let logo_h = 21.0;
        let logo_gap = 5.0;
        let badge_gap = 3.0;
        let total_w =
            logo_w + logo_gap + title_galley.size().x + badge_gap + badge_galley.size().x + 2.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(total_w, 36.0), egui::Sense::click());
        let logo_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + logo_w * 0.5, rect.center().y),
            egui::vec2(logo_w, logo_h),
        );
        paint_youtube_logo_badge_at(ui.painter(), logo_rect);

        let title_x = logo_rect.right() + logo_gap;
        let title_y = rect.center().y - title_galley.size().y * 0.5;
        let title_pos = egui::pos2(title_x, title_y);
        let title_right = title_x + title_galley.size().x;
        ui.painter()
            .galley(title_pos, title_galley, palette.header_button_fg);

        let badge_pos = egui::pos2(title_right + badge_gap, (title_y - 1.5).max(rect.top()));
        ui.painter()
            .galley(badge_pos, badge_galley, palette.neutral_status_fg);

        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(self.pick("P\u{00e1}gina inicial do FastTube", "FastTube Home"))
    }

    pub(crate) fn handle_header_home(&mut self, ctx: &egui::Context) {
        let already_on_default_home = !self.is_watch_page_active()
            && !self.channels.view_open
            && !self.ui.settings_open
            && self.ui.browse_section == BrowseSection::Home
            && !self.ui.search_results_mode
            && self.feed.filter == FeedFilter::All
            && self.feed.home.title.is_none();
        if already_on_default_home && !self.feed.videos.is_empty() {
            return;
        }
        self.ui.settings_open = false;
        self.ui.browse_section = BrowseSection::Home;
        self.ui.search_results_mode = false;
        self.feed.filter = FeedFilter::All;
        self.queue.remote_playlist_id = None;
        self.invalidate_feed_cache();
        self.close_channel_view();
        if self.selected.is_some() {
            if !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
        } else {
            self.sync_player_window_visibility();
        }
        self.feed.home.subscriptions_progressive_streaming = false;
        if !self.restore_cached_home_feed(ctx)
            && (!self.feed.home.stream_task.is_running()
                || self.feed.home.stream_task.active_request_id().is_some())
            && !self.feed.home.batch_task.is_running()
            && !self.feed.search.task.is_running()
        {
            self.start_fast_home_feed(false, ctx);
        }
    }

    pub(crate) fn header_search_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        search_width: f32,
    ) -> egui::Rect {
        let search_width = search_width.clamp(140.0, 640.0);
        let pill_height = 38.0;
        let pill_radius = 19u8;
        let dark_theme = self.is_dark_theme();
        let pill_bg = if dark_theme {
            Color32::from_rgb(18, 18, 18)
        } else {
            Color32::WHITE
        };
        let pill_stroke = if dark_theme {
            Color32::from_rgb(48, 48, 48)
        } else {
            Color32::from_rgb(204, 204, 204)
        };
        let button_bg = if dark_theme {
            Color32::from_rgb(34, 34, 34)
        } else {
            Color32::from_rgb(240, 240, 240)
        };
        let button_hover_bg = if dark_theme {
            Color32::from_rgb(44, 44, 44)
        } else {
            Color32::from_rgb(230, 230, 230)
        };
        let mut trigger_search = false;
        let mut search_changed = false;
        let mut search_has_focus = false;
        let mut search_gained_focus = false;
        let mut search_lost_focus = false;
        let left_margin = 16.0;
        let inner_w = (search_width - left_margin).max(124.0);
        let pill_resp = egui::Frame::new()
            .fill(pill_bg)
            .stroke(egui::Stroke::NONE)
            .corner_radius(pill_radius)
            .inner_margin(egui::Margin {
                left: 16,
                right: 0,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| {
                ui.set_width(inner_w);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let btn_w = 64.0;
                    let div_w = 1.0;
                    let clear_w = if self.feed.query.is_empty() {
                        0.0
                    } else {
                        28.0
                    };
                    let input_w = (inner_w - clear_w - div_w - btn_w).max(40.0);
                    let search_hint = self.pick("Pesquisar", "Search").to_owned();
                    let search = ui.add_sized(
                        [input_w, pill_height],
                        egui::TextEdit::singleline(&mut self.feed.query)
                            .id_salt("header_search_input")
                            .frame(egui::Frame::NONE)
                            .desired_width(input_w)
                            .vertical_align(egui::Align::Center)
                            .hint_text(search_hint),
                    );
                    if std::mem::take(&mut self.ui.focus_search_requested) {
                        search.request_focus();
                    }
                    search_changed = search.changed();
                    search_has_focus = search.has_focus();
                    search_gained_focus = search.gained_focus();
                    search_lost_focus = search.lost_focus();
                    let enter =
                        search_lost_focus && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if !self.feed.query.is_empty()
                        && ui
                            .add_sized(
                                [clear_w, pill_height],
                                egui::Button::new(RichText::new("\u{00d7}").size(16.0))
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(self.pick("Limpar pesquisa", "Clear search"))
                            .clicked()
                    {
                        self.feed.query.clear();
                        self.feed.search_suggest.clear_remote_suggestions();
                        self.feed.search_suggest.pending_fetch_query = None;
                        self.feed.search_suggest.highlighted_index = None;
                        self.feed.search_suggest.task.cancel();
                    }
                    let (div_rect, _) = ui
                        .allocate_exact_size(egui::vec2(div_w, pill_height), egui::Sense::hover());
                    ui.painter().line_segment(
                        [
                            egui::pos2(div_rect.center().x, div_rect.top()),
                            egui::pos2(div_rect.center().x, div_rect.bottom()),
                        ],
                        egui::Stroke::new(1.0, pill_stroke),
                    );
                    let (btn_rect, btn_resp) = ui
                        .allocate_exact_size(egui::vec2(btn_w, pill_height), egui::Sense::click());
                    let btn_corner = egui::CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: pill_radius,
                        se: pill_radius,
                    };
                    let fill = if btn_resp.hovered() {
                        button_hover_bg
                    } else {
                        button_bg
                    };
                    ui.painter().rect_filled(btn_rect, btn_corner, fill);
                    let icon_rect =
                        egui::Rect::from_center_size(btn_rect.center(), egui::vec2(18.0, 18.0));
                    Icon::Search
                        .image(palette.icon_fg, 18.0)
                        .paint_at(ui, icon_rect);
                    let search_clicked = btn_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(self.pick("Pesquisar (Enter)", "Search (Enter)"))
                        .clicked();
                    trigger_search = search_clicked || enter;
                });
            });
        let outer_stroke = if search_has_focus {
            Color32::from_rgb(28, 98, 185)
        } else {
            pill_stroke
        };
        ui.painter().rect_stroke(
            pill_resp.response.rect,
            f32::from(pill_radius),
            egui::Stroke::new(1.0, outer_stroke),
            egui::StrokeKind::Inside,
        );

        let just_opened = search_gained_focus || search_changed;
        if just_opened {
            self.feed.search_suggest.open = true;
        }
        if search_changed {
            self.feed.search_suggest.highlighted_index = None;
            self.feed.search_suggest.last_input_query = self.feed.query.clone();
            self.feed.search_suggest.last_input_at = Some(Instant::now());
            let trimmed = self.feed.query.trim().to_owned();
            if trimmed.len() < 2
                || trimmed.starts_with("http://")
                || trimmed.starts_with("https://")
                || trimmed.starts_with(':')
            {
                self.feed.search_suggest.pending_fetch_query = None;
                self.feed.search_suggest.clear_remote_suggestions();
                self.feed.search_suggest.task.cancel();
            } else if !trimmed
                .eq_ignore_ascii_case(self.feed.search_suggest.last_fetched_query.trim())
            {
                if let Some(cached) = self.feed.search_suggest.get_cached_prefix(&trimmed) {
                    self.feed.search_suggest.pending_fetch_query = None;
                    self.feed.search_suggest.last_fetched_query = trimmed;
                    self.feed.search_suggest.set_remote_suggestions(cached);
                } else {
                    self.feed.search_suggest.pending_fetch_query = Some(trimmed);
                    ctx.request_repaint_after(Duration::from_millis(220));
                }
            }
        }

        let suggestions: Vec<search::SearchSuggestionItem> = self
            .feed
            .search_suggest
            .merged_suggestions(&self.feed.query, &self.feed.recent_searches, 8)
            .to_vec();
        if let Some(idx) = self.feed.search_suggest.highlighted_index
            && idx >= suggestions.len()
        {
            self.feed.search_suggest.highlighted_index = None;
        }

        if self.feed.search_suggest.open
            && !suggestions.is_empty()
            && (search_has_focus || search_lost_focus)
        {
            let (up_pressed, down_pressed, esc_pressed, enter_pressed) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowUp),
                    input.key_pressed(egui::Key::ArrowDown),
                    input.key_pressed(egui::Key::Escape),
                    input.key_pressed(egui::Key::Enter),
                )
            });
            if down_pressed {
                self.feed.search_suggest.highlighted_index =
                    Some(match self.feed.search_suggest.highlighted_index {
                        None => 0,
                        Some(idx) => (idx + 1) % suggestions.len(),
                    });
            } else if up_pressed {
                self.feed.search_suggest.highlighted_index =
                    Some(match self.feed.search_suggest.highlighted_index {
                        None | Some(0) => suggestions.len() - 1,
                        Some(idx) => idx - 1,
                    });
            } else if esc_pressed {
                self.feed.search_suggest.open = false;
                self.feed.search_suggest.highlighted_index = None;
            } else if enter_pressed
                && let Some(idx) = self.feed.search_suggest.highlighted_index
                && let Some(item) = suggestions.get(idx)
            {
                self.feed.query = item.text.clone();
                trigger_search = true;
            }
        }

        if !trigger_search && self.feed.search_suggest.open && !suggestions.is_empty() {
            let pill_rect = pill_resp.response.rect;
            let dropdown_bg = if dark_theme {
                Color32::from_rgb(33, 33, 33)
            } else {
                Color32::WHITE
            };
            let dropdown_hover = if dark_theme {
                Color32::from_rgb(52, 52, 52)
            } else {
                Color32::from_rgb(238, 238, 238)
            };
            let mut clicked_query: Option<String> = None;
            let mut removed_recent: Option<String> = None;
            let mut dropdown_interacted = false;

            let area_resp = egui::Area::new(egui::Id::new("header_search_suggestions_popup"))
                .fixed_pos(egui::pos2(pill_rect.left(), pill_rect.bottom() + 4.0))
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(dropdown_bg)
                        .stroke(egui::Stroke::new(1.0, pill_stroke))
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(6, 6))
                        .show(ui, |ui| {
                            let inner_w = (pill_rect.width() - 12.0).max(200.0);
                            ui.set_width(inner_w);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                for (idx, item) in suggestions.iter().enumerate() {
                                    let is_highlighted =
                                        self.feed.search_suggest.highlighted_index == Some(idx);
                                    let row_w = inner_w;
                                    let delete_w = if item.is_recent { 26.0 } else { 0.0 };
                                    let main_w = (row_w - delete_w - 4.0).max(120.0);
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        let prefix = if item.is_recent { "🕒 " } else { "🔍 " };
                                        let row_btn = egui::Button::new(
                                            RichText::new(format!("{prefix}{}", item.text))
                                                .size(13.5)
                                                .color(palette.channel_fg),
                                        )
                                        .fill(if is_highlighted {
                                            dropdown_hover
                                        } else {
                                            Color32::TRANSPARENT
                                        })
                                        .corner_radius(8)
                                        .min_size(egui::vec2(main_w, 30.0));
                                        let row_resp = ui
                                            .add(row_btn)
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if row_resp.hovered() {
                                            self.feed.search_suggest.highlighted_index = Some(idx);
                                        }
                                        if row_resp.clicked() {
                                            clicked_query = Some(item.text.clone());
                                            dropdown_interacted = true;
                                        }
                                        if item.is_recent {
                                            let del_btn = egui::Button::new(
                                                RichText::new("\u{00d7}")
                                                    .size(14.0)
                                                    .color(palette.neutral_status_fg),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .corner_radius(6)
                                            .min_size(egui::vec2(24.0, 28.0));
                                            if ui
                                                .add(del_btn)
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text("Remover das pesquisas recentes")
                                                .clicked()
                                            {
                                                removed_recent = Some(item.text.clone());
                                                dropdown_interacted = true;
                                            }
                                        }
                                    });
                                }
                            });
                        });
                });

            if let Some(recent) = removed_recent {
                self.remove_recent_search(&recent);
            }
            if let Some(selected_query) = clicked_query {
                self.feed.query = selected_query;
                trigger_search = true;
            } else if !just_opened && !dropdown_interacted {
                let clicked_outside = ctx.input(|input| {
                    input.pointer.primary_pressed().then(|| {
                        input.pointer.interact_pos().is_some_and(|pos| {
                            !pill_rect.contains(pos) && !area_resp.response.rect.contains(pos)
                        })
                    })
                });
                if clicked_outside == Some(true) {
                    self.feed.search_suggest.open = false;
                    self.feed.search_suggest.highlighted_index = None;
                }
            }
        }

        if trigger_search {
            self.feed.search_suggest.open = false;
            self.feed.search_suggest.highlighted_index = None;
            let query = self.feed.query.clone();
            if let Some(direct) = command_palette::parse_direct_youtube_target(&query) {
                self.record_recent_search(&query);
                self.open_direct_youtube_target(direct, ctx);
            } else {
                self.ui.search_results_mode = true;
                self.ui.browse_section = BrowseSection::Home;
                self.feed.filter = FeedFilter::All;
                self.queue.remote_playlist_id = None;
                self.feed.search_chip = search::SearchChip::All;
                self.feed.search_filters_open = false;
                self.feed.search_filters_modal_open = false;
                self.feed.search_save_modal_video = None;
                self.feed.search_report_modal_video = None;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                self.record_recent_search(&query);
                self.start_search(ctx);
            }
        }
        pill_resp.response.rect
    }

    pub(crate) fn navigate_from_shortcut(&mut self, ctx: &egui::Context, section: BrowseSection) {
        match section {
            BrowseSection::Subscriptions => {
                self.open_subscriptions_section(false, ctx);
                return;
            }
            BrowseSection::Shorts => {
                self.open_shorts_tab(ctx);
                return;
            }
            _ => {}
        }
        self.ui.settings_open = false;
        self.ui.search_results_mode = false;
        self.queue.remote_playlist_id = None;
        self.close_channel_view();
        if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
            self.close_video(ctx);
        }
        self.ui.browse_section = section;
        if section == BrowseSection::Home {
            self.feed.filter = FeedFilter::All;
            let _ = self.restore_cached_home_feed(ctx);
            self.invalidate_feed_cache();
        } else if section == BrowseSection::Explore {
            self.select_explore_category(self.feed.explore_category, ctx);
        } else if section == BrowseSection::History {
            self.on_enter_history_section(ctx);
        } else if section == BrowseSection::Playlists {
            self.library.selected_collection_idx = None;
            self.library.active_playlist_detail.playlist_id = None;
            self.on_enter_playlists_section(ctx);
        } else if section == BrowseSection::WatchLater {
            self.on_enter_watch_later_section(ctx);
        } else if section == BrowseSection::LikedVideos {
            self.start_liked_videos_fetch(false, ctx);
        } else if matches!(self.feed.filter, FeedFilter::Shorts) {
            self.feed.filter = FeedFilter::All;
            self.invalidate_feed_cache();
        }
    }

    pub(crate) fn shortcuts_window_ui(&mut self, ctx: &egui::Context) {
        if !self.ui.shortcuts_open {
            return;
        }
        let mut open = self.ui.shortcuts_open;
        let palette = self.palette();
        let kb = self.settings.prefs.value.keybindings.clone();
        let pt = self.ui_pt();
        let title = if pt {
            "Atalhos e acessibilidade"
        } else {
            "Shortcuts & accessibility"
        };
        let play_key = if pt {
            format!("Espaço / {}", kb.play_pause)
        } else {
            format!("Space / {}", kb.play_pause)
        };
        let seek_keys = format!("{} / {}", kb.seek_back, kb.seek_forward);
        let ab_keys = format!("{} / {} / {}", kb.ab_loop_a, kb.ab_loop_b, kb.ab_loop_clear);
        let seek_label = if pt {
            format!("voltar ou avançar {} segundos", self.seek_step_secs())
        } else {
            format!("seek back or forward {} seconds", self.seek_step_secs())
        };
        let screen = ctx.content_rect();

        let paint_shortcut_row = |ui: &mut egui::Ui, key: &str, action: &str| {
            ui.horizontal(|ui| {
                egui::Frame::new()
                    .fill(palette.header_button_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(7, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(key)
                                .size(11.5)
                                .monospace()
                                .strong()
                                .color(palette.channel_fg),
                        );
                    });
                ui.add_space(4.0);
                ui.label(RichText::new(action).size(12.5).color(palette.snippet_fg));
            });
        };

        let mut close_clicked = false;
        app_modal_window(
            ctx,
            egui::Id::new("shortcuts_modal_window"),
            title,
            &mut open,
            500.0,
            |ui| {
                ui.label(
                    RichText::new(if pt {
                        "Use Tab e Shift+Tab para percorrer controles. Enter ou Espaço ativa o item focado."
                    } else {
                        "Use Tab and Shift+Tab to move focus. Enter or Space activates the focused item."
                    })
                    .size(12.0)
                    .color(palette.neutral_status_fg),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .id_salt("shortcuts_modal_scroll")
                    .max_height((screen.height() - 180.0).clamp(220.0, 480.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(if pt { "Reprodução" } else { "Playback" })
                                .size(13.5)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        ui.add_space(4.0);
                        let vol_keys = format!("{} / {}", kb.volume_up, kb.volume_down);
                        let frame_keys = format!("{} / {}", kb.frame_back, kb.frame_forward);
                        let speed_keys = format!("{} / {}", kb.speed_down, kb.speed_up);
                        let fs_keys = format!("{} / {}", kb.fullscreen, kb.fullscreen_alt);
                        let nav_vid_keys = format!("{} / {}", kb.prev_video, kb.next_video);
                        let chapter_keys = format!("{} / {}", kb.prev_chapter, kb.next_chapter);
                        let home_end_keys = format!("{} / {}", kb.seek_home, kb.seek_end);
                        let cap_size_keys =
                            format!("{} / {}", kb.caption_size_up, kb.caption_size_down);
                        for (key, action) in [
                            (
                                play_key.as_str(),
                                if pt { "reproduzir/pausar" } else { "play/pause" },
                            ),
                            (seek_keys.as_str(), seek_label.as_str()),
                            (
                                "J / L",
                                if pt {
                                    "voltar ou avançar 10 segundos"
                                } else {
                                    "seek back or forward 10 seconds"
                                },
                            ),
                            (
                                vol_keys.as_str(),
                                if pt {
                                    "aumentar ou diminuir volume (até 200%)"
                                } else {
                                    "volume up or down (up to 200%)"
                                },
                            ),
                            (
                                home_end_keys.as_str(),
                                if pt {
                                    "ir para o início ou fim do vídeo"
                                } else {
                                    "jump to start or end of video"
                                },
                            ),
                            (
                                chapter_keys.as_str(),
                                if pt {
                                    "capítulo anterior / próximo capítulo"
                                } else {
                                    "previous / next chapter"
                                },
                            ),
                            (
                                frame_keys.as_str(),
                                if pt {
                                    "recuar ou avançar 1 quadro (quando pausado)"
                                } else {
                                    "step back or forward 1 frame (while paused)"
                                },
                            ),
                            (
                                speed_keys.as_str(),
                                if pt {
                                    "diminuir ou aumentar velocidade (0.25x – 4x)"
                                } else {
                                    "decrease or increase speed (0.25x – 4x)"
                                },
                            ),
                            (
                                ab_keys.as_str(),
                                if pt {
                                    "definir ponto A, ponto B ou limpar loop A-B"
                                } else {
                                    "set point A, point B, or clear A-B loop"
                                },
                            ),
                            (
                                kb.screenshot.as_str(),
                                if pt {
                                    "capturar quadro do vídeo (PNG)"
                                } else {
                                    "capture video frame (PNG)"
                                },
                            ),
                            (kb.mute.as_str(), if pt { "silenciar" } else { "mute" }),
                            (
                                kb.audio_only.as_str(),
                                if pt {
                                    "alternar somente áudio"
                                } else {
                                    "toggle audio only"
                                },
                            ),
                            (
                                kb.captions.as_str(),
                                if pt {
                                    "ativar ou desativar legendas"
                                } else {
                                    "toggle captions"
                                },
                            ),
                            (
                                cap_size_keys.as_str(),
                                if pt {
                                    "aumentar ou diminuir fonte das legendas"
                                } else {
                                    "increase or decrease caption font size"
                                },
                            ),
                            (
                                kb.theater.as_str(),
                                if pt { "modo teatro" } else { "theater mode" },
                            ),
                            (
                                fs_keys.as_str(),
                                if pt { "tela cheia" } else { "fullscreen" },
                            ),
                            (
                                kb.mini_player.as_str(),
                                if pt {
                                    "alternar miniplayer"
                                } else {
                                    "toggle mini player"
                                },
                            ),
                            (
                                nav_vid_keys.as_str(),
                                if pt {
                                    "vídeo anterior / próximo vídeo"
                                } else {
                                    "previous / next video"
                                },
                            ),
                            (
                                "0 – 9",
                                if pt {
                                    "ir para 0% – 90% do vídeo"
                                } else {
                                    "jump to 0% – 90% of video"
                                },
                            ),
                            (
                                "↑ / ↓",
                                if pt {
                                    "Short anterior / próximo (no modo Shorts)"
                                } else {
                                    "previous / next Short (in Shorts mode)"
                                },
                            ),
                            (
                                "Esc",
                                if pt {
                                    "fechar tela cheia, ajustes ou painel"
                                } else {
                                    "close fullscreen, settings, or panel"
                                },
                            ),
                        ] {
                            paint_shortcut_row(ui, key, action);
                            ui.add_space(3.0);
                        }
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(if pt { "Navegação" } else { "Navigation" })
                                .size(13.5)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        ui.add_space(4.0);
                        let palette_keys = format!("{} / Ctrl+P", kb.command_palette);
                        let help_keys = format!("F1 / {}", kb.show_shortcuts);
                        for (key, action) in [
                            (
                                kb.focus_search.as_str(),
                                if pt { "focar barra de pesquisa" } else { "focus search bar" },
                            ),
                            (
                                help_keys.as_str(),
                                if pt {
                                    "abrir lista de atalhos"
                                } else {
                                    "open keyboard shortcuts"
                                },
                            ),
                            (
                                palette_keys.as_str(),
                                if pt {
                                    "paleta de comandos"
                                } else {
                                    "command palette"
                                },
                            ),
                            (
                                "Alt+← / Alt+→",
                                if pt {
                                    "voltar / avançar histórico de telas"
                                } else {
                                    "navigate back / forward"
                                },
                            ),
                            ("Alt+1", if pt { "Início" } else { "Home" }),
                            ("Alt+2", "Shorts"),
                            ("Alt+3", if pt { "Inscrições" } else { "Subscriptions" }),
                            ("Alt+4", if pt { "Você" } else { "You" }),
                            ("Alt+5", if pt { "Histórico" } else { "History" }),
                            ("Alt+6", "Downloads"),
                        ] {
                            paint_shortcut_row(ui, key, action);
                            ui.add_space(3.0);
                        }
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(if pt { "Legendas" } else { "Captions" })
                                .size(13.5)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        ui.add_space(3.0);
                        ui.label(
                            RichText::new(if pt {
                                "Em Transcri\u{00e7}\u{00e3}o, os trechos s\u{00e3}o agrupados por cap\u{00ed}tulos, sincronizam com o v\u{00ed}deo e podem ser pesquisados, copiados ou exportados no menu \u{22ee}."
                            } else {
                                "In Transcript, segments are grouped by chapters, sync with playback, and can be searched, copied, or exported from the \u{22ee} menu."
                            })
                            .size(12.0)
                            .color(palette.snippet_fg),
                        );
                    });
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if styled_pill_button(ui, if pt { "Fechar" } else { "Close" }, false, &palette)
                        .clicked()
                    {
                        close_clicked = true;
                    }
                });
            },
        );
        if close_clicked {
            open = false;
        }
        self.ui.shortcuts_open = open;
    }

    pub(crate) fn handle_help_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let kb = self.settings.prefs.value.keybindings.clone();
        let (toggle_help, focus_search, escape_pressed) = ctx.input(|input| {
            let no_alt_ctrl = !input.modifiers.alt && !input.modifiers.ctrl;
            let text_question = no_alt_ctrl
                && input
                    .events
                    .iter()
                    .any(|ev| matches!(ev, egui::Event::Text(t) if t == "?"));
            let text_slash = no_alt_ctrl
                && !input.modifiers.shift
                && input
                    .events
                    .iter()
                    .any(|ev| matches!(ev, egui::Event::Text(t) if t == "/"));
            let show_shortcuts_hit = input.key_pressed(egui::Key::F1)
                || settings::Keybindings::matches_input(input, &kb.show_shortcuts)
                || (kb.show_shortcuts == "?" && text_question);
            let focus_search_hit = settings::Keybindings::matches_input(input, &kb.focus_search)
                || (kb.focus_search == "/" && text_slash);
            (
                show_shortcuts_hit,
                focus_search_hit,
                input.key_pressed(egui::Key::Escape),
            )
        });
        if focus_search && !self.ui.shortcuts_open && !self.ui.command_palette.open {
            self.ui.focus_search_requested = true;
            ctx.request_repaint();
        }
        if toggle_help {
            self.ui.shortcuts_open = !self.ui.shortcuts_open;
        }
        if escape_pressed && self.ui.shortcuts_open {
            self.ui.shortcuts_open = false;
        }
    }

    pub(crate) fn handle_player_shortcuts(
        &mut self,
        ctx: &egui::Context,
        allow_escape_exit_fullscreen: bool,
    ) {
        if !self.is_watch_page_active() || self.ui.settings_open || ctx.egui_wants_keyboard_input()
        {
            return;
        }
        let short_mode = self.is_watch_short_mode();
        let default_seek_step = f64::from(self.seek_step_secs());
        let kb = self.settings.prefs.value.keybindings.clone();
        let (
            toggle,
            back_step,
            forward_step,
            mute,
            volume_up,
            volume_down,
            captions,
            caption_size_up,
            caption_size_down,
            seek_home,
            seek_end,
            prev_chapter,
            next_chapter,
            theater,
            fullscreen,
            escape_pressed,
            mini_player,
            audio_only,
            prev_shortcut,
            next_shortcut,
            short_prev,
            short_next,
            pct_seek,
            frame_back,
            frame_forward,
            speed_down,
            speed_up,
            set_loop_a,
            set_loop_b,
            clear_loop_ab,
            screenshot,
        ) = ctx.input(|input| {
            let no_alt_ctrl = !input.modifiers.alt && !input.modifiers.ctrl;
            let no_mods = no_alt_ctrl && !input.modifiers.shift;
            let mut text_comma = false;
            let mut text_period = false;
            let mut text_lt = false;
            let mut text_gt = false;
            let mut text_plus = false;
            let mut text_minus = false;
            let mut text_open_bracket = false;
            let mut text_close_bracket = false;
            let mut text_backslash = false;
            if no_alt_ctrl {
                for ev in &input.events {
                    if let egui::Event::Text(text) = ev {
                        match text.as_str() {
                            "," if no_mods => text_comma = true,
                            "." if no_mods => text_period = true,
                            "<" => text_lt = true,
                            ">" => text_gt = true,
                            "+" => text_plus = true,
                            "-" if no_mods => text_minus = true,
                            "[" => text_open_bracket = true,
                            "]" => text_close_bracket = true,
                            "\\" => text_backslash = true,
                            _ => {}
                        }
                    }
                }
            }
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
            let prev_ch = settings::Keybindings::matches_input(input, &kb.prev_chapter);
            let next_ch = settings::Keybindings::matches_input(input, &kb.next_chapter);
            let back = if prev_ch {
                None
            } else if no_alt_ctrl && !short_mode && input.key_pressed(egui::Key::J) {
                Some(10.0)
            } else if settings::Keybindings::matches_input(input, &kb.seek_back) {
                Some(default_seek_step)
            } else {
                None
            };
            let forward = if next_ch {
                None
            } else if no_alt_ctrl && input.key_pressed(egui::Key::L) {
                Some(10.0)
            } else if settings::Keybindings::matches_input(input, &kb.seek_forward) {
                Some(default_seek_step)
            } else {
                None
            };
            let shift_only_key_pressed = |target: egui::Key| {
                input.events.iter().any(|ev| {
                    matches!(
                        ev,
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } if *key == target
                            && !modifiers.alt
                            && !modifiers.ctrl
                            && !modifiers.command
                            && (modifiers.shift || (no_alt_ctrl && input.modifiers.shift))
                    )
                })
            };
            let vol_up_hit =
                !short_mode && settings::Keybindings::matches_input(input, &kb.volume_up);
            let vol_down_hit =
                !short_mode && settings::Keybindings::matches_input(input, &kb.volume_down);
            let cap_up_hit = settings::Keybindings::matches_input(input, &kb.caption_size_up)
                || (kb.caption_size_up == "+" && text_plus);
            let cap_down_hit = settings::Keybindings::matches_input(input, &kb.caption_size_down)
                || (kb.caption_size_down == "-" && text_minus);
            let prev_vid_hit = settings::Keybindings::matches_input(input, &kb.prev_video)
                || (kb.prev_video.eq_ignore_ascii_case("Shift+P")
                    && shift_only_key_pressed(egui::Key::P));
            let next_vid_hit = settings::Keybindings::matches_input(input, &kb.next_video)
                || (kb.next_video.eq_ignore_ascii_case("Shift+N")
                    && shift_only_key_pressed(egui::Key::N));
            let frame_back_hit = settings::Keybindings::matches_input(input, &kb.frame_back)
                || (kb.frame_back == "," && no_mods && text_comma);
            let frame_forward_hit = settings::Keybindings::matches_input(input, &kb.frame_forward)
                || (kb.frame_forward == "." && no_mods && text_period);
            let speed_down_hit = settings::Keybindings::matches_input(input, &kb.speed_down)
                || (kb.speed_down == "Shift+," && text_lt);
            let speed_up_hit = settings::Keybindings::matches_input(input, &kb.speed_up)
                || (kb.speed_up == "Shift+." && text_gt);
            (
                (no_alt_ctrl && input.key_pressed(egui::Key::Space))
                    || (!short_mode && settings::Keybindings::matches_input(input, &kb.play_pause)),
                back,
                forward,
                settings::Keybindings::matches_input(input, &kb.mute),
                vol_up_hit,
                vol_down_hit,
                settings::Keybindings::matches_input(input, &kb.captions),
                cap_up_hit,
                cap_down_hit,
                settings::Keybindings::matches_input(input, &kb.seek_home),
                settings::Keybindings::matches_input(input, &kb.seek_end),
                prev_ch,
                next_ch,
                settings::Keybindings::matches_input(input, &kb.theater),
                settings::Keybindings::matches_input(input, &kb.fullscreen)
                    || settings::Keybindings::matches_input(input, &kb.fullscreen_alt),
                input.key_pressed(egui::Key::Escape),
                settings::Keybindings::matches_input(input, &kb.mini_player),
                settings::Keybindings::matches_input(input, &kb.audio_only),
                prev_vid_hit,
                next_vid_hit,
                short_mode
                    && no_alt_ctrl
                    && (input.key_pressed(egui::Key::ArrowUp) || input.key_pressed(egui::Key::K)),
                short_mode
                    && no_alt_ctrl
                    && (input.key_pressed(egui::Key::ArrowDown) || input.key_pressed(egui::Key::J)),
                pct,
                frame_back_hit,
                frame_forward_hit,
                speed_down_hit,
                speed_up_hit,
                settings::Keybindings::matches_input(input, &kb.ab_loop_a)
                    || (kb.ab_loop_a == "[" && text_open_bracket),
                settings::Keybindings::matches_input(input, &kb.ab_loop_b)
                    || (kb.ab_loop_b == "]" && text_close_bracket),
                settings::Keybindings::matches_input(input, &kb.ab_loop_clear)
                    || (kb.ab_loop_clear == "\\" && text_backslash),
                settings::Keybindings::matches_input(input, &kb.screenshot),
            )
        });

        let any_activity = toggle
            || back_step.is_some()
            || forward_step.is_some()
            || mute
            || volume_up
            || volume_down
            || captions
            || caption_size_up
            || caption_size_down
            || seek_home
            || seek_end
            || prev_chapter
            || next_chapter
            || theater
            || fullscreen
            || mini_player
            || audio_only
            || prev_shortcut
            || next_shortcut
            || short_prev
            || short_next
            || pct_seek.is_some()
            || frame_back
            || frame_forward
            || speed_down
            || speed_up
            || set_loop_a
            || set_loop_b
            || clear_loop_ab
            || screenshot;
        if any_activity {
            self.playback.overlay_last_activity = Instant::now();
        }

        if toggle || back_step.is_some() || forward_step.is_some() || mute || pct_seek.is_some() {
            let is_live = self.active_video_is_live();
            let live_dvr = self
                .playback
                .active
                .as_ref()
                .is_some_and(|active| active.live_dvr);
            let mut key_seek_pos: Option<f64> = None;
            let mut key_seek_dur: Option<f64> = None;
            let mut key_flush_pause = false;
            let mut trigger_go_live = false;
            let fallback_pos = self
                .playback
                .active
                .as_ref()
                .map_or(0.0, |active| active.position_ms as f64 / 1000.0);
            let fallback_dur = self
                .playback
                .active
                .as_ref()
                .and_then(|a| a.duration_ms)
                .filter(|&d| d > 0)
                .map(|d| d as f64 / 1000.0);

            #[cfg(windows)]
            if let Some(player) = &self.player {
                let was_ended = toggle && player.flag_cstr(c"eof-reached") == Some(true);
                let was_paused = toggle && player.flag_cstr(c"pause") == Some(true);
                let cur_pos = if back_step.is_some() || forward_step.is_some() {
                    player
                        .double_cstr(c"time-pos")
                        .filter(|&pos| pos >= 0.0)
                        .unwrap_or(fallback_pos)
                } else {
                    0.0
                };
                let cur_dur = if back_step.is_some() || forward_step.is_some() || pct_seek.is_some()
                {
                    player
                        .double_cstr(c"duration")
                        .filter(|&d| d > 0.0)
                        .or(fallback_dur)
                } else {
                    None
                };
                key_seek_dur = cur_dur;
                let pct_target = pct_seek.and_then(|pct| cur_dur.map(|dur| (pct, dur * pct)));
                let result = if toggle {
                    if was_ended && is_live {
                        trigger_go_live = true;
                        Ok(())
                    } else if was_ended {
                        player
                            .seek(0.0, "absolute+exact")
                            .and_then(|()| player.set("pause", "no"))
                    } else {
                        player.set("pause", if was_paused { "no" } else { "yes" })
                    }
                } else if let Some(step) = back_step {
                    if is_live && !live_dvr {
                        Ok(())
                    } else {
                        player.seek(-step, "relative")
                    }
                } else if let Some(step) = forward_step {
                    if is_live && !live_dvr {
                        Ok(())
                    } else if is_live {
                        let target = cur_pos + step;
                        if cur_dur.is_some_and(|dur| dur <= 1.5 || target >= (dur - 1.5).max(0.0)) {
                            trigger_go_live = true;
                            Ok(())
                        } else {
                            let clamped =
                                cur_dur.map_or(target, |dur| target.min((dur - 1.5).max(0.0)));
                            key_seek_pos = Some(clamped);
                            player.seek(clamped, "absolute+exact")
                        }
                    } else {
                        player.seek(step, "relative")
                    }
                } else if let Some((pct, target)) = pct_target {
                    if is_live && !live_dvr {
                        Ok(())
                    } else if is_live {
                        if pct >= 0.985
                            || cur_dur
                                .is_some_and(|dur| dur <= 1.5 || target >= (dur - 1.5).max(0.0))
                        {
                            trigger_go_live = true;
                            Ok(())
                        } else {
                            let clamped =
                                cur_dur.map_or(target, |dur| target.min((dur - 1.5).max(0.0)));
                            key_seek_pos = Some(clamped);
                            player.seek(clamped, "absolute+exact")
                        }
                    } else {
                        player.seek(target, "absolute+exact")
                    }
                } else if mute {
                    player.set(
                        "mute",
                        if player.flag_cstr(c"mute") == Some(true) {
                            "no"
                        } else {
                            "yes"
                        },
                    )
                } else {
                    Ok(())
                };
                match result {
                    Ok(()) => {
                        if toggle && !trigger_go_live {
                            if was_ended {
                                key_seek_pos = Some(0.0);
                            } else if !was_paused {
                                key_flush_pause = true;
                            }
                        } else if let Some(step) = back_step
                            && (!is_live || live_dvr)
                        {
                            key_seek_pos = Some((cur_pos - step).max(0.0));
                        } else if let Some(step) = forward_step
                            && !is_live
                        {
                            key_seek_pos =
                                Some(cur_dur.map_or(cur_pos + step, |d| (cur_pos + step).min(d)));
                        } else if let Some((_, target)) = pct_target
                            && !is_live
                        {
                            key_seek_pos = Some(target);
                        }
                    }
                    Err(error) => self.message = error,
                }
            } else if !is_live || live_dvr {
                key_seek_dur = fallback_dur;
                if let Some(step) = back_step {
                    key_seek_pos = Some((fallback_pos - step).max(0.0));
                } else if let Some(step) = forward_step {
                    if is_live
                        && fallback_dur.is_some_and(|dur| {
                            dur <= 1.5 || fallback_pos + step >= (dur - 1.5).max(0.0)
                        })
                    {
                        trigger_go_live = true;
                    } else {
                        let target = fallback_pos + step;
                        key_seek_pos = Some(if is_live {
                            fallback_dur.map_or(target, |dur| target.min((dur - 1.5).max(0.0)))
                        } else {
                            fallback_dur.map_or(target, |dur| target.min(dur))
                        });
                    }
                } else if let Some(pct) = pct_seek
                    && let Some(dur) = fallback_dur
                {
                    let target = dur * pct;
                    if is_live && (pct >= 0.985 || dur <= 1.5 || target >= (dur - 1.5).max(0.0)) {
                        trigger_go_live = true;
                    } else {
                        key_seek_pos = Some(if is_live {
                            target.min((dur - 1.5).max(0.0))
                        } else {
                            target
                        });
                    }
                }
            }

            if trigger_go_live {
                if let Some(video) = self.selected.clone() {
                    self.go_live_edge(&video);
                }
            } else if let Some(pos) = key_seek_pos {
                self.record_manual_seek(pos, key_seek_dur, Instant::now());
            } else if key_flush_pause {
                self.flush_playback_history_at(Instant::now());
            }
        }

        #[cfg(windows)]
        if (frame_back || frame_forward)
            && !self.active_video_is_live()
            && let Some(player) = &self.player
            && player.flag_cstr(c"pause") == Some(true)
        {
            let res = if frame_back {
                player.frame_back_step()
            } else {
                player.frame_step()
            };
            if let Err(error) = res {
                self.message = error;
            }
        }
        #[cfg(not(windows))]
        let _ = (frame_back, frame_forward);

        if speed_up != speed_down {
            #[cfg(windows)]
            let current_speed = self
                .player
                .as_ref()
                .and_then(|p| p.double_cstr(c"speed"))
                .filter(|s| s.is_finite() && *s > 0.0)
                .unwrap_or(self.playback.speed);
            #[cfg(not(windows))]
            let current_speed = self.playback.speed;
            let next_speed = overlay::step_speed_preset(current_speed, speed_up);
            self.apply_and_remember_playback_speed(next_speed);
            self.message = format!("Velocidade: {:.2}x", next_speed);
        }

        if set_loop_a {
            let pos = self.current_playback_position_for_ab(None);
            self.set_ab_loop_point_a(pos);
        }
        if set_loop_b {
            let pos = self.current_playback_position_for_ab(None);
            self.set_ab_loop_point_b(pos);
        }
        if clear_loop_ab {
            self.clear_ab_loop_points();
        }
        if screenshot && let Some(video) = self.selected.clone() {
            let pos = self.current_playback_position_for_ab(None);
            self.capture_player_screenshot(&video, pos);
        }

        if captions {
            self.toggle_player_captions(ctx);
        }
        if volume_up != volume_down {
            self.adjust_player_volume_by(if volume_up { 5.0 } else { -5.0 });
        }
        if caption_size_up != caption_size_down {
            self.step_caption_font_scale(if caption_size_up { 0.15 } else { -0.15 });
        }
        if seek_home {
            self.seek_to_seconds(0.0);
        } else if seek_end {
            let is_live = self.active_video_is_live();
            if is_live {
                if let Some(video) = self.selected.clone() {
                    self.go_live_edge(&video);
                }
            } else {
                #[cfg(windows)]
                let dur = self
                    .player
                    .as_ref()
                    .and_then(|p| p.double_cstr(c"duration"))
                    .filter(|&d| d > 0.0)
                    .or_else(|| {
                        self.playback
                            .active
                            .as_ref()
                            .and_then(|a| a.duration_ms)
                            .filter(|&d| d > 0)
                            .map(|d| d as f64 / 1000.0)
                    });
                #[cfg(not(windows))]
                let dur = self
                    .playback
                    .active
                    .as_ref()
                    .and_then(|a| a.duration_ms)
                    .filter(|&d| d > 0)
                    .map(|d| d as f64 / 1000.0);
                if let Some(d) = dur {
                    self.seek_to_seconds((d - 1.0).max(0.0));
                }
            }
        } else if prev_chapter != next_chapter {
            self.jump_to_adjacent_chapter(next_chapter);
        }
        if theater && !self.playback.fullscreen {
            self.toggle_theater_mode();
        }
        if fullscreen
            || (escape_pressed && allow_escape_exit_fullscreen && self.playback.fullscreen)
        {
            self.toggle_fullscreen(ctx);
        }
        if mini_player {
            self.toggle_mini_player(ctx);
        }
        if audio_only {
            self.toggle_audio_only_mode();
        }
        if prev_shortcut {
            let is_live = self.active_video_is_live();
            #[cfg(windows)]
            let position_secs = self
                .player
                .as_ref()
                .and_then(|player| player.double_cstr(c"time-pos"))
                .filter(|&pos| pos >= 0.0)
                .unwrap_or_else(|| {
                    self.playback
                        .active
                        .as_ref()
                        .map_or(0.0, |active| active.position_ms as f64 / 1000.0)
                });
            #[cfg(not(windows))]
            let position_secs = self
                .playback
                .active
                .as_ref()
                .map_or(0.0, |active| active.position_ms as f64 / 1000.0);
            self.trigger_previous_video_action(is_live, position_secs, false);
        } else if next_shortcut {
            self.trigger_next_video_action(false);
        } else if short_prev {
            if let Some(video) = self.next_or_prev_short(false) {
                self.playback.pending_open_video = Some(video);
            }
        } else if short_next && let Some(video) = self.next_or_prev_short(true) {
            self.playback.pending_open_video = Some(video);
        }
    }

    pub(crate) fn header_window_controls_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> egui::Rect {
        let is_maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let pt = self.ui_pt();
        let close_tip = if pt { "Fechar" } else { "Close" };
        let max_tip = if is_maximized {
            if pt { "Restaurar" } else { "Restore" }
        } else if pt {
            "Maximizar"
        } else {
            "Maximize"
        };
        let min_tip = if pt { "Minimizar" } else { "Minimize" };

        let close_resp =
            paint_window_control_button(ui, WindowControlKind::Close, close_tip, &palette);
        if close_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.add_space(2.0);
        let max_kind = if is_maximized {
            WindowControlKind::Restore
        } else {
            WindowControlKind::Maximize
        };
        let max_resp = paint_window_control_button(ui, max_kind, max_tip, &palette);
        self.desktop_win11.maximize_button_rect = Some(max_resp.rect);
        if max_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
        }
        ui.add_space(2.0);
        let min_resp =
            paint_window_control_button(ui, WindowControlKind::Minimize, min_tip, &palette);
        if min_resp.clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        close_resp.rect.union(max_resp.rect).union(min_resp.rect)
    }

    pub(crate) fn header_actions_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        show_back_button: bool,
        viewport_width: f32,
    ) -> egui::Rect {
        let mut open_my_channel = false;
        let mut open_library = false;
        let mut toggle_settings = false;
        let mut toggle_theme = false;
        let mut sign_out = false;
        let pt = self.ui_pt();
        let ultra_narrow = viewport_width < 520.0;
        let tight_gap = if ultra_narrow { 4.0 } else { 6.0 };
        let mut widgets_rect = egui::Rect::NOTHING;

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(6.0);
            widgets_rect |= self.header_window_controls_ui(ui, ctx, palette);

            let div_pad = if ultra_narrow { 4.0 } else { 8.0 };
            ui.add_space(div_pad);
            let (div_rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
            ui.painter().line_segment(
                [
                    egui::pos2(div_rect.center().x, div_rect.top()),
                    egui::pos2(div_rect.center().x, div_rect.bottom()),
                ],
                egui::Stroke::new(1.0, palette.card_alt_stroke),
            );
            widgets_rect |= div_rect;
            ui.add_space(div_pad);

            let mut open_sign_in_account = false;
            if self.auth.authenticated {
                let avatar_url = self.auth.my_channel_avatar_url.clone();
                let label = self
                    .auth
                    .my_channel_title
                    .clone()
                    .unwrap_or_else(|| if pt { "Voc\u{00ea}" } else { "You" }.to_owned());
                let avatar_resp = if self.ui.incognito_mode {
                    let (r, resp) =
                        ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
                    ui.painter()
                        .circle_filled(r.center(), 16.0, palette.header_button_bg);
                    paint_incognito_icon(ui.painter(), r.center(), palette.info_accent_fg);
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                } else {
                    self.paint_avatar_or_initials(
                        ui,
                        ctx,
                        avatar_url.as_deref(),
                        &label,
                        32.0,
                        true,
                    )
                };
                paint_focus_ring_if_focused(ui, &avatar_resp, 16.0);
                widgets_rect |= avatar_resp.rect;
                egui::Popup::menu(&avatar_resp).show(|ui| {
                    self.account_menu_contents_ui(ui, ctx);
                    ui.add_space(4.0);
                    if styled_menu_item_button(
                        ui,
                        if pt { "Seu canal" } else { "Your channel" },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        open_my_channel = true;
                        ui.close();
                    }
                    if styled_menu_item_button(
                        ui,
                        if pt { "Voc\u{00ea}" } else { "You" },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        open_library = true;
                        ui.close();
                    }
                    let appearance_label = match (self.settings.theme.value, pt) {
                        (Theme::System, true) => "Apar\u{00ea}ncia: Sistema",
                        (Theme::Dark, true) => "Apar\u{00ea}ncia: Escuro",
                        (Theme::Light, true) => "Apar\u{00ea}ncia: Claro",
                        (Theme::System, false) => "Appearance: System",
                        (Theme::Dark, false) => "Appearance: Dark",
                        (Theme::Light, false) => "Appearance: Light",
                    };
                    if styled_menu_item_button(ui, appearance_label, false, &palette).clicked() {
                        toggle_theme = true;
                        ui.close();
                    }
                    ui.separator();
                    if styled_menu_item_button(
                        ui,
                        if pt { "Sair da conta" } else { "Sign out" },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        sign_out = true;
                        ui.close();
                    }
                });
            } else {
                let accent = Color32::from_rgb(62, 166, 255);
                let button = egui::Button::new(
                    RichText::new(if pt { "Fazer login" } else { "Sign in" })
                        .size(if ultra_narrow { 12.0 } else { 13.0 })
                        .strong()
                        .color(accent),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(62, 166, 255, 110),
                ))
                .corner_radius(18);
                let login_w = if viewport_width < 560.0 { 80.0 } else { 98.0 };
                let login_resp = ui
                    .add_sized([login_w, 34.0], button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Entrar com Google para carregar inscri\u{00e7}\u{00f5}es e playlists"
                    } else {
                        "Sign in with Google to load subscriptions and playlists"
                    });
                widgets_rect |= login_resp.rect;
                if login_resp.clicked() {
                    open_sign_in_account = true;
                }
                ui.add_space(tight_gap);
                let (acct_rect, acct_resp) =
                    ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
                ui.painter().circle_filled(
                    acct_rect.center(),
                    16.0,
                    if self.ui.incognito_mode {
                        palette.info_accent_fg.gamma_multiply(0.22)
                    } else {
                        palette.header_button_bg
                    },
                );
                if self.ui.incognito_mode {
                    paint_incognito_icon(ui.painter(), acct_rect.center(), palette.info_accent_fg);
                } else {
                    ui.painter().text(
                        acct_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "F",
                        egui::FontId::proportional(13.0),
                        palette.header_button_fg,
                    );
                }
                paint_focus_ring_if_focused(ui, &acct_resp, 16.0);
                let acct_resp = acct_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Menu da conta, modo an\u{00f4}nimo e idioma"
                    } else {
                        "Account menu, Incognito & language"
                    });
                widgets_rect |= acct_rect;
                egui::Popup::menu(&acct_resp).show(|ui| {
                    self.account_menu_contents_ui(ui, ctx);
                });
            }
            if open_sign_in_account {
                self.start_reauth(ctx);
            }

            ui.add_space(tight_gap);
            let has_alert = self.settings.buffer.error.is_some()
                || self.settings.playback.error.is_some()
                || self.settings.theme.error.is_some()
                || self.settings.prefs.error.is_some()
                || self.has_pending_buffer_restart()
                || self.has_pending_hwdec_restart();
            let icon_btn_size = if ultra_narrow { 34.0 } else { 38.0 };

            // Dedicated masthead Settings Gear button.
            let (gear_rect, gear_resp) = ui.allocate_exact_size(
                egui::vec2(icon_btn_size, icon_btn_size),
                egui::Sense::click(),
            );
            widgets_rect |= gear_rect;
            if gear_resp.hovered() || self.ui.settings_open {
                ui.painter().circle_filled(
                    gear_rect.center(),
                    icon_btn_size * 0.5,
                    palette.header_button_bg,
                );
            }
            let gear_color = if has_alert {
                palette.warning_fg
            } else {
                palette.header_button_fg
            };
            paint_settings_gear_icon(ui.painter(), gear_rect.center(), gear_color);
            if has_alert {
                ui.painter().circle_filled(
                    egui::pos2(gear_rect.center().x + 6.0, gear_rect.center().y - 6.0),
                    3.0,
                    Color32::from_rgb(255, 0, 51),
                );
            }
            paint_focus_ring_if_focused(ui, &gear_resp, icon_btn_size * 0.5);
            let settings_tip = format!(
                "{} (buffer: {} \u{2022} {}: {})",
                if pt {
                    "Configura\u{00e7}\u{00f5}es"
                } else {
                    "Settings"
                },
                buffer_profile_summary(self.settings.buffer.profile),
                if pt { "tema" } else { "theme" },
                theme_name(self.settings.theme.value)
            );
            if gear_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(settings_tip)
                .clicked()
            {
                toggle_settings = true;
            }

            // Dedicated masthead Notifications Bell button with unread badge.
            ui.add_space(tight_gap);
            let unread_count = self.unread_notifications_count();
            let (bell_rect, bell_resp) = ui.allocate_exact_size(
                egui::vec2(icon_btn_size, icon_btn_size),
                egui::Sense::click(),
            );
            widgets_rect |= bell_rect;
            if bell_resp.hovered() || self.ui.notifications_open {
                ui.painter().circle_filled(
                    bell_rect.center(),
                    icon_btn_size * 0.5,
                    palette.header_button_bg,
                );
            }
            paint_notifications_bell_icon(
                ui.painter(),
                bell_rect.center(),
                palette.header_button_fg,
                unread_count,
            );
            paint_focus_ring_if_focused(ui, &bell_resp, icon_btn_size * 0.5);
            let bell_tip = if unread_count > 0 {
                if pt {
                    format!("Notifica\u{00e7}\u{00f5}es ({unread_count} n\u{00e3}o lidas)")
                } else {
                    format!("Notifications ({unread_count} unread)")
                }
            } else if pt {
                "Notifica\u{00e7}\u{00f5}es".to_owned()
            } else {
                "Notifications".to_owned()
            };
            let bell_resp = bell_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(bell_tip);
            let was_open = self.ui.notifications_open;
            if bell_resp.clicked() && !was_open {
                self.ui.notifications_open = true;
                if !self.feed.notifications_loading {
                    self.refresh_notifications_inbox(ctx);
                }
            }
            let bell_popup_open = egui::Popup::menu(&bell_resp)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    self.notifications_inbox_panel_ui(ui, ctx);
                })
                .is_some();
            if bell_popup_open || !bell_resp.clicked() || was_open {
                self.ui.notifications_open = bell_popup_open;
            }

            if self.ui.incognito_mode {
                ui.add_space(tight_gap);
                let badge_label = if pt { "An\u{00f4}nimo" } else { "Incognito" };
                let badge_btn = egui::Button::new(
                    RichText::new(badge_label)
                        .size(11.5)
                        .strong()
                        .color(palette.info_accent_fg),
                )
                .fill(palette.info_accent_fg.gamma_multiply(0.18))
                .stroke(egui::Stroke::new(
                    1.0,
                    palette.info_accent_fg.gamma_multiply(0.45),
                ))
                .corner_radius(14);
                let badge_resp = ui
                    .add_sized([74.0, 28.0], badge_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Modo an\u{00f4}nimo ativo (clique para desativar)"
                    } else {
                        "Incognito mode active (click to turn off)"
                    });
                widgets_rect |= badge_resp.rect;
                if badge_resp.clicked() {
                    self.set_incognito_mode(false, ctx);
                }
            }

            if show_back_button {
                ui.add_space(tight_gap);
                let back_size = if ultra_narrow { 34.0 } else { 36.0 };
                let back_button = egui::Button::new(RichText::new("\u{2190}").size(17.0))
                    .fill(palette.header_button_bg)
                    .corner_radius(18);
                let back_resp = ui
                    .add_sized([back_size, back_size], back_button)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt {
                        "Fechar reprodu\u{00e7}\u{00e3}o e voltar"
                    } else {
                        "Close video and go back"
                    });
                widgets_rect |= back_resp.rect;
                if back_resp.clicked() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
            }
        });

        if open_my_channel {
            self.ui.settings_open = false;
            self.ui.search_results_mode = false;
            if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                self.close_video(ctx);
            }
            if let Some(target) = self.auth.my_channel.clone() {
                self.open_channel(target, ctx);
            } else {
                self.start_fetch_my_channel(true, ctx);
            }
        }
        if open_library {
            self.navigate_from_shortcut(ctx, BrowseSection::Library);
        }
        if toggle_settings {
            if !self.ui.settings_open && self.selected.is_some() {
                let _ = self.maybe_enter_auto_mini_player(ctx);
            }
            self.ui.settings_open = !self.ui.settings_open;
            self.sync_player_window_visibility();
        }
        if toggle_theme {
            let next = if self.is_dark_theme() {
                Theme::Light
            } else {
                Theme::Dark
            };
            self.settings.apply_theme(next);
            self.sync_theme_visuals(ctx);
        }
        if sign_out {
            self.sign_out_and_reload_home(ctx);
        }
        widgets_rect
    }

    pub(crate) fn header_nav_history_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) -> egui::Rect {
        let can_back = self.ui.can_go_back();
        let can_forward = self.ui.can_go_forward();
        let back_fg = if can_back {
            palette.header_button_fg
        } else {
            palette.neutral_status_fg.gamma_multiply(0.45)
        };
        let fwd_fg = if can_forward {
            palette.header_button_fg
        } else {
            palette.neutral_status_fg.gamma_multiply(0.45)
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let back_btn = egui::Button::new(RichText::new("\u{2190}").size(15.0).color(back_fg))
                .fill(palette.header_button_bg)
                .corner_radius(15);
            let back_resp = ui
                .add_enabled(can_back, back_btn.min_size(egui::vec2(30.0, 30.0)))
                .on_hover_cursor(if can_back {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text(self.pick("Voltar (Alt+\u{2190})", "Back (Alt+\u{2190})"));
            if back_resp.clicked() {
                self.navigate_back(ctx);
            }
            let fwd_btn = egui::Button::new(RichText::new("\u{2192}").size(15.0).color(fwd_fg))
                .fill(palette.header_button_bg)
                .corner_radius(15);
            let fwd_resp = ui
                .add_enabled(can_forward, fwd_btn.min_size(egui::vec2(30.0, 30.0)))
                .on_hover_cursor(if can_forward {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::Default
                })
                .on_hover_text(
                    self.pick("Avan\u{00e7}ar (Alt+\u{2192})", "Forward (Alt+\u{2192})"),
                );
            if fwd_resp.clicked() {
                self.navigate_forward(ctx);
            }
            back_resp.rect.union(fwd_resp.rect)
        })
        .inner
    }

    pub(crate) fn clipboard_quick_open_pill_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let Some(target) = self.ui.clipboard_youtube_target.clone() else {
            return;
        };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(14)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                self.pick("Link copiado", "Copied link"),
                                target.display_label_i18n(self.ui_pt())
                            ))
                            .size(12.5)
                            .color(palette.header_button_fg),
                        );
                        let open_btn = egui::Button::new(
                            RichText::new(self.pick("Abrir", "Open"))
                                .size(12.0)
                                .strong()
                                .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .corner_radius(10);
                        if ui
                            .add(open_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(target.canonical_url())
                            .clicked()
                        {
                            self.open_direct_youtube_target(target.clone(), ctx);
                            self.ui.dismiss_clipboard_target();
                        }
                        let dismiss_btn = egui::Button::new(
                            RichText::new("\u{2715}")
                                .size(12.0)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false);
                        if ui
                            .add(dismiss_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Dispensar sugest\u{00e3}o")
                            .clicked()
                        {
                            self.ui.dismiss_clipboard_target();
                        }
                    });
                });
        });
    }

    pub(crate) fn app_update_banner_pill_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        if self.settings.app_update_banner_dismissed {
            return;
        }
        let Some(check) = self.active_app_update_available().cloned() else {
            return;
        };
        let pt = self.ui_pt();
        let busy = self.settings.app_update_task.is_running();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(
                    1.0,
                    palette.info_accent_fg.gamma_multiply(0.45),
                ))
                .corner_radius(14)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(
                            RichText::new(if pt {
                                format!(
                                    "Nova versão do FastTube disponível: v{} (atual: v{})",
                                    check.latest_version, check.current_version
                                )
                            } else {
                                format!(
                                    "New FastTube update available: v{} (current: v{})",
                                    check.latest_version, check.current_version
                                )
                            })
                            .size(12.5)
                            .color(palette.header_button_fg),
                        );
                        let update_label = if busy {
                            if pt { "Baixando..." } else { "Updating..." }
                        } else if pt {
                            "Atualizar agora"
                        } else {
                            "Update now"
                        };
                        let update_btn = egui::Button::new(
                            RichText::new(update_label)
                                .size(12.0)
                                .strong()
                                .color(palette.active_button_fg),
                        )
                        .fill(palette.active_button_bg)
                        .corner_radius(10);
                        if ui
                            .add_enabled(!busy, update_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&check.asset_name)
                            .clicked()
                        {
                            self.start_app_update_download(ctx);
                        }
                        let gh_btn = egui::Button::new(
                            RichText::new("GitHub")
                                .size(12.0)
                                .color(palette.header_button_fg),
                        )
                        .fill(palette.header_button_bg)
                        .corner_radius(10);
                        if ui
                            .add(gh_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(&check.release_url)
                            .clicked()
                        {
                            open_external_url(&check.release_url);
                        }
                        let skip_btn = egui::Button::new(
                            RichText::new(if pt { "Ignorar versão" } else { "Skip" })
                                .size(11.5)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false);
                        if ui
                            .add(skip_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            let mut prefs = self.settings.prefs.value.clone();
                            prefs.skipped_app_update_version = Some(check.latest_version.clone());
                            self.apply_app_preferences(prefs);
                        }
                        let dismiss_btn = egui::Button::new(
                            RichText::new("\u{2715}")
                                .size(12.0)
                                .color(palette.neutral_status_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .frame(false);
                        if ui
                            .add(dismiss_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(if pt { "Fechar aviso" } else { "Dismiss notice" })
                            .clicked()
                        {
                            self.settings.app_update_banner_dismissed = true;
                        }
                    });
                });
        });
    }

    pub(crate) fn header_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        self.poll_clipboard_youtube_target(ctx);
        let palette = self.palette();
        let viewport_width = ui.available_width();
        let layout_mode = compute_header_layout_mode(viewport_width);
        let show_back_button =
            layout_mode == HeaderLayoutMode::TwoRows && self.is_watch_page_active();
        ui.spacing_mut().item_spacing.y = 0.0;
        let drag_origin = ui.cursor().min;
        let header_drag_height = if layout_mode == HeaderLayoutMode::SingleRow {
            56.0
        } else {
            52.0
        };
        let drag_rect =
            egui::Rect::from_min_size(drag_origin, egui::vec2(viewport_width, header_drag_height));
        let titlebar_resp = ui.interact(
            drag_rect,
            egui::Id::new("fasttube_titlebar_drag"),
            egui::Sense::click_and_drag(),
        );
        let mut interactive_rects: Vec<egui::Rect> = Vec::with_capacity(6);
        ui.add_space(8.0);

        match layout_mode {
            HeaderLayoutMode::SingleRow => {
                ui.allocate_ui_with_layout(
                    egui::vec2(viewport_width, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(18.0);
                        interactive_rects.push(self.header_menu_button_ui(ui).rect);
                        ui.add_space(6.0);
                        let brand_resp = self.header_brand_ui(ui, palette);
                        interactive_rects.push(brand_resp.rect);
                        if brand_resp.clicked() {
                            self.handle_header_home(ctx);
                        }
                        ui.add_space(6.0);
                        interactive_rects.push(self.header_nav_history_ui(ui, ctx, palette));

                        let window_controls_w = 136.0;
                        let auth_w = if self.auth.authenticated { 38.0 } else { 140.0 };
                        let gear_and_bell_w = 88.0;
                        let incognito_w = if self.ui.incognito_mode { 80.0 } else { 0.0 };
                        let reserve_right =
                            window_controls_w + auth_w + gear_and_bell_w + incognito_w + 8.0;
                        let (leading_space, search_width) =
                            compute_header_search_layout(ui.available_width(), reserve_right);
                        ui.add_space(leading_space);
                        interactive_rects.push(self.header_search_ui(
                            ui,
                            ctx,
                            palette,
                            search_width,
                        ));
                        interactive_rects.push(self.header_actions_ui(
                            ui,
                            ctx,
                            palette,
                            false,
                            viewport_width,
                        ));
                    },
                );
            }
            HeaderLayoutMode::TwoRows => {
                ui.allocate_ui_with_layout(
                    egui::vec2(viewport_width, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let wide_two_row = viewport_width >= 540.0;
                        ui.add_space(if wide_two_row { 18.0 } else { 10.0 });
                        interactive_rects.push(self.header_menu_button_ui(ui).rect);
                        ui.add_space(if wide_two_row { 6.0 } else { 4.0 });
                        let brand_resp = self.header_brand_ui(ui, palette);
                        interactive_rects.push(brand_resp.rect);
                        if brand_resp.clicked() {
                            self.handle_header_home(ctx);
                        }
                        if viewport_width >= 580.0 {
                            ui.add_space(6.0);
                            interactive_rects.push(self.header_nav_history_ui(ui, ctx, palette));
                        }
                        interactive_rects.push(self.header_actions_ui(
                            ui,
                            ctx,
                            palette,
                            show_back_button,
                            viewport_width,
                        ));
                    },
                );
                ui.add_space(4.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(viewport_width, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.add_space(6.0);
                        let available = (ui.available_width() - 12.0).max(220.0);
                        let search_width = available.min(640.0);
                        let leftover = (ui.available_width() - search_width - 12.0).max(0.0);
                        ui.add_space(leftover * 0.5);
                        let _ = self.header_search_ui(ui, ctx, palette, search_width);
                    },
                );
            }
        }

        let (is_maximized, is_fullscreen, content_rect, pointer_pos, press_origin) =
            ctx.input(|input| {
                let vp = input.viewport();
                (
                    vp.maximized.unwrap_or(false),
                    self.playback.fullscreen || vp.fullscreen.unwrap_or(false),
                    input.content_rect(),
                    input
                        .pointer
                        .interact_pos()
                        .or_else(|| input.pointer.hover_pos()),
                    input.pointer.press_origin(),
                )
            });
        let point_hits_interactive = |pos: egui::Pos2| {
            interactive_rects
                .iter()
                .any(|rect| rect.is_positive() && rect.contains(pos))
        };
        let over_interactive = pointer_pos.is_some_and(point_hits_interactive)
            || press_origin.is_some_and(point_hits_interactive);
        let in_top_resize_edge = !is_maximized
            && !is_fullscreen
            && (pointer_pos.is_some_and(|pos| pos.y <= content_rect.top() + 4.0)
                || press_origin.is_some_and(|pos| pos.y <= content_rect.top() + 4.0));
        if !over_interactive && !in_top_resize_edge {
            if titlebar_resp.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
            } else if titlebar_resp.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }

        self.clipboard_quick_open_pill_ui(ui, ctx, palette);
        self.app_update_banner_pill_ui(ui, ctx, palette);
        ui.add_space(8.0);
    }

    pub(crate) fn unread_notifications_count(&self) -> usize {
        let innertube_unread = self
            .feed
            .notifications
            .iter()
            .filter(|n| !n.is_read)
            .count();
        let existing_urls: HashSet<&str> = self
            .feed
            .notifications
            .iter()
            .filter_map(|n| n.video_url.as_deref())
            .collect();
        let rss_unread = self
            .followed_channels
            .new_video_urls
            .iter()
            .filter(|url| !existing_urls.contains(url.as_str()))
            .count();
        innertube_unread + rss_unread
    }

    pub(crate) fn active_innertube_web_session(&self) -> Option<innertube::YouTubeWebSession> {
        if let Some(comment_sess) = &self.auth.youtube_web_session {
            let now_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if let Ok(it_sess) = history_sync::to_innertube_web_session(comment_sess, now_unix) {
                return Some(it_sess);
            }
        }
        Self::load_innertube_web_session_cached()
    }

    pub(crate) fn refresh_notifications_inbox(&mut self, ctx: &egui::Context) {
        self.start_notifications_inbox_fetch(false, None, ctx);
    }

    pub(crate) fn start_notifications_inbox_fetch(
        &mut self,
        allow_interactive: bool,
        continuation: Option<String>,
        ctx: &egui::Context,
    ) {
        let append = continuation
            .as_deref()
            .is_some_and(|tok| !tok.trim().is_empty());
        self.feed.notifications_task.cancel();
        if cfg!(test) {
            self.feed.notifications_loading = false;
            self.feed.notifications_loading_more = false;
            return;
        }
        if !append
            && !self.followed_channels.data.channels.is_empty()
            && !self.followed_channels.feed_task.is_running()
        {
            self.start_local_followed_feed(false, ctx);
        }
        let cached_session = self.auth.youtube_web_session.clone();
        let it_ctx = self.innertube_context();
        if append {
            self.feed.notifications_loading_more = true;
        } else {
            self.feed.notifications_loading = true;
        }
        self.feed.notifications_error = None;
        self.feed.notifications_needs_connect = false;

        self.feed.notifications_task.spawn_with_id_and_cancel(
            ctx.clone(),
            move |request_id, cancel| {
                let had_cached_or_dpapi = cached_session.is_some()
                    || innertube::load_web_session().ok().flatten().is_some();

                let mut comment_sess = match history_sync::acquire_web_session(
                    cached_session,
                    allow_interactive,
                    &cancel,
                ) {
                    Ok(sess) => sess,
                    Err(err) => {
                        let needs_connect = err.contains("Entre na sua conta")
                            || err.contains("expirou")
                            || err.contains("expirada")
                            || err.contains("janela de login")
                            || err.contains("authentication cookies");
                        return app_state::NotificationsInboxFetchMessage {
                            request_id,
                            append,
                            web_session: None,
                            auth_expired: needs_connect,
                            needs_connect,
                            result: Err(err),
                        };
                    }
                };

                let now_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let mut it_session =
                    match history_sync::to_innertube_web_session(&comment_sess, now_unix) {
                        Ok(sess) => sess,
                        Err(err) => {
                            let auth_expired = err.is_auth_expired();
                            return app_state::NotificationsInboxFetchMessage {
                                request_id,
                                append,
                                web_session: None,
                                auth_expired,
                                needs_connect: auth_expired,
                                result: Err(err.into_message()),
                            };
                        }
                    };

                if it_session.needs_config_refresh()
                    && innertube::refresh_web_session_config(&mut it_session, &it_ctx, &cancel)
                {
                    let _ = innertube::save_web_session(&it_session);
                }
                if let Ok(updated) = history_sync::from_innertube_web_session(&it_session) {
                    comment_sess = updated;
                }

                let first_attempt = innertube::fetch_notifications_menu(
                    &it_ctx,
                    &mut it_session,
                    continuation.as_deref(),
                    &cancel,
                );

                let menu_res = match first_attempt {
                    Ok(menu) => Ok(menu),
                    Err(err) if err.is_auth_expired() && !cancel.cancelled() => {
                        let _ = innertube::invalidate_web_session();
                        if had_cached_or_dpapi || allow_interactive {
                            match history_sync::acquire_web_session(
                                None,
                                allow_interactive,
                                &cancel,
                            ) {
                                Ok(fresh_comment) => {
                                    let now_retry = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs();
                                    match history_sync::to_innertube_web_session(
                                        &fresh_comment,
                                        now_retry,
                                    ) {
                                        Ok(mut fresh_it) => {
                                            let _ = innertube::refresh_web_session_config(
                                                &mut fresh_it,
                                                &it_ctx,
                                                &cancel,
                                            );
                                            let _ = innertube::save_web_session(&fresh_it);
                                            if let Ok(updated) =
                                                history_sync::from_innertube_web_session(&fresh_it)
                                            {
                                                comment_sess = updated;
                                            } else {
                                                comment_sess = fresh_comment;
                                            }
                                            it_session = fresh_it;
                                            innertube::fetch_notifications_menu(
                                                &it_ctx,
                                                &mut it_session,
                                                continuation.as_deref(),
                                                &cancel,
                                            )
                                        }
                                        Err(conv_err) => Err(conv_err),
                                    }
                                }
                                Err(acq_err) => {
                                    return app_state::NotificationsInboxFetchMessage {
                                        request_id,
                                        append,
                                        web_session: None,
                                        auth_expired: true,
                                        needs_connect: true,
                                        result: Err(acq_err),
                                    };
                                }
                            }
                        } else {
                            Err(err)
                        }
                    }
                    Err(err) => Err(err),
                };

                match menu_res {
                    Ok(menu) => {
                        let _ = innertube::save_web_session(&it_session);
                        let final_comment = history_sync::from_innertube_web_session(&it_session)
                            .unwrap_or(comment_sess);
                        app_state::NotificationsInboxFetchMessage {
                            request_id,
                            append,
                            web_session: Some(final_comment),
                            auth_expired: false,
                            needs_connect: false,
                            result: Ok(menu),
                        }
                    }
                    Err(err) => {
                        let auth_expired = err.is_auth_expired();
                        app_state::NotificationsInboxFetchMessage {
                            request_id,
                            append,
                            web_session: if auth_expired {
                                None
                            } else {
                                Some(comment_sess)
                            },
                            auth_expired,
                            needs_connect: auth_expired,
                            result: Err(err.into_message()),
                        }
                    }
                }
            },
        );
    }

    pub(crate) fn poll_notifications_inbox(&mut self) {
        match self
            .feed
            .notifications_task
            .poll_matching_id(|msg| msg.request_id)
        {
            async_task::AsyncTaskPoll::Ready(msg) => {
                self.feed.notifications_loading = false;
                self.feed.notifications_loading_more = false;
                if msg.auth_expired {
                    self.auth.youtube_web_session = None;
                    self.auth.youtube_web_session_checked = true;
                } else if let Some(sess) = msg.web_session {
                    self.auth.youtube_web_session = Some(sess);
                    self.auth.youtube_web_session_checked = true;
                    self.auth.cached_has_saved_profile = true;
                }
                match msg.result {
                    Ok(menu) => {
                        self.feed.notifications_loaded = true;
                        self.feed.notifications_error = None;
                        self.feed.notifications_needs_connect = false;
                        self.feed.notifications_continuation = menu.continuation;
                        if msg.append {
                            let existing_ids: HashSet<String> = self
                                .feed
                                .notifications
                                .iter()
                                .map(|n| n.id.clone())
                                .collect();
                            for item in menu.items {
                                if !existing_ids.contains(&item.id) {
                                    self.feed.notifications.push(item);
                                }
                            }
                        } else {
                            self.feed.notifications = menu.items;
                        }
                    }
                    Err(err) => {
                        self.feed.notifications_needs_connect =
                            msg.needs_connect || msg.auth_expired;
                        self.feed.notifications_error = if self.feed.notifications_needs_connect
                            && self.feed.notifications.is_empty()
                        {
                            None
                        } else {
                            Some(err)
                        };
                    }
                }
            }
            async_task::AsyncTaskPoll::Disconnected => {
                self.feed.notifications_loading = false;
                self.feed.notifications_loading_more = false;
            }
            async_task::AsyncTaskPoll::Pending | async_task::AsyncTaskPoll::Idle => {}
        }
    }

    pub(crate) fn mark_all_notifications_read(&mut self, ctx: &egui::Context) {
        let mut tokens: Vec<String> = Vec::new();
        for notif in &mut self.feed.notifications {
            if !notif.is_read {
                notif.is_read = true;
                if let Some(tok) = notif.mark_read_token.clone() {
                    tokens.push(tok);
                }
            }
        }
        self.followed_channels.new_video_urls.clear();
        if !cfg!(test)
            && !tokens.is_empty()
            && let Some(mut session) = self.active_innertube_web_session()
        {
            let it_ctx = self.innertube_context();
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let cancel = process::Cancellation::default();
                for token in tokens {
                    let _ = innertube::mutate_mark_notifications_read(
                        &token,
                        &it_ctx,
                        &mut session,
                        &cancel,
                    );
                }
                ctx_clone.request_repaint();
            });
        }
    }

    pub(crate) fn mark_notification_read(
        &mut self,
        id: &str,
        token: Option<&str>,
        ctx: &egui::Context,
    ) {
        if let Some(url) = id.strip_prefix("rss:") {
            self.followed_channels.new_video_urls.remove(url);
        }
        for notif in &mut self.feed.notifications {
            if notif.id == id {
                notif.is_read = true;
            }
        }
        if !cfg!(test)
            && let Some(tok) = token.map(str::to_owned)
            && let Some(mut session) = self.active_innertube_web_session()
        {
            let it_ctx = self.innertube_context();
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let cancel = process::Cancellation::default();
                let _ =
                    innertube::mutate_mark_notifications_read(&tok, &it_ctx, &mut session, &cancel);
                ctx_clone.request_repaint();
            });
        }
    }

    pub(crate) fn hide_notification(
        &mut self,
        id: &str,
        feedback_token: Option<&str>,
        interaction_token: Option<&str>,
        ctx: &egui::Context,
    ) {
        if let Some(url) = id.strip_prefix("rss:") {
            self.followed_channels.new_video_urls.remove(url);
        }
        self.feed.notifications.retain(|n| n.id != id);
        if cfg!(test) {
            return;
        }
        let fb_tok = feedback_token
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let int_tok = interaction_token
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if (fb_tok.is_some() || int_tok.is_some())
            && let Some(mut session) = self.active_innertube_web_session()
        {
            let it_ctx = self.innertube_context();
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
                let cancel = process::Cancellation::default();
                if let Some(tok) = fb_tok {
                    let _ = innertube::mutate_feedback(&tok, &it_ctx, &mut session, &cancel);
                } else if let Some(tok) = int_tok {
                    let _ = innertube::mutate_mark_notifications_read(
                        &tok,
                        &it_ctx,
                        &mut session,
                        &cancel,
                    );
                }
                ctx_clone.request_repaint();
            });
        }
    }

    pub(crate) fn set_incognito_mode(&mut self, enabled: bool, _ctx: &egui::Context) {
        self.ui.incognito_mode = enabled;
        if enabled {
            if let Some(cancel) = self.history.remote.watch_init_cancel.take() {
                cancel.cancel();
            }
            self.history.remote.watch_init_rx = None;
            self.history.remote.watch_tracker = None;
        }
        let pt = self.ui_pt();
        self.toasts.info(if enabled {
            if pt {
                "Modo an\u{00f4}nimo ativado: hist\u{00f3}rico e sincroniza\u{00e7}\u{00e3}o pausados."
            } else {
                "Incognito mode enabled: watch history and remote sync paused."
            }
        } else if pt {
            "Modo an\u{00f4}nimo desativado."
        } else {
            "Incognito mode disabled."
        });
    }

    pub(crate) fn set_restricted_mode(&mut self, enabled: bool, ctx: &egui::Context) {
        let mut prefs = self.settings.prefs.value.clone();
        prefs.restricted_mode = enabled;
        self.apply_app_preferences(prefs);
        let pt = self.ui_pt();
        self.toasts.info(if enabled {
            if pt {
                "Modo restrito ativado."
            } else {
                "Restricted Mode enabled."
            }
        } else if pt {
            "Modo restrito desativado."
        } else {
            "Restricted Mode disabled."
        });
        if !cfg!(test) {
            if self.ui.browse_section == BrowseSection::Explore {
                self.select_explore_category(self.feed.explore_category, ctx);
            } else if self.ui.browse_section == BrowseSection::Home {
                self.start_innertube_home_feed(None, false, true, ctx);
            }
        }
    }

    pub(crate) fn set_country_region(&mut self, country_code: Option<String>, ctx: &egui::Context) {
        let normalized = country_code
            .map(|c| c.trim().to_ascii_uppercase())
            .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()));
        let mut prefs = self.settings.prefs.value.clone();
        prefs.country_code = normalized.clone();
        self.apply_app_preferences(prefs);
        let pt = self.ui_pt();
        let label = normalized.as_deref().unwrap_or("AUTO");
        self.toasts.info(if pt {
            format!("Regi\u{00e3}o atualizada para {label}.")
        } else {
            format!("Location updated to {label}.")
        });
        if !cfg!(test) && self.ui.browse_section == BrowseSection::Explore {
            self.select_explore_category(self.feed.explore_category, ctx);
        }
    }

    pub(crate) fn set_ui_language(&mut self, lang: i18n::UiLanguage, _ctx: &egui::Context) {
        let mut prefs = self.settings.prefs.value.clone();
        prefs.language = lang;
        self.apply_app_preferences(prefs);
        let pt = self.ui_pt();
        let update_version = self
            .active_app_update_available()
            .map(|u| u.latest_version.clone());
        let video_title = self.selected.as_ref().map(|v| v.title.clone());
        self.desktop.sync_tray(
            self.desktop.hidden_to_tray,
            self.selected.is_some(),
            self.playback
                .active
                .as_ref()
                .is_some_and(|a| a.has_live_sample),
            pt,
            update_version.as_deref(),
            video_title.as_deref(),
        );
        self.toasts.info(if pt {
            "Idioma alterado para Portugu\u{00ea}s (BR)."
        } else {
            "Language switched to English."
        });
    }

    pub(crate) fn set_clipboard_preview_text(&mut self, raw: Option<&str>) {
        self.ui.clipboard_last_checked_at = Some(Instant::now());
        let normalized = raw
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        self.ui.clipboard_last_raw = normalized.clone();
        if let Some(current) = normalized {
            if self.ui.clipboard_dismissed_raw.as_deref() == Some(current.as_str()) {
                self.ui.clipboard_youtube_target = None;
            } else {
                self.ui.clipboard_dismissed_raw = None;
                self.ui.clipboard_youtube_target =
                    command_palette::parse_direct_youtube_url(&current);
            }
        } else {
            self.ui.clipboard_dismissed_raw = None;
            self.ui.clipboard_youtube_target = None;
        }
    }

    pub(crate) fn poll_clipboard_with_state<F>(
        &mut self,
        window_focused: bool,
        seq: u32,
        read_clipboard: F,
    ) -> bool
    where
        F: FnOnce() -> Option<String>,
    {
        if !window_focused || self.desktop.hidden_to_tray {
            return false;
        }
        if seq != 0 && seq == self.ui.last_clipboard_seq {
            return false;
        }
        self.ui.last_clipboard_seq = seq;
        let raw = read_clipboard();
        let trimmed = raw.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if trimmed == self.ui.clipboard_last_raw.as_deref() {
            self.ui.clipboard_last_checked_at = Some(Instant::now());
            return true;
        }
        self.set_clipboard_preview_text(raw.as_deref());
        true
    }

    pub(crate) fn poll_clipboard_youtube_target(&mut self, ctx: &egui::Context) {
        #[cfg(not(test))]
        {
            let window_focused = ctx.input(|input| input.viewport().focused.unwrap_or(false));
            if !window_focused || self.desktop.hidden_to_tray {
                return;
            }
            let seq = os_clipboard_sequence_number();
            if seq != 0 {
                let _ = self.poll_clipboard_with_state(true, seq, read_text_from_os_clipboard);
                return;
            }
            if self
                .ui
                .clipboard_last_checked_at
                .is_some_and(|at| at.elapsed() < Duration::from_millis(750))
            {
                return;
            }
            let _ = self.poll_clipboard_with_state(true, 0, read_text_from_os_clipboard);
        }
        #[cfg(test)]
        let _ = ctx;
    }

    pub(crate) fn current_navigation_route(&self) -> app_state::NavigationRoute {
        if self.ui.settings_open {
            app_state::NavigationRoute::Settings(self.settings.active_tab)
        } else if self.is_watch_page_active()
            && let Some(video) = &self.selected
        {
            app_state::NavigationRoute::Watch(video.clone())
        } else if self.channels.view_open
            && let Some(target) = self.current_channel_target()
        {
            app_state::NavigationRoute::Channel(target)
        } else {
            let search_query = (self.ui.search_results_mode && !self.feed.query.trim().is_empty())
                .then(|| self.feed.query.clone());
            app_state::NavigationRoute::Browse {
                section: self.ui.browse_section,
                search_query,
            }
        }
    }

    pub(crate) fn apply_navigation_route(
        &mut self,
        route: app_state::NavigationRoute,
        ctx: &egui::Context,
    ) {
        self.ui.nav_suppress_record = true;
        self.ui.nav_last_route = Some(route.clone());
        match route {
            app_state::NavigationRoute::Settings(tab) => {
                self.settings.active_tab = tab;
                self.ui.settings_open = true;
                self.sync_player_window_visibility();
            }
            app_state::NavigationRoute::Watch(video) => {
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
                    self.exit_in_app_mini_player(ctx);
                } else {
                    self.playback.pending_open_seek_ms = None;
                    self.playback.pending_open_video = Some(video);
                    ctx.request_repaint();
                }
            }
            app_state::NavigationRoute::Channel(target) => {
                self.ui.settings_open = false;
                self.ui.search_results_mode = false;
                self.queue.remote_playlist_id = None;
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                if self.channels.last_target.as_ref() == Some(&target)
                    && self.channels.page.is_some()
                {
                    self.channels.view_open = true;
                    self.sync_player_window_visibility();
                } else {
                    self.open_channel(target, ctx);
                }
            }
            app_state::NavigationRoute::Browse {
                section,
                search_query,
            } => {
                self.ui.settings_open = false;
                self.queue.remote_playlist_id = None;
                self.close_channel_view();
                if self.selected.is_some() && !self.maybe_enter_auto_mini_player(ctx) {
                    self.close_video(ctx);
                }
                if let Some(query) = search_query {
                    self.ui.search_results_mode = true;
                    self.ui.browse_section = BrowseSection::Home;
                    self.feed.filter = FeedFilter::All;
                    if self.feed.query != query || self.feed.videos.is_empty() {
                        self.feed.query = query;
                        self.start_search(ctx);
                    }
                } else {
                    self.ui.search_results_mode = false;
                    match section {
                        BrowseSection::Home => self.handle_header_home(ctx),
                        BrowseSection::Subscriptions => self.open_subscriptions_section(false, ctx),
                        BrowseSection::Shorts => {
                            self.open_shorts_tab(ctx);
                        }
                        other => {
                            self.ui.browse_section = other;
                            if matches!(self.feed.filter, FeedFilter::Shorts) {
                                self.feed.filter = FeedFilter::All;
                            }
                            self.invalidate_feed_cache();
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn sync_navigation_history(&mut self) {
        if self.playback.pending_open_video.is_some() {
            return;
        }
        let current = self.current_navigation_route();
        if self.ui.nav_suppress_record {
            self.ui.nav_suppress_record = false;
            self.ui.nav_last_route = Some(current);
            return;
        }
        match &self.ui.nav_last_route {
            None => {
                self.ui.nav_last_route = Some(current);
            }
            Some(previous) if *previous == current => {}
            Some(previous) => {
                self.ui.nav_back_stack.push(previous.clone());
                if self.ui.nav_back_stack.len() > 64 {
                    self.ui.nav_back_stack.remove(0);
                }
                self.ui.nav_forward_stack.clear();
                self.ui.nav_last_route = Some(current);
            }
        }
    }

    pub(crate) fn navigate_back(&mut self, ctx: &egui::Context) -> bool {
        let Some(target) = self.ui.nav_back_stack.pop() else {
            return false;
        };
        let current = self
            .ui
            .nav_last_route
            .clone()
            .unwrap_or_else(|| self.current_navigation_route());
        if current != target {
            self.ui.nav_forward_stack.push(current);
            if self.ui.nav_forward_stack.len() > 64 {
                self.ui.nav_forward_stack.remove(0);
            }
        }
        self.apply_navigation_route(target, ctx);
        true
    }

    pub(crate) fn navigate_forward(&mut self, ctx: &egui::Context) -> bool {
        let Some(target) = self.ui.nav_forward_stack.pop() else {
            return false;
        };
        let current = self
            .ui
            .nav_last_route
            .clone()
            .unwrap_or_else(|| self.current_navigation_route());
        if current != target {
            self.ui.nav_back_stack.push(current);
            if self.ui.nav_back_stack.len() > 64 {
                self.ui.nav_back_stack.remove(0);
            }
        }
        self.apply_navigation_route(target, ctx);
        true
    }

    pub(crate) fn build_command_palette_items(&self) -> Vec<command_palette::PaletteItem> {
        use command_palette::{PaletteAction, PaletteCategory, PaletteItem};

        let pt = self.ui_pt();
        let mut items = Vec::new();

        if let Some(target) = &self.ui.clipboard_youtube_target {
            items.push(PaletteItem {
                id: "action:clipboard-open".to_owned(),
                category: PaletteCategory::Action,
                title: format!(
                    "{}: {}",
                    self.pick("Abrir link da área de transferência", "Open clipboard link",),
                    target.display_label_i18n(pt)
                ),
                subtitle: target.canonical_url(),
                keywords: format!(
                    "clipboard area de transferencia colar url link {}",
                    target.canonical_url()
                ),
                shortcut: None,
                action: PaletteAction::OpenDirectTarget(target.clone()),
            });
        }

        items.push(PaletteItem {
            id: "action:toggle-theme".to_owned(),
            category: PaletteCategory::Action,
            title: if self.is_dark_theme() {
                self.pick("Alternar para tema claro", "Switch to light theme")
                    .to_owned()
            } else {
                self.pick("Alternar para tema escuro", "Switch to dark theme")
                    .to_owned()
            },
            subtitle: self
                .pick(
                    "Mudar a aparência geral da interface",
                    "Change the overall UI appearance",
                )
                .to_owned(),
            keywords: "tema claro escuro dark light theme aparencia".to_owned(),
            shortcut: None,
            action: PaletteAction::ToggleTheme,
        });

        items.push(PaletteItem {
            id: "action:toggle-theater".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Alternar modo teatro", "Toggle theater mode")
                .to_owned(),
            subtitle: self
                .pick(
                    "Expandir ou restaurar o player na página de reprodução",
                    "Expand or restore the player on the watch page",
                )
                .to_owned(),
            keywords: "teatro theater layout player".to_owned(),
            shortcut: Some("T"),
            action: PaletteAction::ToggleTheater,
        });

        items.push(PaletteItem {
            id: "action:toggle-miniplayer".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Alternar miniplayer", "Toggle mini-player")
                .to_owned(),
            subtitle: self
                .pick(
                    "Janela compacta sempre visível",
                    "Always-on-top compact window",
                )
                .to_owned(),
            keywords: "miniplayer pip flutuante janela floating".to_owned(),
            shortcut: Some("I"),
            action: PaletteAction::ToggleMiniPlayer,
        });

        items.push(PaletteItem {
            id: "action:cycle-repeat".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Alternar modo de repetição", "Cycle repeat mode")
                .to_owned(),
            subtitle: if self.ui_pt() {
                self.queue.repeat_mode.label_pt().to_owned()
            } else {
                self.queue.repeat_mode.label_en().to_owned()
            },
            keywords: "repetir repeat loop fila modo".to_owned(),
            shortcut: None,
            action: PaletteAction::CycleRepeat,
        });

        if !self.queue.data.is_empty() {
            if self.queue.data.len() > 1 {
                items.push(PaletteItem {
                    id: "action:shuffle-queue".to_owned(),
                    category: PaletteCategory::Action,
                    title: self
                        .pick("Embaralhar fila de reprodução", "Shuffle playback queue")
                        .to_owned(),
                    subtitle: format!(
                        "{} {}",
                        self.queue.data.len(),
                        self.pick("vídeo(s) na fila", "video(s) in queue")
                    ),
                    keywords: "embaralhar shuffle misturar fila queue".to_owned(),
                    shortcut: None,
                    action: PaletteAction::ShuffleQueue,
                });
            }
            items.push(PaletteItem {
                id: "action:clear-queue".to_owned(),
                category: PaletteCategory::Action,
                title: self
                    .pick("Limpar fila de reprodução", "Clear playback queue")
                    .to_owned(),
                subtitle: format!(
                    "{} {}",
                    self.queue.data.len(),
                    self.pick("vídeo(s) na fila", "video(s) in queue")
                ),
                keywords: "limpar fila apagar remover queue".to_owned(),
                shortcut: None,
                action: PaletteAction::ClearQueue,
            });
        }

        items.push(PaletteItem {
            id: "action:shortcuts".to_owned(),
            category: PaletteCategory::Action,
            title: self
                .pick("Atalhos e acessibilidade", "Shortcuts and accessibility")
                .to_owned(),
            subtitle: self
                .pick(
                    "Exibir todos os atalhos de teclado",
                    "Show all keyboard shortcuts",
                )
                .to_owned(),
            keywords: "atalhos ajuda teclado f1 acessibilidade shortcuts".to_owned(),
            shortcut: Some("F1"),
            action: PaletteAction::OpenShortcuts,
        });

        for (section, title, subtitle, keywords, shortcut) in [
            (
                BrowseSection::Home,
                self.pick("Início", "Home"),
                self.pick(
                    "Feed principal e recomendados",
                    "Main feed and recommendations",
                ),
                "inicio home feed principal",
                Some("Alt+1"),
            ),
            (
                BrowseSection::Shorts,
                "Shorts",
                self.pick("Vídeos curtos verticais", "Vertical short videos"),
                "shorts curtos verticais",
                Some("Alt+2"),
            ),
            (
                BrowseSection::Subscriptions,
                self.pick("Inscrições", "Subscriptions"),
                self.pick(
                    "Envios recentes dos canais seguidos",
                    "Recent uploads from followed channels",
                ),
                "inscricoes canais seguidos envios subscriptions",
                Some("Alt+3"),
            ),
            (
                BrowseSection::Library,
                self.pick("Você", "You"),
                self.pick(
                    "Histórico, playlists e Assistir mais tarde",
                    "History, playlists and Watch later",
                ),
                "voce biblioteca salvos assistir mais tarde playlists colecoes library you",
                Some("Alt+4"),
            ),
            (
                BrowseSection::History,
                self.pick("Histórico", "History"),
                self.pick(
                    "Vídeos reproduzidos recentemente",
                    "Recently watched videos",
                ),
                "historico recentes assistidos history",
                Some("Alt+5"),
            ),
            (
                BrowseSection::Downloads,
                "Downloads",
                self.pick(
                    "Gerenciador de downloads offline via yt-dlp",
                    "Offline download manager via yt-dlp",
                ),
                "downloads offline baixar mp4 mp3 ytdlp",
                Some("Alt+6"),
            ),
        ] {
            items.push(PaletteItem {
                id: format!("section:{section:?}"),
                category: PaletteCategory::Section,
                title: title.to_owned(),
                subtitle: subtitle.to_owned(),
                keywords: keywords.to_owned(),
                shortcut,
                action: PaletteAction::Browse(section),
            });
        }

        for tab in app_state::SettingsTab::ALL {
            items.push(PaletteItem {
                id: format!("settings:{tab:?}"),
                category: PaletteCategory::Settings,
                title: format!(
                    "{}: {}",
                    self.pick("Configurações", "Settings"),
                    tab.label(pt)
                ),
                subtitle: tab.subtitle(pt).to_owned(),
                keywords: format!(
                    "configuracoes settings ajustes preferencias {} {}",
                    tab.label_pt(),
                    tab.label_en()
                ),
                shortcut: None,
                action: PaletteAction::SettingsTab(tab),
            });
        }

        for (idx, mark) in self.library.data.bookmarks.iter().enumerate() {
            let ts = command_palette::format_timestamp_ms(mark.time_ms);
            let title = if mark.note.trim().is_empty() {
                format!("{ts} \u{2014} {}", mark.title)
            } else {
                format!("{} ({ts})", mark.note)
            };
            items.push(PaletteItem {
                id: format!("bookmark:{idx}:{}", mark.video_id),
                category: PaletteCategory::Bookmark,
                title,
                subtitle: format!("{} \u{2022} {ts}", mark.title),
                keywords: format!(
                    "marcador bookmark {} {} {}",
                    mark.title, mark.note, mark.video_id
                ),
                shortcut: None,
                action: PaletteAction::OpenBookmark {
                    bookmark: mark.clone(),
                },
            });
        }

        for (idx, video) in self.queue.data.videos().iter().enumerate() {
            items.push(PaletteItem {
                id: format!("queue:{idx}:{}", video.url),
                category: PaletteCategory::Queue,
                title: format!("{}. {}", idx + 1, video.title),
                subtitle: video.channel.clone(),
                keywords: format!("fila queue {} {}", video.title, video.channel),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video: video.clone(),
                    seek_ms: None,
                },
            });
        }

        for (idx, record) in self.library.data.watch_later.iter().enumerate() {
            items.push(PaletteItem {
                id: format!("library:wl:{idx}:{}", record.video.url),
                category: PaletteCategory::Library,
                title: record.video.title.clone(),
                subtitle: format!(
                    "{} \u{2022} {}",
                    self.pick("Assistir mais tarde", "Watch later"),
                    record.video.channel
                ),
                keywords: format!(
                    "biblioteca library assistir mais tarde watch later {} {}",
                    record.video.title, record.video.channel
                ),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video: record.video.clone(),
                    seek_ms: None,
                },
            });
        }

        for (idx, record) in self.library.data.favorites.iter().enumerate() {
            items.push(PaletteItem {
                id: format!("library:fav:{idx}:{}", record.video.url),
                category: PaletteCategory::Library,
                title: record.video.title.clone(),
                subtitle: format!(
                    "{} \u{2022} {}",
                    self.pick("Favoritos", "Favorites"),
                    record.video.channel
                ),
                keywords: format!(
                    "biblioteca library favoritos favorites {} {}",
                    record.video.title, record.video.channel
                ),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video: record.video.clone(),
                    seek_ms: None,
                },
            });
        }

        for (c_idx, collection) in self.library.data.collections.iter().enumerate() {
            for (v_idx, record) in collection.videos.iter().enumerate() {
                items.push(PaletteItem {
                    id: format!("library:col:{c_idx}:{v_idx}:{}", record.video.url),
                    category: PaletteCategory::Library,
                    title: record.video.title.clone(),
                    subtitle: format!(
                        "{} {} \u{2022} {}",
                        self.pick("Coleção", "Collection"),
                        collection.name,
                        record.video.channel
                    ),
                    keywords: format!(
                        "biblioteca library colecao collection {} {} {}",
                        collection.name, record.video.title, record.video.channel
                    ),
                    shortcut: None,
                    action: PaletteAction::OpenVideo {
                        video: record.video.clone(),
                        seek_ms: None,
                    },
                });
            }
        }

        for (idx, entry) in self.history.data.entries().iter().take(60).enumerate() {
            let video = self.find_known_video_by_id(&entry.video_id, &entry.title);
            let subtitle = if let Some(resume_ms) = entry.resume_position_ms() {
                format!(
                    "{} \u{2022} {} {}",
                    entry.channel,
                    self.pick("retomar em", "resume at"),
                    command_palette::format_timestamp_ms(resume_ms)
                )
            } else {
                entry.channel.clone()
            };
            items.push(PaletteItem {
                id: format!("history:{idx}:{}", entry.video_id),
                category: PaletteCategory::History,
                title: entry.title.clone(),
                subtitle,
                keywords: format!(
                    "historico history {} {} {}",
                    entry.title, entry.channel, entry.video_id
                ),
                shortcut: None,
                action: PaletteAction::OpenVideo {
                    video,
                    seek_ms: None,
                },
            });
        }

        if self.auth.authenticated {
            for channel in &self.feed.home.subscription_channels {
                items.push(PaletteItem {
                    id: format!("sub:{}", channel.channel_id),
                    category: PaletteCategory::Subscription,
                    title: channel.title.clone(),
                    subtitle: self
                        .pick("Abrir página do canal inscrito", "Open subscribed channel")
                        .to_owned(),
                    keywords: format!("canal inscricao {} {}", channel.title, channel.channel_id),
                    shortcut: None,
                    action: PaletteAction::OpenChannel(ChannelTarget::ChannelId(
                        channel.channel_id.clone(),
                    )),
                });
            }
        } else {
            for channel in &self.followed_channels.data.channels {
                let target = if channel.channel_id.starts_with("UC") {
                    ChannelTarget::ChannelId(channel.channel_id.clone())
                } else if let Some(handle) = &channel.handle {
                    ChannelTarget::Handle(handle.clone())
                } else {
                    ChannelTarget::ChannelId(channel.channel_id.clone())
                };
                items.push(PaletteItem {
                    id: format!("followed:{}", channel.channel_id),
                    category: PaletteCategory::Subscription,
                    title: channel.title.clone(),
                    subtitle: self
                        .pick(
                            "Abrir canal seguido localmente",
                            "Open locally followed channel",
                        )
                        .to_owned(),
                    keywords: format!(
                        "canal seguido inscricao local {} {} {}",
                        channel.title,
                        channel.channel_id,
                        channel.handle.as_deref().unwrap_or("")
                    ),
                    shortcut: None,
                    action: PaletteAction::OpenChannel(target),
                });
            }
        }

        items
    }

    pub(crate) fn execute_palette_action(
        &mut self,
        action: command_palette::PaletteAction,
        ctx: &egui::Context,
    ) {
        self.ui.command_palette.close_palette();
        match action {
            command_palette::PaletteAction::Browse(section) => match section {
                BrowseSection::Home => self.handle_header_home(ctx),
                BrowseSection::Subscriptions => self.open_subscriptions_section(false, ctx),
                BrowseSection::Shorts => {
                    self.open_shorts_tab(ctx);
                }
                other => {
                    if matches!(self.feed.filter, FeedFilter::Shorts) {
                        self.feed.filter = FeedFilter::All;
                    }
                    self.navigate_from_shortcut(ctx, other);
                }
            },
            command_palette::PaletteAction::SettingsTab(tab) => {
                self.settings.active_tab = tab;
                self.ui.settings_open = true;
                self.sync_player_window_visibility();
            }
            command_palette::PaletteAction::OpenVideo { video, seek_ms } => {
                self.open_video_at_timestamp(video, seek_ms, ctx);
            }
            command_palette::PaletteAction::OpenBookmark { bookmark } => {
                if let Some(video) = self.open_library_bookmark(&bookmark) {
                    self.ui.settings_open = false;
                    self.close_channel_view();
                    self.playback.pending_open_seek_ms = Some(bookmark.time_ms);
                    self.playback.pending_open_video = Some(video);
                }
            }
            command_palette::PaletteAction::OpenChannel(target) => {
                self.open_direct_youtube_target(
                    command_palette::DirectYouTubeTarget::Channel(target),
                    ctx,
                );
            }
            command_palette::PaletteAction::OpenDirectTarget(target) => {
                self.open_direct_youtube_target(target, ctx);
            }
            command_palette::PaletteAction::ToggleTheme => {
                let next = if self.is_dark_theme() {
                    Theme::Light
                } else {
                    Theme::Dark
                };
                self.settings.apply_theme(next);
                self.sync_theme_visuals(ctx);
            }
            command_palette::PaletteAction::ToggleTheater => {
                self.toggle_theater_mode();
            }
            command_palette::PaletteAction::ToggleMiniPlayer => {
                self.toggle_mini_player(ctx);
            }
            command_palette::PaletteAction::CycleRepeat => {
                self.cycle_repeat_mode();
            }
            command_palette::PaletteAction::ShuffleQueue => {
                self.shuffle_queue();
            }
            command_palette::PaletteAction::ClearQueue => {
                self.clear_queue();
            }
            command_palette::PaletteAction::OpenShortcuts => {
                self.ui.shortcuts_open = true;
            }
        }
        ctx.request_repaint();
    }

    pub(crate) fn handle_command_palette_global_shortcut(&mut self, ctx: &egui::Context) -> bool {
        let custom = settings::Keybindings::parse_egui_key(
            &self.settings.prefs.value.keybindings.command_palette,
        );
        let triggered = ctx.input_mut(|input| {
            let custom_hit = custom.is_some_and(|(ctrl, alt, shift, key)| {
                let mods_cmd = egui::Modifiers {
                    alt,
                    ctrl,
                    shift,
                    mac_cmd: false,
                    command: ctrl,
                };
                let mods_ctrl = egui::Modifiers {
                    alt,
                    ctrl,
                    shift,
                    mac_cmd: false,
                    command: false,
                };
                input.consume_key(mods_cmd, key) || (ctrl && input.consume_key(mods_ctrl, key))
            });
            custom_hit
                || input.consume_key(egui::Modifiers::COMMAND, egui::Key::P)
                || input.consume_key(egui::Modifiers::CTRL, egui::Key::P)
        });
        if triggered {
            self.ui.command_palette.toggle();
            ctx.request_repaint();
        }
        triggered
    }

    pub(crate) fn command_palette_cache_key(&self) -> app_state::CommandPaletteCacheKey {
        let first_history = self.history.data.entries().first();
        app_state::CommandPaletteCacheKey {
            pt: self.ui_pt(),
            theme: self.effective_theme(),
            repeat_mode: self.queue.repeat_mode,
            queue_len: self.queue.data.len(),
            queue_first_entry_id: self.queue.data.first_entry_id(),
            feed_generation: self.feed.generation,
            history_len: self.history.data.entries().len(),
            history_first_id: first_history.map(|e| e.video_id.clone()),
            history_first_pos_ms: first_history.map(|e| e.position_ms),
            library_revision: self.library.revision,
            authenticated: self.auth.authenticated,
            sub_channels_len: self.feed.home.subscription_channels.len(),
            followed_channels_len: self.followed_channels.data.channels.len(),
            clipboard_raw: self.ui.clipboard_last_raw.clone(),
            query: self.ui.command_palette.query.clone(),
        }
    }

    pub(crate) fn ensure_command_palette_cache(&mut self) {
        let key = self.command_palette_cache_key();
        if self.ui.command_palette_cache_key.as_ref() == Some(&key) {
            return;
        }
        let pt = key.pt;
        let mut items = self.build_command_palette_items();
        let has_direct_query =
            command_palette::parse_direct_youtube_target(&self.ui.command_palette.query)
                .map(|direct| {
                    let canonical = direct.canonical_url();
                    let display = direct.display_label_i18n(pt);
                    items.insert(
                        0,
                        command_palette::PaletteItem {
                            id: "dynamic:direct-url".to_owned(),
                            category: command_palette::PaletteCategory::Action,
                            title: format!("{} {display}", self.pick("Abrir", "Open")),
                            subtitle: canonical,
                            keywords: self.ui.command_palette.query.clone(),
                            shortcut: Some("Enter"),
                            action: command_palette::PaletteAction::OpenDirectTarget(direct),
                        },
                    );
                })
                .is_some();
        let mut ranked =
            command_palette::filter_and_rank(&self.ui.command_palette.query, &items, 12);
        if has_direct_query && !ranked.iter().any(|entry| entry.index == 0) {
            ranked.insert(
                0,
                command_palette::RankedPaletteEntry {
                    index: 0,
                    score: 5_000,
                },
            );
            ranked.truncate(12);
        } else if has_direct_query
            && let Some(pos) = ranked.iter().position(|entry| entry.index == 0)
            && pos > 0
        {
            let entry = ranked.remove(pos);
            ranked.insert(0, entry);
        }
        self.ui.command_palette_cached_items = items;
        self.ui.command_palette_cached_ranked = ranked;
        self.ui.command_palette_cached_has_direct = has_direct_query;
        self.ui.command_palette_cache_key = Some(key);
        self.ui.command_palette_rebuilds = self.ui.command_palette_rebuilds.wrapping_add(1);
    }

    pub(crate) fn command_palette_ui(&mut self, ctx: &egui::Context) {
        let open_t = ctx.animate_bool_with_time(
            egui::Id::new("command_palette_anim"),
            self.ui.command_palette.open,
            0.12,
        );
        if !self.ui.command_palette.open {
            return;
        }
        let pt = self.ui_pt();
        let palette = self.palette();

        let viewport_rect = ctx.input(|input| input.content_rect());
        let modal_width = (viewport_rect.width() - 32.0).clamp(300.0, 580.0);

        let mut close_requested = false;
        let mut activated_action: Option<command_palette::PaletteAction> = None;

        let scrim_alpha = (125.0 * open_t.max(0.75)).round() as u8;
        egui::Area::new(egui::Id::new("command_palette_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(viewport_rect.min)
            .show(ctx, |ui| {
                let (_, response) =
                    ui.allocate_exact_size(viewport_rect.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    viewport_rect,
                    0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, scrim_alpha),
                );
                if response.clicked() {
                    close_requested = true;
                }
            });

        let outer_width = modal_width + 24.0;
        let modal_pos = egui::pos2(
            (viewport_rect.center().x - outer_width * 0.5).max(viewport_rect.left() + 8.0),
            viewport_rect.top() + 68.0,
        );
        egui::Area::new(egui::Id::new("command_palette_modal"))
            .order(egui::Order::Foreground)
            .fixed_pos(modal_pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(14)
                    .inner_margin(egui::Margin::same(12))
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(0, 0, 0, 150),
                    })
                    .show(ui, |ui| {
                        ui.set_width(modal_width);
                        let input_resp = ui.add_sized(
                            [modal_width, 34.0],
                            egui::TextEdit::singleline(&mut self.ui.command_palette.query)
                                .hint_text(if pt {
                                    "Buscar comando, seção, vídeo, marcador ou link do YouTube..."
                                } else {
                                    "Search command, section, video, bookmark, or YouTube link..."
                                }),
                        );
                        if self.ui.command_palette.focus_requested {
                            input_resp.request_focus();
                            self.ui.command_palette.focus_requested = false;
                        }
                        if input_resp.changed() {
                            self.ui.command_palette.selected_index = 0;
                        }

                        self.ensure_command_palette_cache();
                        let items = &self.ui.command_palette_cached_items;
                        let ranked = &self.ui.command_palette_cached_ranked;

                        let (up_pressed, down_pressed, enter_pressed) = ctx.input(|input| {
                            (
                                input.key_pressed(egui::Key::ArrowUp),
                                input.key_pressed(egui::Key::ArrowDown),
                                input.key_pressed(egui::Key::Enter),
                            )
                        });
                        if up_pressed {
                            self.ui.command_palette.move_selection(-1, ranked.len());
                        }
                        if down_pressed {
                            self.ui.command_palette.move_selection(1, ranked.len());
                        }
                        if !ranked.is_empty()
                            && self.ui.command_palette.selected_index >= ranked.len()
                        {
                            self.ui.command_palette.selected_index = 0;
                        }

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);

                        if ranked.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new(self.pick(
                                        "Nenhum resultado correspondente.",
                                        "No matching results.",
                                    ))
                                    .color(palette.neutral_status_fg),
                                );
                                ui.add_space(12.0);
                            });
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("command_palette_results_scroll")
                                .max_height(360.0)
                                .show(ui, |ui| {
                                    for (row_idx, entry) in ranked.iter().enumerate() {
                                        let item = &items[entry.index];
                                        let selected =
                                            row_idx == self.ui.command_palette.selected_index;
                                        let row_fill = if selected {
                                            palette.active_button_bg
                                        } else {
                                            Color32::TRANSPARENT
                                        };
                                        let title_color = if selected {
                                            palette.active_button_fg
                                        } else {
                                            palette.channel_fg
                                        };
                                        let sub_color = if selected {
                                            palette.active_button_fg.gamma_multiply(0.8)
                                        } else {
                                            palette.neutral_status_fg
                                        };
                                        let row_resp = ui
                                            .push_id(&item.id, |ui| {
                                                egui::Frame::new()
                                                    .fill(row_fill)
                                                    .corner_radius(8)
                                                    .inner_margin(egui::Margin::symmetric(10, 6))
                                                    .show(ui, |ui| {
                                                        ui.set_width(
                                                            (modal_width - 20.0).max(200.0),
                                                        );
                                                        ui.horizontal(|ui| {
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(&item.title)
                                                                        .size(13.5)
                                                                        .strong()
                                                                        .color(title_color),
                                                                );
                                                                if !item.subtitle.is_empty() {
                                                                    ui.label(
                                                                        RichText::new(
                                                                            &item.subtitle,
                                                                        )
                                                                        .size(11.5)
                                                                        .color(sub_color),
                                                                    );
                                                                }
                                                            });
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    if let Some(shortcut) =
                                                                        item.shortcut
                                                                    {
                                                                        ui.label(
                                                                            RichText::new(
                                                                                shortcut,
                                                                            )
                                                                            .size(11.0)
                                                                            .monospace()
                                                                            .color(sub_color),
                                                                        );
                                                                        ui.add_space(6.0);
                                                                    }
                                                                    ui.label(
                                                                        RichText::new(
                                                                            item.category.label(pt),
                                                                        )
                                                                        .size(11.0)
                                                                        .color(sub_color),
                                                                    );
                                                                },
                                                            );
                                                        });
                                                    })
                                                    .response
                                                    .interact(egui::Sense::click())
                                            })
                                            .inner
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if selected && (up_pressed || down_pressed) {
                                            row_resp.scroll_to_me(Some(egui::Align::Center));
                                        }
                                        if row_resp.clicked() {
                                            activated_action = Some(item.action.clone());
                                        }
                                        ui.add_space(2.0);
                                    }
                                });

                            if enter_pressed
                                && activated_action.is_none()
                                && let Some(entry) =
                                    ranked.get(self.ui.command_palette.selected_index)
                            {
                                activated_action = Some(items[entry.index].action.clone());
                            }
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(self.pick(
                                    "\u{2191}\u{2193} navegar  \u{2022}  Enter executar  \u{2022}  Esc fechar",
                                    "\u{2191}\u{2193} navigate  \u{2022}  Enter run  \u{2022}  Esc close",
                                ))
                                .size(11.0)
                                .color(palette.neutral_status_fg),
                            );
                        });
                    });
            });

        if let Some(action) = activated_action {
            self.execute_palette_action(action, ctx);
        } else if close_requested {
            self.ui.command_palette.close_palette();
        }
    }
}
