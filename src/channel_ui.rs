use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, Color32, RichText};

impl FastTube {
    pub(crate) fn channel_header_toolbar_ui(
        &self,
        ui: &mut egui::Ui,
        retry_target: Option<&ChannelTarget>,
        loading: bool,
        current_limit: usize,
        palette: ThemePalette,
    ) -> ChannelHeaderActions {
        let mut actions = ChannelHeaderActions::default();
        ui.horizontal(|ui| {
            let back_btn = egui::Button::new(
                RichText::new("← Voltar")
                    .size(12.5)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .stroke(egui::Stroke::NONE)
            .corner_radius(16);
            if ui
                .add(back_btn)
                .on_hover_text("Fechar página do canal e retornar aos resultados")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                actions.back_to_results = true;
            }
            if let Some(target) = retry_target {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let refresh_btn = egui::Button::new(
                        RichText::new("↻ Atualizar")
                            .size(12.5)
                            .color(palette.header_button_fg),
                    )
                    .fill(palette.header_button_bg)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(16);
                    if ui
                        .add_enabled(!loading, refresh_btn)
                        .on_hover_text("Recarregar do YouTube ignorando o cache local")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        actions.reload_target = Some(target.clone());
                        actions.force_refresh = true;
                    }
                    let mut limit = current_limit;
                    egui::ComboBox::from_id_salt("channel_video_limit")
                        .selected_text(format!("{limit} vídeos"))
                        .show_ui(ui, |ui| {
                            for option in [20usize, 30, 50, search::MAX_CHANNEL_VIDEOS_LIMIT] {
                                if ui
                                    .selectable_value(
                                        &mut limit,
                                        option,
                                        format!("{option} vídeos"),
                                    )
                                    .clicked()
                                {
                                    actions.new_limit = Some(option);
                                }
                            }
                        });
                    ui.label(
                        RichText::new("Limite")
                            .small()
                            .color(palette.neutral_status_fg),
                    );
                });
            }
        });
        ui.add_space(6.0);
        actions
    }

    pub(crate) fn handle_channel_card_actions(
        &mut self,
        ctx: &egui::Context,
        card_action: &SearchCardAction,
        chosen: &mut Option<Video>,
        video: &Video,
    ) {
        if card_action.open_clicked {
            *chosen = Some(video.clone());
        }
        if card_action.enqueue_clicked && self.enqueue_video(video.clone()).is_ok() {
            self.message = self.pick("Adicionado à fila.", "Added to queue.").into();
        }
        if let Some(fmt) = card_action.download_format {
            self.start_download_video(video.clone(), fmt, ctx);
        }
        if card_action.toggle_watch_later {
            self.library.data.toggle_watch_later(video.clone());
            self.save_library();
            let notice = self
                .pick(
                    "Lista \"Assistir mais tarde\" atualizada.",
                    "\"Watch later\" list updated.",
                )
                .to_owned();
            self.message = notice.clone();
            self.toasts.success(notice);
        }
        if card_action.open_save_modal {
            self.feed.search_save_modal_video = Some(video.clone());
            if self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
                && self.auth.remote_playlists_rx.is_none()
            {
                self.start_fetch_remote_playlists(ctx);
            }
        }
        if card_action.share_clicked {
            self.open_share_modal(video);
        }
        if card_action.open_report_modal {
            self.feed.search_report_reason = None;
            self.feed.search_report_details.clear();
            self.feed.search_report_modal_video = Some(video.clone());
        }
    }

    pub(crate) fn channel_card_menu_ui(
        &self,
        ui: &mut egui::Ui,
        video: &Video,
        palette: &ThemePalette,
        action: &mut SearchCardAction,
    ) {
        let pt = self.ui_pt();
        let vid = history::youtube_video_id(&video.url);
        let in_watch_later = self.library.data.watch_later.iter().any(|v| {
            vid.as_deref()
                .is_some_and(|id| history::youtube_video_id(&v.video.url).as_deref() == Some(id))
                || v.video.url == video.url
        });
        let more_resp = paint_more_dots_button(ui, palette);
        egui::Popup::menu(&more_resp).show(|ui| {
            ui.set_min_width(240.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Queue,
                if pt {
                    "Adicionar à fila"
                } else {
                    "Add to queue"
                },
                palette,
            )
            .clicked()
            {
                action.enqueue_clicked = true;
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
            if search_menu_item_ui(ui, SearchMenuIcon::Clock, wl_label, palette).clicked() {
                action.toggle_watch_later = true;
                ui.close();
            }
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Bookmark,
                if pt {
                    "Salvar na playlist"
                } else {
                    "Save to playlist"
                },
                palette,
            )
            .clicked()
            {
                action.open_save_modal = true;
                ui.close();
            }
            ui.menu_button(if pt { "⬇ Baixar" } else { "⬇ Download" }, |ui| {
                for fmt in downloads::DownloadFormat::ALL {
                    let lbl = if pt { fmt.label_pt() } else { fmt.label_en() };
                    if ui.button(lbl).clicked() {
                        action.download_format = Some(fmt);
                        ui.close();
                    }
                }
            });
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Share,
                if pt { "Compartilhar" } else { "Share" },
                palette,
            )
            .clicked()
            {
                action.share_clicked = true;
                ui.close();
            }
            ui.separator();
            if search_menu_item_ui(
                ui,
                SearchMenuIcon::Flag,
                if pt { "Denunciar" } else { "Report" },
                palette,
            )
            .clicked()
            {
                action.open_report_modal = true;
                ui.close();
            }
        });
    }

    pub(crate) fn channel_grid_video_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        width: f32,
        is_playing: bool,
        palette: &ThemePalette,
    ) -> SearchCardAction {
        let mut action = SearchCardAction::default();
        let metadata = self.metadata_for(&video.url).cloned();
        let is_live = metadata.as_ref().is_some_and(|m| {
            m.is_live
                || m.badges
                    .iter()
                    .any(|b| b.eq_ignore_ascii_case("ao vivo") || b.eq_ignore_ascii_case("live"))
        });
        let is_music = recommendations::is_music_video(video, metadata.as_ref());
        let progress_ratio = self.youtube_search_progress_ratio(&video.url);
        let thumb_h = (width * 9.0 / 16.0).round();

        ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let thumb_resp = self.thumbnails.show_rounded(
                ui,
                ctx,
                &video.thumbnail,
                egui::vec2(width, thumb_h),
                12.0,
            );
            let overlay_dur = if is_live { "" } else { video.duration.as_str() };
            paint_thumbnail_overlays(ui, thumb_resp.rect, overlay_dur, progress_ratio, is_music);
            if thumb_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action.open_clicked = true;
            }

            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let title_w = (width - 28.0).max(60.0);
                ui.vertical(|ui| {
                    ui.set_width(title_w);
                    ui.set_max_width(title_w);
                    if is_playing {
                        ui.label(
                            RichText::new("▶ Em reprodução")
                                .small()
                                .strong()
                                .color(palette.playing_badge_fg),
                        );
                    }
                    let title_limit = if width < 220.0 { 48 } else { 68 };
                    let title_btn = egui::Button::new(
                        RichText::new(truncate_text_chars(&video.title, title_limit))
                            .size(14.5)
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
                        action.open_clicked = true;
                    }
                    if is_live {
                        ui.add_space(2.0);
                        paint_live_now_badge(ui);
                    }
                    if let Some(subline) =
                        format_youtube_card_subline(&video.duration, metadata.as_ref())
                    {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(subline)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    self.channel_card_menu_ui(ui, video, palette, &mut action);
                });
            });
        });

        action
    }

    pub(crate) fn channel_short_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        width: f32,
        palette: &ThemePalette,
    ) -> SearchCardAction {
        let mut action = SearchCardAction::default();
        let meta = self.metadata_for(&video.url).cloned();
        let card_h = (width * 16.0 / 9.0).round();

        ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
            let thumb_resp = self.thumbnails.show_cover_rounded(
                ui,
                ctx,
                &video.thumbnail,
                egui::vec2(width, card_h),
                10.0,
            );
            if recommendations::is_recent_upload(meta.as_ref(), None) {
                paint_short_novo_badge(ui, thumb_resp.rect);
            }
            if thumb_resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action.open_clicked = true;
            }
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let title_w = (width - 28.0).max(56.0);
                ui.vertical(|ui| {
                    ui.set_width(title_w);
                    ui.set_max_width(title_w);
                    let title_resp = ui
                        .add(
                            egui::Button::new(
                                RichText::new(truncate_text_chars(&video.title, 48))
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
                        action.open_clicked = true;
                    }
                    let views_line = meta
                        .as_ref()
                        .and_then(|m| m.view_count)
                        .map(recommendations::format_youtube_compact_views)
                        .or_else(|| {
                            meta.as_ref()
                                .and_then(|m| m.view_count_text.as_deref())
                                .map(str::trim)
                                .filter(|t| !t.is_empty())
                                .map(str::to_owned)
                        });
                    if let Some(sub) = views_line {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(sub)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    self.channel_card_menu_ui(ui, video, palette, &mut action);
                });
            });
        });

        action
    }

    pub(crate) fn channel_subscribe_button_ui(
        ui: &mut egui::Ui,
        subscribed: Option<bool>,
        pending: bool,
        local_only: bool,
        pt: bool,
        palette: &ThemePalette,
    ) -> bool {
        let is_sub = subscribed == Some(true);
        let fill = if is_sub {
            palette.header_button_bg
        } else {
            palette.active_button_bg
        };
        let fg = if is_sub {
            palette.header_button_fg
        } else {
            palette.active_button_fg
        };
        if !is_sub {
            let label = if local_only {
                if pt { "Seguir canal" } else { "Follow channel" }
            } else if pt {
                "Inscrever-se"
            } else {
                "Subscribe"
            };
            let btn = egui::Button::new(RichText::new(label).size(14.0).strong().color(fg))
                .fill(fill)
                .stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(114.0, 36.0))
                .corner_radius(18);
            return ui
                .add_enabled(!pending, btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked();
        }
        let sense = if pending {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(132.0, 36.0), sense);
        let bg = if response.hovered() && !pending {
            palette.card_alt_bg
        } else {
            fill
        };
        ui.painter().rect_filled(rect, 18.0, bg);
        let icon_color = if pending {
            palette.neutral_status_fg
        } else {
            fg
        };
        paint_yt_icon(
            ui.painter(),
            egui::pos2(rect.left() + 22.0, rect.center().y),
            YtSidebarIcon::Bell,
            icon_color,
            false,
        );
        let active_label = if local_only {
            if pt { "Seguindo" } else { "Following" }
        } else if pt {
            "Inscrito"
        } else {
            "Subscribed"
        };
        ui.painter().text(
            egui::pos2(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            active_label,
            egui::FontId::proportional(14.0),
            icon_color,
        );
        paint_yt_icon(
            ui.painter(),
            egui::pos2(rect.right() - 18.0, rect.center().y),
            YtSidebarIcon::ChevronDown,
            icon_color,
            false,
        );
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    }

    pub(crate) fn channel_banner_header_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        compact: bool,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        let pt = self.ui_pt();
        let local_only = !self.auth.authenticated;
        let subscribed = if local_only {
            Some(
                self.followed_channels
                    .data
                    .is_following_target(Some(&page.target), page.channel_id.as_deref()),
            )
        } else {
            self.subscription_status_for(Some(&page.target), page.channel_id.as_deref())
                .map(|s| s.subscription_id.is_some())
        };
        let sub_pending = !local_only && self.auth.sub_toggle_rx.is_some();
        let mut sub_clicked = false;
        let mut open_about_modal = false;
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let link_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };

        if let Some(banner_url) = page
            .banner_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            let banner_height = if compact {
                86.0
            } else {
                (card_max_w * 172.0 / 1070.0).clamp(132.0, 198.0).round()
            };
            self.thumbnails.show_cover_rounded(
                ui,
                ctx,
                banner_url,
                egui::vec2(card_max_w, banner_height),
                12.0,
            );
            ui.add_space(if compact { 12.0 } else { 18.0 });
        }

        ui.scope(|ui| {
            ui.set_max_width(card_max_w);
            ui.horizontal_top(|ui| {
                let title = channel_page_display_title(page);
                let avatar_size = if compact { 68.0 } else { 160.0 };
                let _ = self.paint_avatar_or_initials(
                    ui,
                    ctx,
                    page.avatar_url.as_deref(),
                    title,
                    avatar_size,
                    false,
                );
                ui.add_space(if compact { 12.0 } else { 22.0 });
                let info_w =
                    (card_max_w - avatar_size - if compact { 16.0 } else { 28.0 }).max(140.0);
                ui.vertical(|ui| {
                    ui.set_width(info_w);
                    ui.set_max_width(info_w);
                    ui.add_space(if compact { 0.0 } else { 4.0 });

                    // Title + Verified check badge
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(
                            RichText::new(title)
                                .size(if compact { 20.0 } else { 36.0 })
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if page.verified == Some(true) {
                            let badge_d = if compact { 14.0 } else { 18.0 };
                            paint_verified_badge_sized(ui, &palette, badge_d);
                        }
                    });
                    ui.add_space(4.0);

                    // Handle · Subscribers · Videos line
                    let handle_str = page
                        .handle
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let subs_str = page
                        .subscriber_count_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| page.subscriber_count.map(format_subscribers_pt));
                    let videos_str = page
                        .video_count_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| {
                            (!page.videos.is_empty()).then(|| {
                                if page.videos.len() == 1 {
                                    "1 vídeo".to_owned()
                                } else {
                                    format!("{} vídeos", page.videos.len())
                                }
                            })
                        });
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let mut wrote_any = false;
                        if let Some(handle) = handle_str {
                            ui.label(
                                RichText::new(handle)
                                    .size(if compact { 12.5 } else { 14.0 })
                                    .color(palette.channel_fg),
                            );
                            wrote_any = true;
                        }
                        if let Some(subs) = subs_str {
                            if wrote_any {
                                ui.label(
                                    RichText::new(" · ")
                                        .size(if compact { 12.5 } else { 14.0 })
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            ui.label(
                                RichText::new(subs)
                                    .size(if compact { 12.5 } else { 14.0 })
                                    .color(palette.neutral_status_fg),
                            );
                            wrote_any = true;
                        }
                        if let Some(vids) = videos_str {
                            if wrote_any {
                                ui.label(
                                    RichText::new(" · ")
                                        .size(if compact { 12.5 } else { 14.0 })
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            ui.label(
                                RichText::new(vids)
                                    .size(if compact { 12.5 } else { 14.0 })
                                    .color(palette.neutral_status_fg),
                            );
                        }
                    });

                    // Description preview + "...mais"
                    let desc_preview = page
                        .description_preview
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| {
                            clean_description(page.description.as_deref()).and_then(|clean| {
                                description_snippet(Some(&clean), if compact { 60 } else { 78 })
                            })
                        });
                    if desc_preview.is_some()
                        || !page.links.is_empty()
                        || page.country.is_some()
                        || page.joined_date_text.is_some()
                    {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if let Some(snippet) = desc_preview {
                                ui.label(
                                    RichText::new(snippet)
                                        .size(if compact { 12.5 } else { 13.5 })
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("...mais")
                                            .size(if compact { 12.5 } else { 13.5 })
                                            .strong()
                                            .color(palette.channel_fg),
                                    )
                                    .frame(false),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                open_about_modal = true;
                            }
                        });
                    }

                    // External links line (icon + blue primary link + "e mais N links")
                    if let Some(first_link) = page.links.first() {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            paint_link_chain_icon(ui, palette.neutral_status_fg);
                            let link_label = if first_link.title.trim().is_empty() {
                                first_link.url.as_str()
                            } else {
                                first_link.title.as_str()
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(link_label).size(13.5).color(link_blue),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text(&first_link.url)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                open_external_url(&first_link.url);
                            }
                            if page.links.len() > 1 || page.more_links_label.is_some() {
                                let more_text =
                                    page.more_links_label.clone().unwrap_or_else(|| {
                                        if page.links.len() == 2 {
                                            "e mais 1 link".to_owned()
                                        } else {
                                            format!("e mais {} links", page.links.len() - 1)
                                        }
                                    });
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(more_text)
                                                .size(13.5)
                                                .strong()
                                                .color(palette.channel_fg),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    open_about_modal = true;
                                }
                            }
                        });
                    }

                    ui.add_space(if compact { 8.0 } else { 12.0 });
                    if Self::channel_subscribe_button_ui(
                        ui,
                        subscribed,
                        sub_pending,
                        local_only,
                        pt,
                        &palette,
                    ) {
                        sub_clicked = true;
                    }
                });
            });
        });

        if open_about_modal {
            self.channels.about_modal_open = true;
            if !self.channels.about.loaded
                && !self.is_channel_about_loading()
                && self.channels.about.error.is_none()
            {
                self.start_channel_about(ctx);
            }
        }
        if sub_clicked {
            if local_only {
                let channel_id = page.channel_id.clone().or_else(|| match &page.target {
                    ChannelTarget::ChannelId(id) => Some(id.clone()),
                    ChannelTarget::Handle(h) => Some(h.clone()),
                });
                if let Some(cid) = channel_id {
                    self.toggle_local_follow_channel(
                        cid,
                        channel_page_display_title(page).to_owned(),
                        page.handle.clone(),
                        page.canonical_url.clone(),
                        page.avatar_url.clone(),
                    );
                }
            } else {
                self.start_toggle_subscription(page.target.clone(), page.channel_id.clone(), ctx);
            }
        }
        ui.add_space(if compact { 6.0 } else { 10.0 });
    }

    pub(crate) fn channel_tabs_bar_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        from_cache: bool,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        let mut switch_to_tab = None;
        let mut trigger_search = false;
        let mut close_search = false;

        let has_tab_flags = page.featured_video.is_some()
            || !page.home_shelves.is_empty()
            || page.has_shorts_tab
            || page.has_streams_tab
            || page.has_playlists_tab;
        let show_shorts = !has_tab_flags
            || page.has_shorts_tab
            || !self.channels.shorts.items.is_empty()
            || self.channels.tab == ChannelTab::Shorts;
        let show_streams = !has_tab_flags
            || page.has_streams_tab
            || !self.channels.streams.items.is_empty()
            || self.channels.tab == ChannelTab::Streams;
        let show_playlists = !has_tab_flags
            || page.has_playlists_tab
            || !self.channels.playlists.items.is_empty()
            || self.channels.tab == ChannelTab::Playlists;

        ui.scope(|ui| {
            ui.set_max_width(card_max_w);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                if youtube_tab_button(
                    ui,
                    "Início",
                    self.channels.tab == ChannelTab::Home,
                    &palette,
                )
                .clicked()
                {
                    switch_to_tab = Some(ChannelTab::Home);
                }
                if youtube_tab_button(
                    ui,
                    "Vídeos",
                    self.channels.tab == ChannelTab::Videos,
                    &palette,
                )
                .clicked()
                {
                    switch_to_tab = Some(ChannelTab::Videos);
                }
                if show_shorts
                    && youtube_tab_button(
                        ui,
                        "Shorts",
                        self.channels.tab == ChannelTab::Shorts,
                        &palette,
                    )
                    .clicked()
                {
                    switch_to_tab = Some(ChannelTab::Shorts);
                }
                if show_streams
                    && youtube_tab_button(
                        ui,
                        "Ao vivo",
                        self.channels.tab == ChannelTab::Streams,
                        &palette,
                    )
                    .clicked()
                {
                    switch_to_tab = Some(ChannelTab::Streams);
                }
                if show_playlists
                    && youtube_tab_button(
                        ui,
                        "Playlists",
                        self.channels.tab == ChannelTab::Playlists,
                        &palette,
                    )
                    .clicked()
                {
                    switch_to_tab = Some(ChannelTab::Playlists);
                }
                if self.channels.tab == ChannelTab::About
                    && youtube_tab_button(ui, "Sobre", true, &palette).clicked()
                {
                    switch_to_tab = Some(ChannelTab::About);
                }

                // Inline search toggle + expandable input
                let search_btn = paint_channel_search_icon_btn(
                    ui,
                    self.channels.search.input_open || self.channels.tab == ChannelTab::Search,
                    &palette,
                );
                if search_btn.on_hover_text("Pesquisar neste canal").clicked() {
                    if self.channels.search.input_open && !self.channels.search.query.trim().is_empty()
                    {
                        trigger_search = true;
                    } else {
                        self.channels.search.input_open = !self.channels.search.input_open;
                    }
                }

                if self.channels.search.input_open || self.channels.tab == ChannelTab::Search {
                    let input_id = ui.make_persistent_id("channel_inline_search_input");
                    let edit = egui::TextEdit::singleline(&mut self.channels.search.query)
                        .id(input_id)
                        .hint_text("Pesquisar")
                        .desired_width(180.0);
                    let resp = ui.add(edit);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        trigger_search = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("✕")
                                    .size(13.0)
                                    .color(palette.neutral_status_fg),
                            )
                            .frame(false),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        close_search = true;
                    }
                }

                if from_cache {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("• cache local")
                                .small()
                                .color(palette.neutral_status_fg),
                        )
                        .on_hover_text(
                            "Página recuperada do cache em memória. Use ↻ Atualizar para recarregar.",
                        );
                    });
                }
            });
            ui.add_space(-2.0);
            let sep_rect = ui.available_rect_before_wrap();
            let sep_y = sep_rect.top();
            ui.painter().line_segment(
                [
                    egui::pos2(sep_rect.left(), sep_y),
                    egui::pos2(sep_rect.left() + card_max_w, sep_y),
                ],
                egui::Stroke::new(1.0, palette.card_stroke),
            );
            ui.add_space(16.0);
        });

        if close_search {
            self.channels.search.input_open = false;
            self.channels.search.query.clear();
            self.channels.cancel_channel_search();
            if self.channels.tab == ChannelTab::Search {
                self.channels.tab = ChannelTab::Home;
            }
        } else if trigger_search {
            let query = self.channels.search.query.clone();
            self.start_channel_search(&query, ctx);
        }

        if let Some(tab) = switch_to_tab {
            self.channels.tab = tab;
            match tab {
                ChannelTab::Home | ChannelTab::Videos | ChannelTab::Search => {}
                ChannelTab::Shorts => {
                    if !self.channels.shorts.loaded
                        && !self.channels.is_shorts_loading()
                        && self.channels.shorts.error.is_none()
                    {
                        self.start_channel_shorts(ctx);
                    }
                }
                ChannelTab::Streams => {
                    if !self.channels.streams.loaded
                        && !self.channels.is_streams_loading()
                        && self.channels.streams.error.is_none()
                    {
                        self.start_channel_streams(ctx);
                    }
                }
                ChannelTab::Playlists => {
                    if !self.channels.playlists.loaded
                        && !self.is_channel_playlists_loading()
                        && self.channels.playlists.error.is_none()
                    {
                        self.start_channel_playlists(ctx);
                    }
                }
                ChannelTab::About => {
                    if !self.channels.about.loaded
                        && !self.is_channel_about_loading()
                        && self.channels.about.error.is_none()
                    {
                        self.start_channel_about(ctx);
                    }
                }
            }
        }
    }

    pub(crate) fn channel_section_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scroll_id: &str,
        compact: bool,
        max_card_width: Option<f32>,
    ) -> Option<Video> {
        let palette = self.palette();
        let loading = self.is_channel_loading();
        let loading_target = self.channels.loading_target.clone();
        let error = self.channels.error.clone();
        let page = self.channels.page.clone();
        let retry_target = self.current_channel_target();
        let from_cache = self.channels.from_cache;
        let current_limit = self.channels.video_limit;

        let mut actions = self.channel_header_toolbar_ui(
            ui,
            retry_target.as_ref(),
            loading,
            current_limit,
            palette,
        );

        let (cancel_initial, status_reload_target) = self.channel_load_status_ui(
            ui,
            loading,
            loading_target.as_ref(),
            error.as_deref(),
            retry_target.as_ref(),
            palette,
        );
        if status_reload_target.is_some() {
            actions.reload_target = status_reload_target;
        }

        let mut chosen = None;
        if let Some(page) = &page {
            let max_container = if compact {
                ui.available_width()
            } else {
                max_card_width
                    .map(|limit| ui.available_width().min(limit).min(1284.0))
                    .unwrap_or_else(|| ui.available_width().min(1284.0))
            };
            self.thumbnails.begin_pass();
            egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_channel_page"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let avail = ui.available_width();
                    let card_max_w = max_container.min(avail).max(200.0);
                    let side_pad = if !compact && avail > card_max_w {
                        ((avail - card_max_w) * 0.5).floor()
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        if side_pad > 0.0 {
                            ui.add_space(side_pad);
                        }
                        ui.vertical(|ui| {
                            ui.set_width(card_max_w);
                            ui.set_max_width(card_max_w);
                            self.channel_banner_header_ui(
                                ui, ctx, page, compact, card_max_w, palette,
                            );
                            self.channel_tabs_bar_ui(
                                ui, ctx, page, from_cache, card_max_w, palette,
                            );

                            let layout = ChannelTabLayout {
                                compact,
                                card_max_w,
                                palette,
                            };
                            match self.channels.tab {
                                ChannelTab::Home => {
                                    chosen = self.channel_home_tab_ui(ui, ctx, page, &layout);
                                }
                                ChannelTab::Videos => {
                                    chosen = self.channel_videos_tab_ui(ui, ctx, page, &layout);
                                }
                                ChannelTab::Shorts => {
                                    chosen = self.channel_tab_videos_ui(ui, ctx, true, &layout);
                                }
                                ChannelTab::Streams => {
                                    chosen = self.channel_tab_videos_ui(ui, ctx, false, &layout);
                                }
                                ChannelTab::Playlists => {
                                    chosen = self.channel_playlists_tab_ui(ui, ctx, &layout);
                                }
                                ChannelTab::Search => {
                                    chosen = self.channel_search_tab_ui(ui, ctx, &layout);
                                }
                                ChannelTab::About => {
                                    self.channel_about_tab_ui(ui, ctx, page, &layout);
                                }
                            }
                        });
                    });
                });
            self.thumbnails.end_pass(ctx);
        }

        if cancel_initial {
            self.cancel_channel_loading();
        }
        if actions.back_to_results {
            self.close_channel_view();
        }
        if let Some(limit) = actions.new_limit {
            self.channels.video_limit = limit;
        }
        if (actions.new_limit.is_some() || actions.reload_target.is_some())
            && let Some(target) = actions
                .reload_target
                .or_else(|| self.current_channel_target())
        {
            let force = actions.force_refresh && actions.new_limit.is_none();
            self.open_channel_with_options(target, ctx, force);
        }
        chosen
    }

    pub(crate) fn channel_home_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        if page.featured_video.is_none() && page.home_shelves.is_empty() {
            return self.channel_videos_tab_ui(ui, ctx, page, layout);
        }

        let palette = layout.palette;
        let current_url = self.selected.as_ref().map(|v| v.url.clone());
        let mut chosen = None;
        let mut enqueue_all: Vec<Video> = Vec::new();
        let mut open_playlist: Option<search::ChannelPlaylist> = None;

        // 1. Featured Spotlight Video (Hero Row)
        if let Some(featured) = &page.featured_video {
            let video = &featured.video;
            let meta = &featured.metadata;
            let mut card_action = SearchCardAction::default();
            let is_live = meta.is_live
                || meta
                    .badges
                    .iter()
                    .any(|b| b.eq_ignore_ascii_case("ao vivo") || b.eq_ignore_ascii_case("live"));
            let is_music = recommendations::is_music_video(video, Some(meta));
            let progress_ratio = self.youtube_search_progress_ratio(&video.url);
            let channel_name = if video.channel.trim().is_empty() {
                channel_page_display_title(page)
            } else {
                video.channel.as_str()
            };

            let paint_featured_info =
                |app: &mut Self,
                 ui: &mut egui::Ui,
                 info_w: f32,
                 card_action: &mut SearchCardAction| {
                    ui.vertical(|ui| {
                        ui.set_width(info_w);
                        ui.set_max_width(info_w);
                        ui.horizontal_top(|ui| {
                            let title_w = (info_w - 30.0).max(80.0);
                            ui.vertical(|ui| {
                                ui.set_width(title_w);
                                ui.set_max_width(title_w);
                                let title_btn = egui::Button::new(
                                    RichText::new(&video.title)
                                        .size(if layout.compact { 15.0 } else { 18.0 })
                                        .strong()
                                        .color(palette.channel_fg),
                                )
                                .frame(false)
                                .wrap();
                                if ui
                                    .add(title_btn)
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    card_action.open_clicked = true;
                                }
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.label(
                                        RichText::new(channel_name)
                                            .size(12.5)
                                            .color(palette.neutral_status_fg),
                                    );
                                    if page.verified == Some(true) {
                                        paint_verified_badge(ui, &palette);
                                    }
                                    if let Some(sub) =
                                        format_youtube_card_subline(&video.duration, Some(meta))
                                    {
                                        ui.label(
                                            RichText::new(format!(" · {sub}"))
                                                .size(12.5)
                                                .color(palette.neutral_status_fg),
                                        );
                                    }
                                });
                                if let Some(desc) =
                                    description_snippet(meta.description.as_deref(), 220)
                                {
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(desc)
                                            .size(12.5)
                                            .color(palette.neutral_status_fg),
                                    );
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                app.channel_card_menu_ui(ui, video, &palette, card_action);
                            });
                        });
                    });
                };

            ui.push_id(("chan_home_featured", &video.url), |ui| {
                if layout.compact {
                    let thumb_w = layout.card_max_w;
                    let thumb_h = (thumb_w * 9.0 / 16.0).round();
                    let thumb_resp = self.thumbnails.show_rounded(
                        ui,
                        ctx,
                        &video.thumbnail,
                        egui::vec2(thumb_w, thumb_h),
                        12.0,
                    );
                    let overlay_dur = if is_live { "" } else { video.duration.as_str() };
                    paint_thumbnail_overlays(
                        ui,
                        thumb_resp.rect,
                        overlay_dur,
                        progress_ratio,
                        is_music,
                    );
                    if thumb_resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        card_action.open_clicked = true;
                    }
                    ui.add_space(8.0);
                    paint_featured_info(self, ui, layout.card_max_w, &mut card_action);
                } else {
                    let thumb_w = (layout.card_max_w * 0.39).clamp(300.0, 424.0);
                    let thumb_h = (thumb_w * 9.0 / 16.0).round();
                    ui.horizontal_top(|ui| {
                        let thumb_resp = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            &video.thumbnail,
                            egui::vec2(thumb_w, thumb_h),
                            12.0,
                        );
                        let overlay_dur = if is_live { "" } else { video.duration.as_str() };
                        paint_thumbnail_overlays(
                            ui,
                            thumb_resp.rect,
                            overlay_dur,
                            progress_ratio,
                            is_music,
                        );
                        if thumb_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            card_action.open_clicked = true;
                        }
                        ui.add_space(16.0);
                        let info_w = (layout.card_max_w - thumb_w - 20.0).max(160.0);
                        paint_featured_info(self, ui, info_w, &mut card_action);
                    });
                }
            });
            self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
            ui.add_space(18.0);
            if !page.home_shelves.is_empty() {
                ui.separator();
                ui.add_space(14.0);
            }
        }

        // 2. Home Shelves
        for (shelf_idx, shelf) in page.home_shelves.iter().enumerate() {
            if shelf.items.is_empty() {
                continue;
            }
            ui.push_id(("chan_shelf", shelf_idx, &shelf.title), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if shelf.kind == search::ChannelShelfKind::Shorts {
                        paint_shorts_badge_icon(ui);
                    }
                    ui.label(
                        RichText::new(&shelf.title)
                            .size(if layout.compact { 16.5 } else { 20.0 })
                            .strong()
                            .color(palette.channel_fg),
                    );
                    if let Some(pid) = &shelf.playlist_id {
                        let play_btn = egui::Button::new(
                            RichText::new("▶ Reproduzir tudo")
                                .size(13.0)
                                .strong()
                                .color(palette.header_button_fg),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(16);
                        if ui
                            .add(play_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            if let Some(first) = shelf.items.first() {
                                chosen = Some(first.video.clone());
                                for extra in shelf.items.iter().skip(1) {
                                    enqueue_all.push(extra.video.clone());
                                }
                            } else {
                                open_playlist = Some(search::ChannelPlaylist {
                                    id: pid.clone(),
                                    title: shelf.title.clone(),
                                    url: format!("https://www.youtube.com/playlist?list={pid}"),
                                    thumbnail: String::new(),
                                    item_count: None,
                                    description: shelf.subtitle.clone(),
                                });
                            }
                        }
                    }
                });
                if let Some(subtitle) = &shelf.subtitle {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(subtitle)
                            .size(13.0)
                            .color(palette.neutral_status_fg),
                    );
                }
                ui.add_space(12.0);

                match shelf.kind {
                    search::ChannelShelfKind::Shorts => {
                        let gap = 12.0;
                        let cols = if layout.compact {
                            2
                        } else {
                            ((layout.card_max_w + gap) / 205.0).floor().clamp(2.0, 5.0) as usize
                        };
                        let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                            / cols as f32)
                            .max(120.0);
                        for row in shelf.items.chunks(cols) {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for (col_idx, item) in row.iter().enumerate() {
                                    let card_action = ui
                                        .push_id(("shelf_short", col_idx, &item.video.url), |ui| {
                                            self.channel_short_card_ui(
                                                ui,
                                                ctx,
                                                &item.video,
                                                card_w,
                                                &palette,
                                            )
                                        })
                                        .inner;
                                    self.handle_channel_card_actions(
                                        ctx,
                                        &card_action,
                                        &mut chosen,
                                        &item.video,
                                    );
                                }
                            });
                            ui.add_space(12.0);
                        }
                    }
                    search::ChannelShelfKind::Videos => {
                        let gap = 16.0;
                        let cols = if layout.compact {
                            1
                        } else {
                            ((layout.card_max_w + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
                        };
                        let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                            / cols as f32)
                            .max(180.0);
                        for row in shelf.items.chunks(cols) {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for (col_idx, item) in row.iter().enumerate() {
                                    let is_playing =
                                        current_url.as_deref() == Some(item.video.url.as_str());
                                    let card_action = ui
                                        .push_id(("shelf_vid", col_idx, &item.video.url), |ui| {
                                            self.channel_grid_video_card_ui(
                                                ui,
                                                ctx,
                                                &item.video,
                                                card_w,
                                                is_playing,
                                                &palette,
                                            )
                                        })
                                        .inner;
                                    self.handle_channel_card_actions(
                                        ctx,
                                        &card_action,
                                        &mut chosen,
                                        &item.video,
                                    );
                                }
                            });
                            ui.add_space(16.0);
                        }
                    }
                }
            });
            if shelf_idx + 1 < page.home_shelves.len() {
                ui.separator();
                ui.add_space(14.0);
            }
        }

        for video in enqueue_all {
            let _ = self.enqueue_video(video);
        }
        if let Some(pl) = open_playlist {
            self.channels.tab = ChannelTab::Playlists;
            self.open_channel_public_playlist(pl, ctx);
        }
        chosen
    }

    pub(crate) fn channel_tab_videos_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        is_shorts: bool,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let (loading, loaded, error, videos) = if is_shorts {
            (
                self.channels.is_shorts_loading(),
                self.channels.shorts.loaded,
                self.channels.shorts.error.clone(),
                self.channels
                    .shorts
                    .items
                    .iter()
                    .map(|item| item.video.clone())
                    .collect::<Vec<_>>(),
            )
        } else {
            (
                self.channels.is_streams_loading(),
                self.channels.streams.loaded,
                self.channels.streams.error.clone(),
                self.channels
                    .streams
                    .items
                    .iter()
                    .map(|item| item.video.clone())
                    .collect::<Vec<_>>(),
            )
        };

        let mut reload = false;
        let mut cancel = false;
        let mut chosen = None;

        if loading {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(if is_shorts {
                                "Carregando Shorts do canal..."
                            } else {
                                "Carregando transmissões do canal..."
                            })
                            .color(palette.info_accent_fg),
                        );
                        if ui.button(RichText::new("Cancelar").small()).clicked() {
                            cancel = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if let Some(err) = &error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar: {err}"))
                                .color(palette.error_banner_fg),
                        );
                        if ui
                            .button(RichText::new("↻ Tentar novamente").small())
                            .clicked()
                        {
                            reload = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if loaded && videos.is_empty() && error.is_none() {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.label(
                        RichText::new(if is_shorts {
                            "Este canal não tem Shorts públicos."
                        } else {
                            "Este canal não tem transmissões públicas."
                        })
                        .color(palette.neutral_status_fg),
                    );
                });
        } else if !videos.is_empty() {
            let current_url = self.selected.as_ref().map(|v| v.url.clone());
            if is_shorts {
                let gap = 12.0;
                let cols = if layout.compact {
                    2
                } else {
                    ((layout.card_max_w + gap) / 205.0).floor().clamp(2.0, 5.0) as usize
                };
                let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                    / cols as f32)
                    .max(120.0);
                for (row_idx, row) in videos.chunks(cols).enumerate() {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (col_idx, video) in row.iter().enumerate() {
                            let card_action = ui
                                .push_id(("chan_short", row_idx, col_idx, &video.url), |ui| {
                                    self.channel_short_card_ui(ui, ctx, video, card_w, &palette)
                                })
                                .inner;
                            self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                        }
                    });
                    ui.add_space(14.0);
                }
            } else {
                let gap = 16.0;
                let cols = if layout.compact {
                    1
                } else {
                    ((layout.card_max_w + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
                };
                let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                    / cols as f32)
                    .max(180.0);
                for (row_idx, row) in videos.chunks(cols).enumerate() {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (col_idx, video) in row.iter().enumerate() {
                            let is_playing = current_url.as_deref() == Some(video.url.as_str());
                            let card_action = ui
                                .push_id(("chan_stream", row_idx, col_idx, &video.url), |ui| {
                                    self.channel_grid_video_card_ui(
                                        ui, ctx, video, card_w, is_playing, &palette,
                                    )
                                })
                                .inner;
                            self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                        }
                    });
                    ui.add_space(18.0);
                }
            }
        }

        if cancel {
            if is_shorts {
                self.cancel_channel_shorts();
            } else {
                self.cancel_channel_streams();
            }
        } else if reload {
            if is_shorts {
                self.start_channel_shorts(ctx);
            } else {
                self.start_channel_streams(ctx);
            }
        }
        chosen
    }

    pub(crate) fn channel_videos_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let mut chosen = None;
        let mut load_more_clicked = false;
        let mut cancel_more_clicked = false;
        let mut requested_sort = None;
        let active_sort = self.channels.videos.sort;
        let sort_loading = self.channels.is_sorted_videos_loading();
        let sort_error = self.channels.videos.sorted_error.clone();
        let more_loading = self.is_channel_more_videos_loading();
        let more_error = self.channels.videos.more_error.clone();
        let has_more = self.channels.videos.has_more;
        let reached_max = self.channels.videos.reached_max
            || page.videos.len() >= search::MAX_CHANNEL_VIDEOS_LIMIT;

        // Sort chips ("Mais recentes", "Em alta", "Mais antigos")
        if !page.videos.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (sort, label) in [
                    (search::ChannelVideosSort::Latest, "Mais recentes"),
                    (search::ChannelVideosSort::Popular, "Em alta"),
                    (search::ChannelVideosSort::Oldest, "Mais antigos"),
                ] {
                    let selected = active_sort == sort;
                    let fill = if selected {
                        palette.active_button_bg
                    } else {
                        palette.header_button_bg
                    };
                    let fg = if selected {
                        palette.active_button_fg
                    } else {
                        palette.header_button_fg
                    };
                    let chip =
                        egui::Button::new(RichText::new(label).size(13.0).strong().color(fg))
                            .fill(fill)
                            .stroke(egui::Stroke::NONE)
                            .min_size(egui::vec2(0.0, 32.0))
                            .corner_radius(8);
                    if ui
                        .add(chip)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                        && active_sort != sort
                    {
                        requested_sort = Some(sort);
                    }
                }
            });
            ui.add_space(14.0);
        }

        if sort_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Ordenando vídeos do canal...")
                        .small()
                        .color(palette.info_accent_fg),
                );
            });
            ui.add_space(8.0);
        } else if let Some(err) = sort_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.label(
                        RichText::new(format!("Erro ao ordenar vídeos: {err}"))
                            .small()
                            .color(palette.error_banner_fg),
                    );
                });
            ui.add_space(8.0);
        }

        let channel_videos: Vec<Video> = if active_sort == search::ChannelVideosSort::Latest {
            page.videos.iter().map(|item| item.video.clone()).collect()
        } else if !self.channels.videos.sorted_items.is_empty() {
            self.channels
                .videos
                .sorted_items
                .iter()
                .map(|item| item.video.clone())
                .collect()
        } else {
            page.videos.iter().map(|item| item.video.clone()).collect()
        };

        if channel_videos.is_empty() {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.label(
                        RichText::new("Nenhum vídeo público listado para este canal.")
                            .color(palette.neutral_status_fg),
                    );
                });
            if let Some(sort) = requested_sort {
                self.start_channel_sorted_videos(sort, ctx);
            }
            return None;
        }

        let current_url = self.selected.as_ref().map(|video| video.url.clone());
        let gap = 16.0;
        let cols = if layout.compact {
            1
        } else {
            ((layout.card_max_w + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
        };
        let card_w =
            ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(180.0);

        for (row_idx, row) in channel_videos.chunks(cols).enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (col_idx, video) in row.iter().enumerate() {
                    let is_playing = current_url.as_deref() == Some(video.url.as_str());
                    let card_action = ui
                        .push_id(("chan_vid_grid", row_idx, col_idx, &video.url), |ui| {
                            self.channel_grid_video_card_ui(
                                ui, ctx, video, card_w, is_playing, &palette,
                            )
                        })
                        .inner;
                    self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                }
            });
            ui.add_space(18.0);
        }

        if active_sort == search::ChannelVideosSort::Latest {
            if let Some(err) = &more_error {
                egui::Frame::new()
                    .fill(palette.error_bg)
                    .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(layout.card_max_w);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("Erro ao carregar mais vídeos: {err}"))
                                    .small()
                                    .color(palette.error_banner_fg),
                            );
                            if ui
                                .button(RichText::new("↻ Tentar novamente").small())
                                .clicked()
                            {
                                load_more_clicked = true;
                            }
                        });
                    });
                ui.add_space(8.0);
            }

            if more_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Carregando mais vídeos do canal...")
                            .small()
                            .color(palette.info_accent_fg),
                    );
                    if ui
                        .button(RichText::new("Cancelar").small())
                        .on_hover_text("Cancelar carregamento adicional de vídeos")
                        .clicked()
                    {
                        cancel_more_clicked = true;
                    }
                });
            } else if reached_max {
                ui.label(
                    RichText::new(format!(
                        "Limite máximo de {} vídeos por canal atingido.",
                        search::MAX_CHANNEL_VIDEOS_LIMIT
                    ))
                    .small()
                    .color(palette.neutral_status_fg),
                );
            } else if has_more {
                let remaining =
                    search::MAX_CHANNEL_VIDEOS_LIMIT.saturating_sub(channel_videos.len());
                let step = remaining.min(search::CHANNEL_VIDEOS_PAGE_STEP);
                let more_btn = egui::Button::new(
                    RichText::new(format!(
                        "Carregar mais vídeos (+{step}, até {})",
                        search::MAX_CHANNEL_VIDEOS_LIMIT
                    ))
                    .size(13.0)
                    .strong()
                    .color(palette.header_button_fg),
                )
                .fill(palette.header_button_bg)
                .stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(0.0, 34.0))
                .corner_radius(17);
                if ui
                    .add(more_btn)
                    .on_hover_text("Buscar próximo lote de vídeos públicos do canal sob demanda")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    load_more_clicked = true;
                }
            }
        }

        if let Some(sort) = requested_sort {
            self.start_channel_sorted_videos(sort, ctx);
        }
        if cancel_more_clicked {
            self.cancel_channel_more_videos();
        } else if load_more_clicked {
            self.load_more_channel_videos(ctx);
        }

        chosen
    }

    pub(crate) fn channel_playlists_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        if let Some(selected_playlist) = self.channels.playlist_detail.selected.clone() {
            return self.channel_playlist_detail_ui(ui, ctx, layout, selected_playlist);
        }

        self.channel_playlists_list_ui(ui, ctx, layout)
    }

    pub(crate) fn channel_playlist_detail_header_ui(
        &self,
        ui: &mut egui::Ui,
        selected_playlist: &search::ChannelPlaylist,
        loading_videos: bool,
        palette: ThemePalette,
    ) -> (bool, bool) {
        let mut back_to_playlists = false;
        let mut reload_playlist = false;

        ui.horizontal_wrapped(|ui| {
            let back_btn = egui::Button::new(
                RichText::new("← Voltar às playlists")
                    .size(12.5)
                    .color(palette.header_button_fg),
            )
            .fill(palette.header_button_bg)
            .stroke(egui::Stroke::NONE)
            .corner_radius(16);
            if ui
                .add(back_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                back_to_playlists = true;
            }
            ui.label(
                RichText::new(&selected_playlist.title)
                    .size(16.0)
                    .strong()
                    .color(palette.channel_fg),
            );
            if let Some(count) = selected_playlist.item_count {
                ui.label(
                    RichText::new(format!("({count} vídeos)"))
                        .size(13.0)
                        .color(palette.neutral_status_fg),
                );
            }
            if ui
                .add_enabled(
                    !loading_videos,
                    egui::Button::new(RichText::new("↻ Atualizar").small()),
                )
                .clicked()
            {
                reload_playlist = true;
            }
        });
        ui.add_space(10.0);

        (back_to_playlists, reload_playlist)
    }

    pub(crate) fn channel_playlist_detail_loading_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> bool {
        let mut cancel_playlist_videos = false;

        egui::Frame::new()
            .fill(palette.info_box_bg)
            .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(card_max_w);
                ui.horizontal_wrapped(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Carregando vídeos da playlist pública...")
                            .color(palette.info_accent_fg),
                    );
                    if ui.button(RichText::new("Cancelar").small()).clicked() {
                        cancel_playlist_videos = true;
                    }
                });
            });
        ui.add_space(8.0);

        cancel_playlist_videos
    }

    pub(crate) fn channel_playlist_detail_error_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
        err: &str,
    ) -> bool {
        let mut reload_playlist = false;

        egui::Frame::new()
            .fill(palette.error_bg)
            .stroke(egui::Stroke::new(1.0, palette.error_stroke))
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(card_max_w);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Erro ao carregar vídeos da playlist: {err}"))
                            .color(palette.error_banner_fg),
                    );
                    if ui
                        .button(RichText::new("↻ Tentar novamente").small())
                        .clicked()
                    {
                        reload_playlist = true;
                    }
                });
            });
        ui.add_space(8.0);

        reload_playlist
    }

    pub(crate) fn channel_playlist_detail_empty_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_min_width(card_max_w);
                ui.label(
                    RichText::new("Nenhum vídeo público disponível nesta playlist.")
                        .color(palette.neutral_status_fg),
                );
            });
    }

    pub(crate) fn channel_playlist_detail_status_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> (bool, bool) {
        let loading_videos = self.is_channel_playlist_videos_loading();
        let videos_error = self.channels.playlist_detail.error.as_deref();
        let videos_loaded = self.channels.playlist_detail.loaded;
        let has_videos = !self.channels.playlist_detail.items.is_empty();
        let mut cancel_playlist_videos = false;
        let mut reload_playlist = false;

        if loading_videos && self.channel_playlist_detail_loading_ui(ui, card_max_w, palette) {
            cancel_playlist_videos = true;
        }

        if let Some(err) = videos_error
            && self.channel_playlist_detail_error_ui(ui, card_max_w, palette, err)
        {
            reload_playlist = true;
        }

        if videos_loaded && !has_videos && videos_error.is_none() {
            self.channel_playlist_detail_empty_ui(ui, card_max_w, palette);
        }

        (cancel_playlist_videos, reload_playlist)
    }

    pub(crate) fn channel_playlist_detail_videos_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let mut chosen = None;
        let current_url = self.selected.as_ref().map(|video| video.url.clone());
        let playlist_videos: Vec<Video> = self
            .channels
            .playlist_detail
            .items
            .iter()
            .map(|item| item.video.clone())
            .collect();

        let gap = 16.0;
        let cols = if layout.compact {
            1
        } else {
            ((layout.card_max_w + gap) / 280.0).floor().clamp(2.0, 4.0) as usize
        };
        let card_w =
            ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(180.0);

        for (row_idx, row) in playlist_videos.chunks(cols).enumerate() {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (col_idx, video) in row.iter().enumerate() {
                    let is_playing = current_url.as_deref() == Some(video.url.as_str());
                    let card_action = ui
                        .push_id(("chan_pl_vid", row_idx, col_idx, &video.url), |ui| {
                            self.channel_grid_video_card_ui(
                                ui, ctx, video, card_w, is_playing, &palette,
                            )
                        })
                        .inner;
                    self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                }
            });
            ui.add_space(18.0);
        }
        if playlist_videos.len() >= search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT {
            ui.label(
                RichText::new(format!(
                    "Exibindo até o limite de {} vídeos por playlist pública.",
                    search::MAX_CHANNEL_PLAYLIST_VIDEOS_LIMIT
                ))
                .small()
                .color(palette.neutral_status_fg),
            );
        }

        chosen
    }

    pub(crate) fn channel_playlist_detail_handle_actions(
        &mut self,
        ctx: &egui::Context,
        selected_playlist: search::ChannelPlaylist,
        back_to_playlists: bool,
        cancel_playlist_videos: bool,
        reload_playlist: bool,
    ) {
        if back_to_playlists {
            self.close_channel_public_playlist(ctx);
        } else if cancel_playlist_videos {
            self.cancel_channel_playlist_videos();
        } else if reload_playlist {
            self.open_channel_public_playlist(selected_playlist, ctx);
        }
    }

    pub(crate) fn channel_playlist_detail_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
        selected_playlist: search::ChannelPlaylist,
    ) -> Option<Video> {
        let loading_videos = self.is_channel_playlist_videos_loading();

        let (back_to_playlists, reload_header) = self.channel_playlist_detail_header_ui(
            ui,
            &selected_playlist,
            loading_videos,
            layout.palette,
        );

        let (cancel_playlist_videos, reload_status) =
            self.channel_playlist_detail_status_ui(ui, layout.card_max_w, layout.palette);

        let reload_playlist = reload_header || reload_status;

        let mut chosen = None;
        if !self.channels.playlist_detail.items.is_empty() {
            chosen = self.channel_playlist_detail_videos_ui(ui, ctx, layout);
        }

        self.channel_playlist_detail_handle_actions(
            ctx,
            selected_playlist,
            back_to_playlists,
            cancel_playlist_videos,
            reload_playlist,
        );

        chosen
    }

    pub(crate) fn channel_playlist_card_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        playlist: &search::ChannelPlaylist,
        width: f32,
        palette: ThemePalette,
    ) -> bool {
        let mut open_clicked = false;
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let thumb_h = (width * 9.0 / 16.0).round();

        ui.vertical(|ui| {
            ui.set_width(width);
            ui.set_max_width(width);
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
                    &playlist.thumbnail,
                    egui::vec2(width, (thumb_h - 9.0).max(40.0)),
                    12.0,
                );
                thumb_rect = resp.rect;
                thumb_clicked = resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked();
            });
            let count_label = playlist
                .item_count
                .map(|c| {
                    if c == 1 {
                        "1 vídeo".to_owned()
                    } else {
                        format!("{c} vídeos")
                    }
                })
                .unwrap_or_else(|| "Playlist".to_owned());
            paint_playlist_badge_overlay(ui, thumb_rect, &count_label);
            if thumb_clicked {
                open_clicked = true;
            }
            ui.add_space(8.0);
            let title_btn = egui::Button::new(
                RichText::new(truncate_text_chars(&playlist.title, 68))
                    .size(14.5)
                    .strong()
                    .color(palette.channel_fg),
            )
            .frame(false)
            .wrap();
            if ui
                .add(title_btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&playlist.title)
                .clicked()
            {
                open_clicked = true;
            }
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Ver playlist completa")
                            .size(12.5)
                            .color(palette.neutral_status_fg),
                    )
                    .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open_clicked = true;
            }
        });

        open_clicked
    }

    pub(crate) fn channel_playlists_list_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let mut reload_playlists = false;
        let mut cancel_playlists = false;
        let mut open_playlist = None;
        let loading_playlists = self.is_channel_playlists_loading();
        let playlists_error = self.channels.playlists.error.clone();
        let playlists_loaded = self.channels.playlists.loaded;
        let playlists = self.channels.playlists.items.clone();

        if loading_playlists {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Carregando playlists criadas...")
                                .color(palette.info_accent_fg),
                        );
                        if ui.button(RichText::new("Cancelar").small()).clicked() {
                            cancel_playlists = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if let Some(err) = &playlists_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar playlists públicas: {err}"))
                                .color(palette.error_banner_fg),
                        );
                        if ui
                            .button(RichText::new("↻ Tentar novamente").small())
                            .clicked()
                        {
                            reload_playlists = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if playlists_loaded && playlists.is_empty() && playlists_error.is_none() {
            Self::channel_playlists_empty_state_ui(ui, layout.card_max_w, palette);
        } else if !playlists.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Playlists criadas")
                        .size(16.0)
                        .strong()
                        .color(palette.channel_fg),
                );
            });
            ui.add_space(10.0);
            let gap = 16.0;
            let cols = if layout.compact {
                1
            } else {
                ((layout.card_max_w + gap) / 260.0).floor().clamp(2.0, 4.0) as usize
            };
            let card_w = ((layout.card_max_w - gap * (cols.saturating_sub(1) as f32))
                / cols as f32)
                .max(170.0);
            for (row_idx, row) in playlists.chunks(cols).enumerate() {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (col_idx, playlist) in row.iter().enumerate() {
                        if ui
                            .push_id(("chan_pl", row_idx, col_idx, &playlist.id), |ui| {
                                self.channel_playlist_card_ui(ui, ctx, playlist, card_w, palette)
                            })
                            .inner
                        {
                            open_playlist = Some(playlist.clone());
                        }
                    }
                });
                ui.add_space(18.0);
            }
        }

        if cancel_playlists {
            self.cancel_channel_playlists();
        } else if reload_playlists {
            self.start_channel_playlists(ctx);
        } else if let Some(pl) = open_playlist {
            self.open_channel_public_playlist(pl, ctx);
        }
        None
    }

    pub(crate) fn channel_search_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        layout: &ChannelTabLayout,
    ) -> Option<Video> {
        let palette = layout.palette;
        let loading = self.channels.is_channel_search_loading();
        let error = self.channels.search.error.clone();
        let loaded = self.channels.search.loaded;
        let query = self.channels.search.query.clone();
        let videos: Vec<Video> = self
            .channels
            .search
            .items
            .iter()
            .map(|item| item.video.clone())
            .collect();
        let mut chosen = None;
        let mut retry = false;
        let mut cancel = false;

        if loading {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(format!("Pesquisando \"{query}\" no canal..."))
                                .color(palette.info_accent_fg),
                        );
                        if ui.button(RichText::new("Cancelar").small()).clicked() {
                            cancel = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if let Some(err) = error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao pesquisar no canal: {err}"))
                                .color(palette.error_banner_fg),
                        );
                        if ui
                            .button(RichText::new("↻ Tentar novamente").small())
                            .clicked()
                        {
                            retry = true;
                        }
                    });
                });
            ui.add_space(10.0);
        }

        if loaded && videos.is_empty() && self.channels.search.error.is_none() {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_min_width(layout.card_max_w);
                    ui.label(
                        RichText::new(format!("Nenhum resultado encontrado para \"{query}\"."))
                            .color(palette.neutral_status_fg),
                    );
                });
        } else if !videos.is_empty() {
            for (index, video) in videos.iter().enumerate() {
                let card_action = ui
                    .push_id(("chan_search_row", index, &video.url), |ui| {
                        ui.set_max_width(layout.card_max_w);
                        self.youtube_search_result_card_ui(ui, ctx, video)
                    })
                    .inner;
                self.handle_channel_card_actions(ctx, &card_action, &mut chosen, video);
                ui.add_space(16.0);
            }
        }

        if cancel {
            self.channels.cancel_channel_search();
        } else if retry {
            self.start_channel_search(&query, ctx);
        }
        chosen
    }

    pub(crate) fn channel_about_status_ui(
        &self,
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) -> (bool, bool) {
        let loading_about = self.is_channel_about_loading();
        let about_error = self.channels.about.error.as_deref();
        let mut reload_about = false;
        let mut cancel_about = false;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Informações e metadados públicos do canal")
                    .small()
                    .color(palette.neutral_status_fg),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !loading_about,
                        egui::Button::new(RichText::new("↻ Atualizar informações").small()),
                    )
                    .clicked()
                {
                    reload_about = true;
                }
            });
        });
        ui.add_space(6.0);

        if loading_about {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Consultando aba Sobre do canal...")
                                .color(palette.info_accent_fg),
                        );
                        if ui.button(RichText::new("Cancelar").small()).clicked() {
                            cancel_about = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if let Some(err) = about_error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(card_max_w);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Erro ao consultar detalhes da aba Sobre: {err}"
                            ))
                            .color(palette.error_banner_fg),
                        );
                        if ui
                            .button(RichText::new("↻ Tentar novamente").small())
                            .clicked()
                        {
                            reload_about = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        (cancel_about, reload_about)
    }

    pub(crate) fn channel_about_card_ui(
        ui: &mut egui::Ui,
        page: &ChannelPage,
        about: Option<&search::ChannelAboutInfo>,
        layout: &ChannelTabLayout,
        dark_theme: bool,
    ) -> bool {
        let mut copy_link = false;
        let palette = layout.palette;
        let link_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        egui::Frame::new()
            .fill(palette.card_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_stroke))
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_min_width(layout.card_max_w);
                ui.set_max_width(layout.card_max_w);
                Self::channel_about_modal_body_ui(
                    ui,
                    page,
                    about,
                    &palette,
                    link_blue,
                    &mut copy_link,
                );
            });
        copy_link
    }

    pub(crate) fn channel_about_tab_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        page: &ChannelPage,
        layout: &ChannelTabLayout,
    ) {
        let dark_theme = self.settings.theme.value == Theme::Dark;
        let (cancel_about, reload_about) =
            self.channel_about_status_ui(ui, layout.card_max_w, layout.palette);
        let about = self.channels.about.info.clone();
        if Self::channel_about_card_ui(ui, page, about.as_ref(), layout, dark_theme) {
            self.message = match copy_text_to_clipboard(ctx, &page.canonical_url) {
                Ok(()) => format!("Link do canal copiado: {}", page.canonical_url),
                Err(error) => error,
            };
        }

        if cancel_about {
            self.cancel_channel_about();
        } else if reload_about {
            self.start_channel_about(ctx);
        }
    }

    pub(crate) fn channel_about_modal_body_ui(
        ui: &mut egui::Ui,
        page: &ChannelPage,
        about: Option<&search::ChannelAboutInfo>,
        palette: &ThemePalette,
        link_blue: Color32,
        copy_link: &mut bool,
    ) {
        let full_desc = about
            .and_then(|a| clean_description(a.description.as_deref()))
            .or_else(|| clean_description(page.description.as_deref()));

        ui.label(
            RichText::new("Descrição")
                .size(16.0)
                .strong()
                .color(palette.channel_fg),
        );
        ui.add_space(6.0);
        if let Some(desc) = full_desc {
            ui.label(RichText::new(desc).size(13.5).color(palette.description_fg));
        } else {
            ui.label(
                RichText::new("Nenhuma descrição pública informada.")
                    .size(13.0)
                    .color(palette.neutral_status_fg),
            );
        }

        let links = if !page.links.is_empty() {
            &page.links
        } else if let Some(info) = about {
            &info.links
        } else {
            &page.links
        };
        if !links.is_empty() {
            ui.add_space(16.0);
            ui.label(
                RichText::new("Links")
                    .size(16.0)
                    .strong()
                    .color(palette.channel_fg),
            );
            ui.add_space(8.0);
            for link in links {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    paint_link_chain_icon(ui, palette.neutral_status_fg);
                    ui.vertical(|ui| {
                        if !link.title.trim().is_empty() {
                            ui.label(
                                RichText::new(&link.title)
                                    .size(13.5)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(&link.url).size(12.5).color(link_blue),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            open_external_url(&link.url);
                        }
                    });
                });
                ui.add_space(6.0);
            }
        }

        ui.add_space(16.0);
        ui.label(
            RichText::new("Detalhes do canal")
                .size(16.0)
                .strong()
                .color(palette.channel_fg),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            paint_link_chain_icon(ui, palette.neutral_status_fg);
            ui.label(
                RichText::new(&page.canonical_url)
                    .size(13.0)
                    .color(palette.channel_fg),
            );
        });
        ui.add_space(6.0);

        if let Some(subs) = page
            .subscriber_count_text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                page.subscriber_count
                    .or_else(|| about.and_then(|a| a.subscriber_count))
                    .map(format_subscribers_pt)
            })
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(subs).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        if let Some(vids) = page
            .video_count_text
            .as_deref()
            .or_else(|| about.and_then(|a| a.video_count_text.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(vids).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        if let Some(views_text) = page
            .view_count_text
            .as_deref()
            .or_else(|| about.and_then(|a| a.view_count_text.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                page.view_count
                    .or_else(|| about.and_then(|a| a.view_count))
                    .map(|v| format!("{} visualizações", format_view_count(v)))
            })
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(
                    RichText::new(views_text)
                        .size(13.0)
                        .color(palette.channel_fg),
                );
            });
            ui.add_space(6.0);
        }

        if let Some(joined) = page
            .joined_date_text
            .as_deref()
            .or_else(|| about.and_then(|a| a.joined_date_text.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_info_circle_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(joined).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        if let Some(country) = page
            .country
            .as_deref()
            .or_else(|| about.and_then(|a| a.country.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                paint_globe_icon(ui, palette.neutral_status_fg);
                ui.label(RichText::new(country).size(13.0).color(palette.channel_fg));
            });
            ui.add_space(6.0);
        }

        let tags = if !page.tags.is_empty() {
            &page.tags
        } else if let Some(about_info) = about {
            &about_info.tags
        } else {
            &page.tags
        };
        if !tags.is_empty() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("Tags: {}", tags.join(", ")))
                    .size(12.0)
                    .color(palette.neutral_status_fg),
            );
        }

        ui.add_space(12.0);
        let share_btn = egui::Button::new(
            RichText::new("Compartilhar canal")
                .size(13.0)
                .strong()
                .color(palette.header_button_fg),
        )
        .fill(palette.header_button_bg)
        .stroke(egui::Stroke::NONE)
        .min_size(egui::vec2(0.0, 34.0))
        .corner_radius(17);
        if ui
            .add(share_btn)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            *copy_link = true;
        }
    }

    pub(crate) fn channel_about_modal_ui(&mut self, ctx: &egui::Context) {
        if !self.channels.about_modal_open || !self.channels.view_open {
            return;
        }
        let Some(page) = self.channels.page.clone() else {
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
            Color32::from_rgb(220, 220, 220)
        };
        let link_blue = if dark_theme {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let loading_about = self.is_channel_about_loading();
        let about = self.channels.about.info.clone();
        let mut close_modal = false;
        let mut copy_link = false;

        let title = channel_page_display_title(&page);
        let window_resp = egui::Window::new("channel_about_modal")
            .id(egui::Id::new("channel_about_modal_window"))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(520.0, 480.0))
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .fill(modal_bg)
                    .stroke(egui::Stroke::new(1.0, modal_stroke))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(20, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(title)
                            .size(19.0)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("✕")
                                        .size(16.0)
                                        .color(palette.neutral_status_fg),
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
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(8.0);
                if loading_about {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Carregando detalhes adicionais...")
                                .small()
                                .color(palette.info_accent_fg),
                        );
                    });
                    ui.add_space(8.0);
                }
                egui::ScrollArea::vertical()
                    .id_salt("channel_about_modal_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        Self::channel_about_modal_body_ui(
                            ui,
                            &page,
                            about.as_ref(),
                            &palette,
                            link_blue,
                            &mut copy_link,
                        );
                    });
            });

        if let Some(resp) = window_resp
            && ctx.input(|i| {
                i.pointer.primary_clicked()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !resp.response.rect.contains(p))
            })
        {
            close_modal = true;
        }
        if copy_link {
            self.message = match copy_text_to_clipboard(ctx, &page.canonical_url) {
                Ok(()) => format!("Link do canal copiado: {}", page.canonical_url),
                Err(error) => error,
            };
        }
        if close_modal {
            self.channels.about_modal_open = false;
        }
    }

    pub(crate) fn channel_load_status_ui(
        &self,
        ui: &mut egui::Ui,
        loading: bool,
        loading_target: Option<&ChannelTarget>,
        error: Option<&str>,
        retry_target: Option<&ChannelTarget>,
        palette: ThemePalette,
    ) -> (bool, Option<ChannelTarget>) {
        let mut cancel_initial = false;
        let mut reload_target = None;

        if loading {
            egui::Frame::new()
                .fill(palette.info_box_bg)
                .stroke(egui::Stroke::new(1.0, palette.info_box_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let target_text = loading_target
                            .map(channel_target_display_label)
                            .unwrap_or("canal");
                        ui.spinner();
                        ui.label(
                            RichText::new(format!("Carregando canal {target_text}..."))
                                .color(palette.info_accent_fg),
                        );
                        if ui
                            .button(RichText::new("Cancelar").small())
                            .on_hover_text("Cancelar o carregamento em andamento do canal")
                            .clicked()
                        {
                            cancel_initial = true;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if let Some(error_msg) = error {
            egui::Frame::new()
                .fill(palette.error_bg)
                .stroke(egui::Stroke::new(1.0, palette.error_stroke))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Erro ao carregar canal: {error_msg}"))
                                .color(palette.error_banner_fg),
                        );
                        if let Some(target) = retry_target
                            && ui
                                .button(RichText::new("↻ Tentar novamente").small())
                                .on_hover_text("Tentar buscar novamente os dados do canal")
                                .clicked()
                        {
                            reload_target = Some(target.clone());
                        }
                    });
                });
            ui.add_space(6.0);
        }

        (cancel_initial, reload_target)
    }

    pub(crate) fn channel_playlists_empty_state_ui(
        ui: &mut egui::Ui,
        card_max_w: f32,
        palette: ThemePalette,
    ) {
        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_min_width(card_max_w);
                ui.label(
                    RichText::new("Nenhuma playlist pública listada para este canal.")
                        .color(palette.neutral_status_fg),
                );
            });
    }
}
