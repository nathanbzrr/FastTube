use super::*;
use eframe::egui::RichText;

/// Returns true when the Watch Comments panel has public comments or an InnerTube continuation
/// available (working 100% OAuth-free), or when an OAuth session with comment scope is active.
pub(crate) fn can_view_watch_comments(
    extras: &watch_extras::WatchExtras,
    has_oauth_comment_scope: bool,
) -> bool {
    !extras.comments.is_empty()
        || extras.innertube_comments_continuation.is_some()
        || extras.innertube_comments_loading
        || has_oauth_comment_scope
}

/// Checks whether a comment author matches the signed-in user's channel title or handle.
pub(crate) fn is_own_comment(
    author: &str,
    my_channel_title: Option<&str>,
    my_channel: Option<&ChannelTarget>,
) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches('@').to_lowercase();
    let author_norm = norm(author);
    if author_norm.is_empty() {
        return false;
    }
    if let Some(title) = my_channel_title
        && !title.trim().is_empty()
        && norm(title) == author_norm
    {
        return true;
    }
    if let Some(ChannelTarget::Handle(handle)) = my_channel
        && !handle.trim().is_empty()
        && norm(handle) == author_norm
    {
        return true;
    }
    false
}

impl FastTube {
    pub(crate) fn paint_avatar_or_initials(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        avatar_url: Option<&str>,
        fallback_name: &str,
        diameter: f32,
        clickable_cursor: bool,
    ) -> egui::Response {
        Self::paint_avatar_with_cache(
            &mut self.avatars,
            ui,
            ctx,
            avatar_url,
            fallback_name,
            diameter,
            clickable_cursor,
        )
    }

    pub(crate) fn paint_avatar_with_cache(
        avatars: &mut thumbnails::AvatarCache,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        avatar_url: Option<&str>,
        fallback_name: &str,
        diameter: f32,
        clickable_cursor: bool,
    ) -> egui::Response {
        let sense = if clickable_cursor {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(diameter, diameter), sense);
        let painted_image = avatar_url
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
            .is_some_and(|uri| avatars.try_paint_circle(ui, ctx, rect, uri));
        if !painted_image {
            let label = if fallback_name.trim().is_empty() {
                "YT"
            } else {
                fallback_name
            };
            let [r, g, b] = recommendations::channel_avatar_rgb(label);
            let initials = recommendations::channel_avatar_initials(label);
            ui.painter()
                .circle_filled(rect.center(), diameter * 0.5, Color32::from_rgb(r, g, b));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initials,
                egui::FontId::proportional((diameter * 0.39).clamp(10.0, 16.0)),
                Color32::WHITE,
            );
        }
        if clickable_cursor {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    }

    pub(crate) fn insert_emoji_into_draft(
        ctx: &egui::Context,
        input_id: egui::Id,
        draft: &mut String,
        emoji: &str,
    ) {
        let char_pos: usize = egui::TextEdit::load_state(ctx, input_id)
            .and_then(|state| state.cursor.char_range())
            .map(|range| usize::from(range.primary.index))
            .unwrap_or_else(|| draft.chars().count());
        let byte_idx = draft
            .char_indices()
            .nth(char_pos)
            .map(|(idx, _)| idx)
            .unwrap_or(draft.len());
        draft.insert_str(byte_idx, emoji);
        let new_char_pos = char_pos + emoji.chars().count();
        if let Some(mut state) = egui::TextEdit::load_state(ctx, input_id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(new_char_pos),
                )));
            state.store(ctx, input_id);
        }
        ctx.memory_mut(|mem| mem.request_focus(input_id));
    }

    pub(crate) fn comment_emoji_picker_ui(
        ui: &mut egui::Ui,
        palette: &ThemePalette,
        id_salt: impl std::hash::Hash + std::fmt::Debug,
    ) -> Option<&'static str> {
        const EMOJIS: &[&str] = &[
            "\u{1f600}",
            "\u{1f602}",
            "\u{1f923}",
            "\u{1f60a}",
            "\u{1f60d}",
            "\u{1f970}",
            "\u{1f914}",
            "\u{1f62e}",
            "\u{1f622}",
            "\u{1f62d}",
            "\u{1f621}",
            "\u{1f525}",
            "\u{2764}\u{fe0f}",
            "\u{1f44d}",
            "\u{1f44e}",
            "\u{1f44f}",
            "\u{1f64c}",
            "\u{1f389}",
            "\u{1f4af}",
            "\u{2728}",
            "\u{1f440}",
            "\u{1f64f}",
            "\u{1f680}",
            "\u{26a1}",
        ];
        let mut chosen = None;
        ui.push_id(id_salt, |ui| {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                        for &emoji in EMOJIS {
                            let btn = egui::Button::new(RichText::new(emoji).size(16.0))
                                .fill(Color32::TRANSPARENT)
                                .frame(false)
                                .min_size(egui::vec2(28.0, 28.0));
                            if ui
                                .add(btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                chosen = Some(emoji);
                            }
                        }
                    });
                });
        });
        chosen
    }

    pub(crate) fn hash_comment_row_state(
        extras: &watch_extras::WatchExtras,
        comment_id: &str,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(state) = extras.comment_replies.get(comment_id) {
            state.reply_open.hash(&mut hasher);
            state.loaded.hash(&mut hasher);
            state.comments.len().hash(&mut hasher);
            state.is_loading().hash(&mut hasher);
            state.is_posting_reply().hash(&mut hasher);
            state.error.is_some().hash(&mut hasher);
            state.reply_post_error.is_some().hash(&mut hasher);
            state.next.is_some().hash(&mut hasher);
            if state.loaded {
                for reply in &state.comments {
                    reply.id.len().hash(&mut hasher);
                    if let Some(tr) = extras.comment_translations.get(&reply.id) {
                        tr.showing_translated.hash(&mut hasher);
                        tr.loading.hash(&mut hasher);
                        tr.error.is_some().hash(&mut hasher);
                        tr.translated_lines.len().hash(&mut hasher);
                    }
                    extras
                        .comment_rating_errors
                        .contains_key(&reply.id)
                        .hash(&mut hasher);
                    extras.is_rating_comment(&reply.id).hash(&mut hasher);
                }
            }
        } else {
            0u8.hash(&mut hasher);
        }
        (extras.reply_emoji_picker_for.as_deref() == Some(comment_id)).hash(&mut hasher);
        if let Some(tr) = extras.comment_translations.get(comment_id) {
            tr.showing_translated.hash(&mut hasher);
            tr.loading.hash(&mut hasher);
            tr.error.is_some().hash(&mut hasher);
            tr.translated_lines.len().hash(&mut hasher);
        }
        if let Some(it_meta) = extras.innertube_comment_meta.get(comment_id) {
            it_meta.is_pinned.hash(&mut hasher);
            it_meta.is_hearted.hash(&mut hasher);
            it_meta.is_owner.hash(&mut hasher);
            it_meta.is_member.hash(&mut hasher);
        } else {
            0u8.hash(&mut hasher);
        }
        extras
            .comment_rating_errors
            .contains_key(comment_id)
            .hash(&mut hasher);
        extras.is_rating_comment(comment_id).hash(&mut hasher);
        let is_editing = extras.editing_comment_id.as_deref() == Some(comment_id);
        is_editing.hash(&mut hasher);
        if is_editing {
            extras.editing_comment_draft.len().hash(&mut hasher);
        }
        hasher.finish()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn comment_item_body_and_actions_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        comment: &oauth::Comment,
        cached_lines: &[watch_extras::CachedTimestampLine],
        parent_comment_id: Option<&str>,
        text_size: f32,
        can_reply: bool,
    ) {
        let pt = self.ui_pt();
        let palette = self.palette();
        let link_color = if self.is_dark_theme() {
            Color32::from_rgb(62, 166, 255)
        } else {
            Color32::from_rgb(6, 95, 212)
        };
        let is_editing =
            self.watch.extras.editing_comment_id.as_deref() == Some(comment.id.as_str());
        if is_editing {
            let mut save_edit = false;
            let mut cancel_edit = false;
            ui.add(
                egui::TextEdit::multiline(&mut self.watch.extras.editing_comment_draft)
                    .id_salt(("edit_comment_input", &comment.id))
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let can_save = !self.watch.extras.editing_comment_draft.trim().is_empty();
                if ui
                    .add_enabled_ui(can_save, |ui| {
                        styled_compact_button(
                            ui,
                            if pt { "Salvar" } else { "Save" },
                            true,
                            &palette,
                        )
                    })
                    .inner
                    .clicked()
                {
                    save_edit = true;
                }
                if styled_ghost_button(ui, if pt { "Cancelar" } else { "Cancel" }, &palette)
                    .clicked()
                {
                    cancel_edit = true;
                }
            });
            if save_edit {
                let updated = self.watch.extras.editing_comment_draft.trim().to_owned();
                self.watch.extras.editing_comment_id = None;
                self.watch.extras.editing_comment_draft.clear();
                self.start_edit_comment(comment.id.clone(), updated, ctx);
            } else if cancel_edit {
                self.watch.extras.editing_comment_id = None;
                self.watch.extras.editing_comment_draft.clear();
            }
            return;
        }

        let (showing_translated, translation_loading, translation_error, clicked_secs) = {
            let translation = self.watch.extras.comment_translations.get(&comment.id);
            let showing_translated = translation.is_some_and(|t| t.showing_translated);
            let translation_loading = translation.is_some_and(|t| t.loading);
            let translation_error = translation.and_then(|t| t.error.clone());
            let lines_to_render = translation
                .filter(|t| t.showing_translated && !t.translated_lines.is_empty())
                .map(|t| t.translated_lines.as_slice())
                .unwrap_or(cached_lines);
            let clicked_secs = Self::render_timestamp_lines_ui(
                ui,
                lines_to_render,
                palette.channel_fg,
                text_size,
                link_color,
            );
            (
                showing_translated,
                translation_loading,
                translation_error,
                clicked_secs,
            )
        };
        if let Some(secs) = clicked_secs {
            self.seek_to_seconds(secs);
        }

        let translate_label = if translation_loading {
            if pt {
                "Traduzindo..."
            } else {
                "Translating..."
            }
        } else if showing_translated {
            if pt { "Ver original" } else { "See original" }
        } else if pt {
            "Traduzir para o portugu\u{00ea}s"
        } else {
            "Translate"
        };
        let translate_btn = egui::Button::new(RichText::new(translate_label).size(12.0).color(
            if showing_translated {
                palette.info_accent_fg
            } else {
                palette.neutral_status_fg
            },
        ))
        .fill(Color32::TRANSPARENT)
        .frame(false);
        if ui
            .add_enabled(!translation_loading, translate_btn)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.watch
                .extras
                .toggle_comment_translation(&comment.id, &comment.text, ctx);
        }
        if let Some(error) = translation_error {
            ui.label(RichText::new(error).size(11.5).color(palette.warning_fg));
        }

        let can_rate_comment = self.auth.authenticated || self.auth.youtube_web_session.is_some();
        let own_comment = self.auth.authenticated
            && is_own_comment(
                &comment.author,
                self.auth.my_channel_title.as_deref(),
                self.auth.my_channel.as_ref(),
            );
        let rating_busy_any = self.watch.extras.is_rating_any_comment();
        let rating_busy_here = self.watch.extras.is_rating_comment(&comment.id);
        let rating_error = self
            .watch
            .extras
            .comment_rating_errors
            .get(&comment.id)
            .cloned();

        let mut start_edit = false;
        let mut trigger_delete = false;
        let mut open_report = false;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let rating = self.watch.extras.comment_ratings.get(&comment.id).copied();
            let liked = rating == Some(watch_extras::CommentRating::Like);
            let disliked = rating == Some(watch_extras::CommentRating::Dislike);
            let displayed_likes = self
                .watch
                .extras
                .comment_displayed_likes(&comment.id, comment.likes);
            let like_color = if liked {
                palette.info_accent_fg
            } else {
                palette.neutral_status_fg
            };
            let like_icon = Icon::ThumbsUp.image(like_color, 15.0);
            let like_btn = if displayed_likes > 0 {
                let compact = recommendations::format_youtube_compact_views(displayed_likes)
                    .trim_end_matches(" visualiza\u{00e7}\u{00f5}es")
                    .trim_end_matches(" visualiza\u{00e7}\u{00e3}o")
                    .to_owned();
                egui::Button::image_and_text(
                    like_icon,
                    RichText::new(compact).size(12.5).color(like_color),
                )
            } else {
                egui::Button::image(like_icon)
            }
            .fill(Color32::TRANSPARENT)
            .frame(false);
            let like_hover = if can_rate_comment {
                if pt { "Gostei" } else { "Like" }
            } else if pt {
                "Entre para avaliar coment\u{00e1}rios"
            } else {
                "Sign in to rate comments"
            };
            let like_resp = ui
                .add_enabled(can_rate_comment && !rating_busy_any, like_btn)
                .on_hover_text(like_hover)
                .on_disabled_hover_text(like_hover);
            if can_rate_comment
                && like_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
            {
                self.watch.extras.start_comment_rating(
                    &comment.id,
                    parent_comment_id,
                    watch_extras::CommentRating::Like,
                    self.auth.youtube_web_session.as_ref(),
                    ctx,
                );
            }

            let dislike_color = if disliked {
                palette.info_accent_fg
            } else {
                palette.neutral_status_fg
            };
            let dislike_btn = egui::Button::image(Icon::ThumbsDown.image(dislike_color, 15.0))
                .fill(Color32::TRANSPARENT)
                .frame(false);
            let dislike_hover = if can_rate_comment {
                if pt { "N\u{00e3}o gostei" } else { "Dislike" }
            } else if pt {
                "Entre para avaliar coment\u{00e1}rios"
            } else {
                "Sign in to rate comments"
            };
            let dislike_resp = ui
                .add_enabled(can_rate_comment && !rating_busy_any, dislike_btn)
                .on_hover_text(dislike_hover)
                .on_disabled_hover_text(dislike_hover);
            if can_rate_comment
                && dislike_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
            {
                self.watch.extras.start_comment_rating(
                    &comment.id,
                    parent_comment_id,
                    watch_extras::CommentRating::Dislike,
                    self.auth.youtube_web_session.as_ref(),
                    ctx,
                );
            }

            if rating_busy_here {
                let _ =
                    skeleton::skeleton_box(ui, egui::vec2(26.0, 14.0), 7.0, self.is_dark_theme());
            }

            if can_reply {
                let reply_btn = egui::Button::new(
                    RichText::new(if pt { "Responder" } else { "Reply" })
                        .size(12.0)
                        .strong()
                        .color(palette.header_button_fg),
                )
                .fill(Color32::TRANSPARENT)
                .frame(false);
                if ui
                    .add(reply_btn)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    let entry = self
                        .watch
                        .extras
                        .comment_replies
                        .entry(comment.id.clone())
                        .or_default();
                    entry.reply_open = !entry.reply_open;
                    entry.reply_post_error = None;
                    if !entry.reply_open
                        && self.watch.extras.reply_emoji_picker_for.as_deref()
                            == Some(comment.id.as_str())
                    {
                        self.watch.extras.reply_emoji_picker_for = None;
                    }
                }
            }

            let more_resp = ui
                .add(
                    egui::Button::new(
                        RichText::new("\u{22ef}")
                            .size(14.0)
                            .color(palette.neutral_status_fg),
                    )
                    .fill(Color32::TRANSPARENT)
                    .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if pt {
                    "Mais a\u{00e7}\u{00f5}es"
                } else {
                    "More actions"
                });
            egui::Popup::menu(&more_resp).show(|ui| {
                ui.set_min_width(150.0);
                if own_comment {
                    if styled_menu_item_button(
                        ui,
                        if pt { "Editar" } else { "Edit" },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        start_edit = true;
                        ui.close();
                    }
                    if styled_menu_item_button(
                        ui,
                        if pt { "Excluir" } else { "Delete" },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        trigger_delete = true;
                        ui.close();
                    }
                    ui.separator();
                }
                if styled_menu_item_button(
                    ui,
                    if pt { "Denunciar" } else { "Report" },
                    false,
                    &palette,
                )
                .clicked()
                {
                    open_report = true;
                    ui.close();
                }
            });
        });

        if start_edit {
            self.watch.extras.editing_comment_id = Some(comment.id.clone());
            self.watch.extras.editing_comment_draft = comment.text.clone();
        } else if trigger_delete {
            self.start_delete_comment(comment.id.clone(), ctx);
        } else if open_report {
            self.watch.extras.report_reason.clear();
            self.watch.extras.report_details = format!("@{} ({})", comment.author, comment.id);
            self.watch.extras.report_modal_open = true;
        }
        if let Some(error) = rating_error {
            ui.label(RichText::new(error).size(11.5).color(palette.warning_fg));
        }
    }

    pub(crate) fn comments_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, scroll_id: &str) {
        let palette = self.palette();
        let pt = self.ui_pt();
        if self.auth.authenticated
            && self.auth.my_channel_avatar_url.is_none()
            && !self.auth.my_channel_task.is_running()
            && !self.auth.my_channel_fetch_attempted
        {
            self.start_fetch_my_channel(false, ctx);
        }
        if !cfg!(test)
            && self.watch.extras.comments_loaded
            && !self.watch.extras.comments.is_empty()
            && !self.watch.extras.comment_state_sync_started
            && (self.auth.youtube_web_session.is_some()
                || (!self.auth.youtube_web_session_checked && self.auth.cached_has_saved_profile))
        {
            self.watch
                .extras
                .start_comment_state_sync(self.auth.youtube_web_session.as_ref(), ctx);
        }

        let needs_reauth = self
            .auth
            .session
            .as_ref()
            .is_some_and(|s| !s.has_comment_scope());
        let can_write_comments = self
            .auth
            .session
            .as_ref()
            .is_some_and(oauth::TokenSession::has_comment_scope);
        let can_view_comments = can_view_watch_comments(&self.watch.extras, can_write_comments);
        let any_comments_loading = self.watch.extras.comments_loading.is_some()
            || self.watch.extras.innertube_comments_loading
            || (scroll_id == "shorts_comments"
                && !self.watch.extras.comments_loaded
                && self.watch.next_task.is_running());

        let displayed_total = self
            .watch
            .extras
            .comments_total
            .unwrap_or(self.watch.extras.comments.len() as u64)
            .max(self.watch.extras.comments.len() as u64);
        let count_text = if displayed_total == 1 {
            if pt {
                "1 coment\u{00e1}rio".to_owned()
            } else {
                "1 comment".to_owned()
            }
        } else if pt {
            format!("{displayed_total} coment\u{00e1}rios")
        } else {
            format!("{displayed_total} comments")
        };

        let mut new_order = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(24.0, 8.0);
            ui.label(
                RichText::new(count_text)
                    .size(20.0)
                    .strong()
                    .color(palette.header_button_fg),
            );

            let can_sort_comments = can_write_comments
                || self
                    .watch
                    .extras
                    .innertube_comments_top_continuation
                    .is_some()
                || self
                    .watch
                    .extras
                    .innertube_comments_newest_continuation
                    .is_some();
            if styled_compact_button_enabled(
                ui,
                !any_comments_loading && can_sort_comments,
                if pt {
                    "\u{2261}  Ordenar por"
                } else {
                    "\u{2261}  Sort by"
                },
                self.watch.extras.comments_order_menu_open,
                &palette,
            )
            .on_hover_text(if pt {
                "Escolher ordena\u{00e7}\u{00e3}o dos coment\u{00e1}rios"
            } else {
                "Choose comment sort order"
            })
            .clicked()
            {
                self.watch.extras.comments_order_menu_open =
                    !self.watch.extras.comments_order_menu_open;
            }

            if !self.watch.extras.comments_loaded && !any_comments_loading && can_view_comments {
                let load_label = if pt {
                    "Carregar coment\u{00e1}rios"
                } else {
                    "Load comments"
                };
                if styled_compact_button(ui, load_label, false, &palette).clicked() {
                    if let Some(cont) = self.watch.extras.innertube_comments_continuation.clone() {
                        self.start_load_innertube_comments(cont, true, ctx);
                    } else {
                        self.watch
                            .extras
                            .start_comments(self.auth.session.as_ref(), false, ctx);
                    }
                }
            } else if self.watch.extras.comments_loaded
                && styled_compact_button_enabled(
                    ui,
                    !any_comments_loading,
                    "\u{21bb}",
                    false,
                    &palette,
                )
                .on_hover_text(if pt {
                    "Atualizar coment\u{00e1}rios"
                } else {
                    "Refresh comments"
                })
                .clicked()
            {
                let sort_cont = match self.watch.extras.comments_order {
                    oauth::CommentOrder::Relevance => self
                        .watch
                        .extras
                        .innertube_comments_top_continuation
                        .clone(),
                    oauth::CommentOrder::Time => self
                        .watch
                        .extras
                        .innertube_comments_newest_continuation
                        .clone(),
                };
                if let Some(cont) = sort_cont
                    .or_else(|| self.watch.extras.innertube_comments_continuation.clone())
                    .filter(|_| !can_write_comments)
                {
                    self.start_load_innertube_comments(cont, true, ctx);
                } else if can_write_comments {
                    self.watch
                        .extras
                        .start_comments(self.auth.session.as_ref(), false, ctx);
                } else if let Some(cont) = self
                    .watch
                    .extras
                    .innertube_comments_top_continuation
                    .clone()
                    .or_else(|| self.watch.extras.innertube_comments_continuation.clone())
                {
                    self.start_load_innertube_comments(cont, true, ctx);
                }
            }
        });

        if self.watch.extras.comments_order_menu_open {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(palette.card_bg)
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(6, 6))
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .show(ui, |ui| {
                    ui.set_max_width(340.0);
                    for (title, subtitle, order) in [
                        (
                            if pt { "Principais" } else { "Top comments" },
                            if pt {
                                "Mostrar coment\u{00e1}rios em destaque"
                            } else {
                                "Show top comments first"
                            },
                            oauth::CommentOrder::Relevance,
                        ),
                        (
                            if pt { "Mais recentes" } else { "Newest first" },
                            if pt {
                                "Mostrar coment\u{00e1}rios recentes, incluindo spam em potencial"
                            } else {
                                "Show newest comments first"
                            },
                            oauth::CommentOrder::Time,
                        ),
                    ] {
                        let selected = self.watch.extras.comments_order == order;
                        let item_fill = if selected {
                            palette.header_button_bg
                        } else {
                            Color32::TRANSPARENT
                        };
                        let resp = egui::Frame::new()
                            .fill(item_fill)
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(12, 9))
                            .show(ui, |ui| {
                                ui.set_min_width(308.0);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.label(
                                        RichText::new(title)
                                            .size(14.0)
                                            .strong()
                                            .color(palette.header_button_fg),
                                    );
                                    ui.label(
                                        RichText::new(subtitle)
                                            .size(12.0)
                                            .color(palette.neutral_status_fg),
                                    );
                                });
                            })
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            new_order = Some(order);
                            self.watch.extras.comments_order_menu_open = false;
                        }
                    }
                });
        }

        if let Some(order) = new_order
            && self.watch.extras.comments_order != order
        {
            let sort_token = match order {
                oauth::CommentOrder::Relevance => self
                    .watch
                    .extras
                    .innertube_comments_top_continuation
                    .clone(),
                oauth::CommentOrder::Time => self
                    .watch
                    .extras
                    .innertube_comments_newest_continuation
                    .clone(),
            };
            if let Some(token) = sort_token.filter(|_| !can_write_comments) {
                self.watch.extras.comments_order = order;
                self.start_load_innertube_comments(token, true, ctx);
            } else if can_write_comments {
                self.watch
                    .extras
                    .set_comments_order(order, self.auth.session.as_ref(), ctx);
            } else if let Some(token) = match order {
                oauth::CommentOrder::Relevance => self
                    .watch
                    .extras
                    .innertube_comments_top_continuation
                    .clone(),
                oauth::CommentOrder::Time => self
                    .watch
                    .extras
                    .innertube_comments_newest_continuation
                    .clone(),
            } {
                self.watch.extras.comments_order = order;
                self.start_load_innertube_comments(token, true, ctx);
            }
        }

        ui.add_space(12.0);

        if needs_reauth {
            if self.auth.reauth_task.is_running() {
                ui.horizontal(|ui| {
                    skeleton::skeleton_circle(ui, 16.0, self.is_dark_theme());
                    ui.label(if pt {
                        "Aguardando autoriza\u{00e7}\u{00e3}o no navegador..."
                    } else {
                        "Waiting for browser authorization..."
                    });
                    if styled_ghost_button(ui, if pt { "Cancelar" } else { "Cancel" }, &palette)
                        .clicked()
                    {
                        self.cancel_reauth();
                    }
                });
            } else if styled_pill_button(
                ui,
                if pt {
                    "Autorizar coment\u{00e1}rios"
                } else {
                    "Authorize comments"
                },
                false,
                &palette,
            )
            .clicked()
            {
                self.start_reauth(ctx);
            }
        } else if can_write_comments {
            let mut post_comment = false;
            let user_avatar_url = self.auth.my_channel_avatar_url.clone();
            let user_name = self
                .auth
                .my_channel_title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| match &self.auth.my_channel {
                    Some(ChannelTarget::Handle(h)) => h.trim_start_matches('@').to_owned(),
                    Some(ChannelTarget::ChannelId(id)) => id.clone(),
                    None => "Voc\u{00ea}".to_owned(),
                });
            let composer_id = ui.make_persistent_id((scroll_id, "comment_composer_input"));
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let composer_avatar_resp = self.paint_avatar_or_initials(
                    ui,
                    ctx,
                    user_avatar_url.as_deref(),
                    &user_name,
                    40.0,
                    false,
                );
                Self::paint_comment_avatar_ring(ui, composer_avatar_resp.rect);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let has_draft = !self.watch.extras.comment_draft.trim().is_empty();
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut self.watch.extras.comment_draft)
                            .id(composer_id)
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .frame(egui::Frame::NONE)
                            .hint_text(
                                RichText::new("Adicione um coment\u{00e1}rio...")
                                    .size(14.0)
                                    .color(palette.neutral_status_fg),
                            ),
                    );
                    if resp.gained_focus() || resp.clicked() || has_draft {
                        self.watch.extras.comment_composer_active = true;
                    }
                    let line_y = resp.rect.bottom() + 4.0;
                    let line_stroke = if resp.has_focus() {
                        egui::Stroke::new(2.0, palette.header_button_fg)
                    } else {
                        egui::Stroke::new(1.0, palette.neutral_status_fg.gamma_multiply(0.5))
                    };
                    ui.painter().line_segment(
                        [
                            egui::pos2(resp.rect.left(), line_y),
                            egui::pos2(ui.max_rect().right(), line_y),
                        ],
                        line_stroke,
                    );
                    ui.add_space(10.0);

                    let sending = self.watch.extras.is_posting_comment();
                    let ready = !sending && has_draft;
                    let ctrl_enter = resp.has_focus()
                        && ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
                    if ctrl_enter && ready {
                        post_comment = true;
                    }

                    if self.watch.extras.comment_composer_active
                        || has_draft
                        || self.watch.extras.comment_emoji_picker_open
                    {
                        ui.horizontal(|ui| {
                            let emoji_btn =
                                egui::Button::image(Icon::Smile.image(palette.icon_fg, 19.0))
                                    .fill(if self.watch.extras.comment_emoji_picker_open {
                                        palette.card_alt_bg
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .frame(false)
                                    .corner_radius(16)
                                    .min_size(egui::vec2(32.0, 32.0));
                            if ui
                                .add(emoji_btn)
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("Inserir emoji")
                                .clicked()
                            {
                                self.watch.extras.comment_emoji_picker_open =
                                    !self.watch.extras.comment_emoji_picker_open;
                                if self.watch.extras.comment_emoji_picker_open {
                                    self.watch.extras.reply_emoji_picker_for = None;
                                }
                            }

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let comentar_btn = egui::Button::new(
                                        RichText::new("Comentar").size(13.0).strong().color(
                                            if ready {
                                                palette.active_button_fg
                                            } else {
                                                palette.neutral_status_fg
                                            },
                                        ),
                                    )
                                    .fill(if ready {
                                        palette.active_button_bg
                                    } else {
                                        palette.header_button_bg
                                    })
                                    .corner_radius(18)
                                    .min_size(egui::vec2(92.0, 34.0));
                                    if ui
                                        .add_enabled(ready, comentar_btn)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        post_comment = true;
                                    }

                                    let cancel_btn = egui::Button::new(
                                        RichText::new("Cancelar")
                                            .size(13.0)
                                            .strong()
                                            .color(palette.header_button_fg),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false)
                                    .corner_radius(18)
                                    .min_size(egui::vec2(74.0, 34.0));
                                    if ui
                                        .add(cancel_btn)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.watch.extras.comment_draft.clear();
                                        self.watch.extras.comment_composer_active = false;
                                        self.watch.extras.comment_emoji_picker_open = false;
                                        self.watch.extras.comment_post_error = None;
                                        resp.surrender_focus();
                                    }
                                },
                            );
                        });

                        if self.watch.extras.comment_emoji_picker_open {
                            ui.add_space(8.0);
                            if let Some(emoji) = Self::comment_emoji_picker_ui(
                                ui,
                                &palette,
                                (scroll_id, "comment_emoji_picker"),
                            ) {
                                Self::insert_emoji_into_draft(
                                    ctx,
                                    composer_id,
                                    &mut self.watch.extras.comment_draft,
                                    emoji,
                                );
                                self.watch.extras.comment_composer_active = true;
                            }
                        }
                    }
                });
            });
            if post_comment {
                self.watch
                    .extras
                    .start_post_comment(self.auth.session.as_ref(), ctx);
            }
            if let Some(error) = &self.watch.extras.comment_post_error {
                ui.add_space(4.0);
                ui.colored_label(palette.warning_fg, error);
            }
        } else if !self.auth.authenticated {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let avatar_resp = self.paint_avatar_or_initials(ui, ctx, None, "FT", 40.0, false);
                Self::paint_comment_avatar_ring(ui, avatar_resp.rect);
                ui.vertical(|ui| {
                    let row = ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if pt {
                                "Adicione um coment\u{00e1}rio..."
                            } else {
                                "Add a comment..."
                            })
                            .size(13.5)
                            .color(palette.neutral_status_fg),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if styled_pill_button(
                                ui,
                                if pt { "Entrar" } else { "Sign in" },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                self.start_reauth(ctx);
                            }
                        });
                    });
                    let line_y = row.response.rect.bottom() + 2.0;
                    ui.painter().line_segment(
                        [
                            egui::pos2(row.response.rect.left(), line_y),
                            egui::pos2(row.response.rect.right(), line_y),
                        ],
                        egui::Stroke::new(1.0, palette.neutral_status_fg.gamma_multiply(0.5)),
                    );
                });
            });
        }

        ui.add_space(10.0);
        Self::paint_hairline_divider(ui, &palette);
        ui.add_space(10.0);

        if !self.watch.extras.comments.is_empty() {
            let total_loaded = self.watch.extras.comments.len();
            let has_search = !self.watch.extras.comment_search_query.trim().is_empty();
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(16)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let suffix_w = if has_search { 88.0 } else { 0.0 };
                        let input_w = (ui.available_width() - suffix_w).max(120.0);
                        ui.add_sized(
                            [input_w, 20.0],
                            egui::TextEdit::singleline(&mut self.watch.extras.comment_search_query)
                                .id_salt((scroll_id, "comment_local_search"))
                                .frame(egui::Frame::NONE)
                                .hint_text(if pt {
                                    "Pesquisar nos coment\u{00e1}rios..."
                                } else {
                                    "Search comments..."
                                }),
                        );
                        if has_search {
                            let matched_len = watch_extras::filter_comments_by_query(
                                &self.watch.extras.comments,
                                &self.watch.extras.comment_search_query,
                            )
                            .len();
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("\u{2715}")
                                                    .size(12.0)
                                                    .color(palette.neutral_status_fg),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        self.watch.extras.comment_search_query.clear();
                                    }
                                    ui.label(
                                        RichText::new(if pt {
                                            format!("{matched_len} de {total_loaded}")
                                        } else {
                                            format!("{matched_len} of {total_loaded}")
                                        })
                                        .size(11.5)
                                        .color(palette.neutral_status_fg),
                                    );
                                },
                            );
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if let Some(error) = &self.watch.extras.comments_error {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.colored_label(palette.warning_fg, error);
                });
        }
        if self.watch.extras.comments_loaded
            && self.watch.extras.comments.is_empty()
            && self.watch.extras.comments_error.is_none()
            && !any_comments_loading
        {
            egui::Frame::new()
                .fill(palette.card_alt_bg)
                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(
                        RichText::new(if pt {
                            "Nenhum coment\u{00e1}rio dispon\u{00ed}vel."
                        } else {
                            "No comments available."
                        })
                        .color(palette.neutral_status_fg),
                    );
                });
        }

        let render_comments_body = |this: &mut Self, ui: &mut egui::Ui| {
            let comments_video_url = this.watch.extras.video_url.clone();
            let initial_comments_len = this.watch.extras.comments.len();
            watch_extras::sync_cached_comment_meta(
                &this.watch.extras.comments,
                &mut this.watch.extras.comment_render_cache,
            );
            let comments = std::mem::take(&mut this.watch.extras.comments);
            let mut comment_metas = std::mem::take(&mut this.watch.extras.comment_render_cache);
            let filtered_indices = watch_extras::filter_comments_by_query(
                &comments,
                &this.watch.extras.comment_search_query,
            );
            let width_key = ui.available_width().round() as i32;
            let alloc_width = ui.available_width().max(80.0);
            let visible_rect = ui.clip_rect().expand2(egui::vec2(0.0, 350.0));
            if !comments.is_empty()
                && !this.watch.extras.comment_search_query.trim().is_empty()
                && filtered_indices.is_empty()
            {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(if pt {
                        "Nenhum coment\u{00e1}rio corresponde \u{00e0} pesquisa."
                    } else {
                        "No comments match your search."
                    })
                    .size(13.0)
                    .color(palette.neutral_status_fg),
                );
            }
            for idx in filtered_indices {
                let comment = &comments[idx];
                let meta = &mut comment_metas[idx];
                let row_hash = Self::hash_comment_row_state(&this.watch.extras, &comment.id);
                ui.add_space(10.0);
                let cursor_top = ui.cursor().top();
                let offscreen_height = meta.measured_height.and_then(|(w_key, s_hash, height)| {
                    (w_key == width_key
                        && s_hash == row_hash
                        && height > 0.0
                        && (cursor_top + height < visible_rect.top()
                            || cursor_top > visible_rect.bottom()))
                    .then_some(height)
                });
                if let Some(height) = offscreen_height {
                    ui.allocate_exact_size(egui::vec2(alloc_width, height), egui::Sense::hover());
                } else {
                    let row_resp = ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let avatar_resp = this.paint_avatar_or_initials(
                            ui,
                            ctx,
                            comment.author_avatar.as_deref(),
                            &meta.avatar_name,
                            36.0,
                            false,
                        );
                        Self::paint_comment_avatar_ring(ui, avatar_resp.rect);

                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.set_max_width(ui.available_width().max(80.0));
                            let it_meta = this
                                .watch
                                .extras
                                .innertube_comment_meta
                                .get(&comment.id)
                                .cloned();
                            if it_meta.as_ref().is_some_and(|m| m.is_pinned) {
                                ui.label(
                                    RichText::new(if pt { "📌 Fixado" } else { "📌 Pinned" })
                                        .size(11.5)
                                        .strong()
                                        .color(palette.neutral_status_fg),
                                );
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(
                                    RichText::new(&meta.handle)
                                        .size(13.0)
                                        .strong()
                                        .color(palette.header_button_fg),
                                );
                                if let Some(ref badge_meta) = it_meta {
                                    if badge_meta.is_owner {
                                        egui::Frame::new()
                                            .fill(palette.active_button_bg)
                                            .corner_radius(8)
                                            .inner_margin(egui::Margin::symmetric(6, 1))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(if pt {
                                                        "Propriet\u{00e1}rio"
                                                    } else {
                                                        "Owner"
                                                    })
                                                    .size(10.5)
                                                    .strong()
                                                    .color(palette.active_button_fg),
                                                );
                                            });
                                    }
                                    if badge_meta.is_member {
                                        egui::Frame::new()
                                            .fill(Color32::from_rgb(43, 166, 64))
                                            .corner_radius(8)
                                            .inner_margin(egui::Margin::symmetric(6, 1))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(if pt {
                                                        "Membro"
                                                    } else {
                                                        "Member"
                                                    })
                                                    .size(10.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                                );
                                            });
                                    }
                                    if badge_meta.is_hearted {
                                        ui.label(
                                            RichText::new("\u{2764}")
                                                .size(12.0)
                                                .color(Color32::from_rgb(255, 0, 51)),
                                        );
                                    }
                                }
                                if !meta.relative_time.is_empty() {
                                    ui.label(
                                        RichText::new(&meta.relative_time)
                                            .size(12.0)
                                            .color(palette.neutral_status_fg),
                                    );
                                }
                            });

                            this.comment_item_body_and_actions_ui(
                                ui,
                                ctx,
                                comment,
                                &meta.lines,
                                None,
                                14.0,
                                can_write_comments,
                            );

                            if this
                                .watch
                                .extras
                                .comment_replies
                                .get(&comment.id)
                                .is_some_and(|state| state.reply_open)
                            {
                                let mut post_reply = false;
                                let mut cancel_reply = false;
                                let mut toggle_reply_emoji = false;
                                let mut picked_reply_emoji: Option<&'static str> = None;
                                let reply_input_id =
                                    ui.make_persistent_id(("reply_composer_input", &comment.id));
                                let reply_emoji_open =
                                    this.watch.extras.reply_emoji_picker_for.as_deref()
                                        == Some(comment.id.as_str());
                                let user_avatar_url = this.auth.my_channel_avatar_url.clone();
                                let user_name = this
                                    .auth
                                    .my_channel_title
                                    .as_deref()
                                    .filter(|s| !s.trim().is_empty())
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| "Voc\u{00ea}".to_owned());

                                ui.add_space(4.0);
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    let _ = this.paint_avatar_or_initials(
                                        ui,
                                        ctx,
                                        user_avatar_url.as_deref(),
                                        &user_name,
                                        26.0,
                                        false,
                                    );
                                    let is_dark = this.is_dark_theme();
                                    if let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                    {
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 0.0;
                                            let resp = ui.add(
                                                egui::TextEdit::multiline(&mut state.reply_draft)
                                                    .id(reply_input_id)
                                                    .desired_rows(1)
                                                    .desired_width(f32::INFINITY)
                                                    .frame(egui::Frame::NONE)
                                                    .hint_text(
                                                        RichText::new("Adicione uma resposta...")
                                                            .size(13.0)
                                                            .color(palette.neutral_status_fg),
                                                    ),
                                            );
                                            let line_y = resp.rect.bottom() + 4.0;
                                            let line_stroke = if resp.has_focus() {
                                                egui::Stroke::new(2.0, palette.header_button_fg)
                                            } else {
                                                egui::Stroke::new(
                                                    1.0,
                                                    palette.neutral_status_fg.gamma_multiply(0.5),
                                                )
                                            };
                                            ui.painter().line_segment(
                                                [
                                                    egui::pos2(resp.rect.left(), line_y),
                                                    egui::pos2(ui.max_rect().right(), line_y),
                                                ],
                                                line_stroke,
                                            );
                                            ui.add_space(8.0);

                                            let sending = state.is_posting_reply();
                                            let ready =
                                                !sending && !state.reply_draft.trim().is_empty();
                                            let ctrl_enter = resp.has_focus()
                                                && ui.input(|i| {
                                                    i.modifiers.ctrl
                                                        && i.key_pressed(egui::Key::Enter)
                                                });
                                            if ctrl_enter && ready {
                                                post_reply = true;
                                            }

                                            ui.horizontal(|ui| {
                                                let emoji_btn = egui::Button::image(
                                                    Icon::Smile.image(palette.icon_fg, 17.0),
                                                )
                                                .fill(if reply_emoji_open {
                                                    palette.card_alt_bg
                                                } else {
                                                    Color32::TRANSPARENT
                                                })
                                                .frame(false)
                                                .corner_radius(14)
                                                .min_size(egui::vec2(28.0, 28.0));
                                                if ui
                                                    .add(emoji_btn)
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .on_hover_text("Inserir emoji")
                                                    .clicked()
                                                {
                                                    toggle_reply_emoji = true;
                                                }

                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        ui.spacing_mut().item_spacing.x = 8.0;
                                                        let responder_btn = egui::Button::new(
                                                            RichText::new("Responder")
                                                                .size(12.5)
                                                                .strong()
                                                                .color(if ready {
                                                                    palette.active_button_fg
                                                                } else {
                                                                    palette.neutral_status_fg
                                                                }),
                                                        )
                                                        .fill(if ready {
                                                            palette.active_button_bg
                                                        } else {
                                                            palette.header_button_bg
                                                        })
                                                        .corner_radius(16)
                                                        .min_size(egui::vec2(88.0, 30.0));
                                                        if ui
                                                            .add_enabled(ready, responder_btn)
                                                            .on_hover_cursor(
                                                                egui::CursorIcon::PointingHand,
                                                            )
                                                            .clicked()
                                                        {
                                                            post_reply = true;
                                                        }

                                                        let cancel_btn = egui::Button::new(
                                                            RichText::new("Cancelar")
                                                                .size(12.5)
                                                                .strong()
                                                                .color(palette.header_button_fg),
                                                        )
                                                        .fill(Color32::TRANSPARENT)
                                                        .frame(false)
                                                        .corner_radius(16)
                                                        .min_size(egui::vec2(68.0, 30.0));
                                                        if ui
                                                            .add(cancel_btn)
                                                            .on_hover_cursor(
                                                                egui::CursorIcon::PointingHand,
                                                            )
                                                            .clicked()
                                                        {
                                                            cancel_reply = true;
                                                            resp.surrender_focus();
                                                        }
                                                    },
                                                );
                                            });

                                            if reply_emoji_open {
                                                ui.add_space(6.0);
                                                picked_reply_emoji = Self::comment_emoji_picker_ui(
                                                    ui,
                                                    &palette,
                                                    ("reply_emoji_picker", &comment.id),
                                                );
                                            }
                                            if state.is_posting_reply() {
                                                ui.add_space(4.0);
                                                skeleton::comments_skeleton(
                                                    ui,
                                                    ui.available_width(),
                                                    1,
                                                    is_dark,
                                                );
                                            }
                                            if let Some(err) = &state.reply_post_error {
                                                ui.colored_label(palette.warning_fg, err);
                                            }
                                        });
                                    }
                                });

                                if toggle_reply_emoji {
                                    if reply_emoji_open {
                                        this.watch.extras.reply_emoji_picker_for = None;
                                    } else {
                                        this.watch.extras.reply_emoji_picker_for =
                                            Some(comment.id.clone());
                                        this.watch.extras.comment_emoji_picker_open = false;
                                    }
                                }
                                if let Some(emoji) = picked_reply_emoji
                                    && let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                {
                                    Self::insert_emoji_into_draft(
                                        ctx,
                                        reply_input_id,
                                        &mut state.reply_draft,
                                        emoji,
                                    );
                                }
                                if cancel_reply
                                    && let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                {
                                    state.reply_open = false;
                                    state.reply_post_error = None;
                                    if this.watch.extras.reply_emoji_picker_for.as_deref()
                                        == Some(comment.id.as_str())
                                    {
                                        this.watch.extras.reply_emoji_picker_for = None;
                                    }
                                } else if post_reply {
                                    if this.watch.extras.reply_emoji_picker_for.as_deref()
                                        == Some(comment.id.as_str())
                                    {
                                        this.watch.extras.reply_emoji_picker_for = None;
                                    }
                                    this.watch.extras.start_post_reply(
                                        &comment.id,
                                        this.auth.session.as_ref(),
                                        ctx,
                                    );
                                }
                            }

                            let has_local_replies = this
                                .watch
                                .extras
                                .comment_replies
                                .get(&comment.id)
                                .is_some_and(|state| !state.comments.is_empty() || state.loaded);
                            if comment.replies > 0 || has_local_replies {
                                let state = this.watch.extras.comment_replies.get(&comment.id);
                                if comment.replies > 0 && state.is_none_or(|state| !state.loaded) {
                                    let loading = state.is_some_and(|state| state.is_loading());
                                    let reply_word = if comment.replies == 1 {
                                        if pt {
                                            "1 resposta \u{2304}".to_owned()
                                        } else {
                                            "1 reply \u{2304}".to_owned()
                                        }
                                    } else if pt {
                                        format!("{} respostas \u{2304}", comment.replies)
                                    } else {
                                        format!("{} replies \u{2304}", comment.replies)
                                    };
                                    let btn = egui::Button::new(
                                        RichText::new(reply_word)
                                            .size(13.0)
                                            .strong()
                                            .color(Color32::from_rgb(62, 166, 255)),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .frame(false);
                                    if ui
                                        .add_enabled(!loading, btn)
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                    {
                                        this.watch.extras.start_replies(
                                            &comment.id,
                                            false,
                                            this.auth.session.as_ref(),
                                            ctx,
                                        );
                                    }
                                }
                                let (loaded, loading, error, has_replies, has_next) = this
                                    .watch
                                    .extras
                                    .comment_replies
                                    .get(&comment.id)
                                    .map(|state| {
                                        (
                                            state.loaded,
                                            state.is_loading(),
                                            state.error.clone(),
                                            !state.comments.is_empty(),
                                            state.next.is_some(),
                                        )
                                    })
                                    .unwrap_or_default();
                                if loading {
                                    ui.add_space(4.0);
                                    skeleton::comments_skeleton(
                                        ui,
                                        ui.available_width(),
                                        2,
                                        this.is_dark_theme(),
                                    );
                                }
                                if let Some(error) = error {
                                    ui.colored_label(palette.warning_fg, error);
                                    if styled_compact_button(
                                        ui,
                                        "Tentar novamente",
                                        false,
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        let retry_next = this
                                            .watch
                                            .extras
                                            .comment_replies
                                            .get(&comment.id)
                                            .is_some_and(
                                                watch_extras::CommentReplies::retry_uses_next_page,
                                            );
                                        this.watch.extras.start_replies(
                                            &comment.id,
                                            retry_next,
                                            this.auth.session.as_ref(),
                                            ctx,
                                        );
                                    }
                                }
                                if loaded && has_replies {
                                    let (replies, reply_metas) = this
                                        .watch
                                        .extras
                                        .comment_replies
                                        .get_mut(&comment.id)
                                        .map(|state| {
                                            watch_extras::sync_cached_comment_meta(
                                                &state.comments,
                                                &mut state.render_cache,
                                            );
                                            (
                                                std::mem::take(&mut state.comments),
                                                std::mem::take(&mut state.render_cache),
                                            )
                                        })
                                        .unwrap_or_default();
                                    for (reply, r_meta) in replies.iter().zip(reply_metas.iter()) {
                                        ui.add_space(6.0);
                                        ui.horizontal_top(|ui| {
                                            ui.spacing_mut().item_spacing.x = 10.0;
                                            let reply_avatar_resp = this.paint_avatar_or_initials(
                                                ui,
                                                ctx,
                                                reply.author_avatar.as_deref(),
                                                &r_meta.avatar_name,
                                                26.0,
                                                false,
                                            );
                                            Self::paint_comment_avatar_ring(
                                                ui,
                                                reply_avatar_resp.rect,
                                            );
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 3.0;
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label(
                                                        RichText::new(&r_meta.handle)
                                                            .size(12.5)
                                                            .strong()
                                                            .color(palette.header_button_fg),
                                                    );
                                                    if !r_meta.relative_time.is_empty() {
                                                        ui.label(
                                                            RichText::new(&r_meta.relative_time)
                                                                .size(11.5)
                                                                .color(palette.neutral_status_fg),
                                                        );
                                                    }
                                                });
                                                this.comment_item_body_and_actions_ui(
                                                    ui,
                                                    ctx,
                                                    reply,
                                                    &r_meta.lines,
                                                    Some(&comment.id),
                                                    13.0,
                                                    false,
                                                );
                                            });
                                        });
                                    }
                                    if let Some(state) =
                                        this.watch.extras.comment_replies.get_mut(&comment.id)
                                        && state.comments.is_empty()
                                    {
                                        state.comments = replies;
                                        state.render_cache = reply_metas;
                                    }
                                }
                                if has_next
                                    && ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Mostrar mais respostas \u{2304}")
                                                    .size(12.5)
                                                    .strong()
                                                    .color(Color32::from_rgb(62, 166, 255)),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                {
                                    this.watch.extras.start_replies(
                                        &comment.id,
                                        true,
                                        this.auth.session.as_ref(),
                                        ctx,
                                    );
                                }
                            }
                        });
                    });
                    meta.measured_height =
                        Some((width_key, row_hash, row_resp.response.rect.height()));
                }
                ui.add_space(6.0);
            }
            if this.watch.extras.video_url == comments_video_url
                && this.watch.extras.comments.is_empty()
                && comments.len() == initial_comments_len
            {
                this.watch.extras.comments = comments;
                this.watch.extras.comment_render_cache = comment_metas;
            }

            if this.watch.extras.comments_loading.is_some()
                || this.watch.extras.innertube_comments_loading
                || (scroll_id == "shorts_comments"
                    && !this.watch.extras.comments_loaded
                    && this.watch.next_task.is_running())
            {
                ui.add_space(8.0);
                let dark_theme = this.is_dark_theme();
                skeleton::comments_skeleton(ui, ui.available_width(), 4, dark_theme);
            } else if this.watch.extras.is_posting_comment() {
                ui.add_space(6.0);
                skeleton::comments_skeleton(ui, ui.available_width(), 1, this.is_dark_theme());
            }

            if (this.watch.extras.comments_loaded || !this.watch.extras.comments.is_empty())
                && let Some(cont) = this.watch.extras.innertube_comments_continuation.clone()
            {
                ui.add_space(8.0);
                let load_more_label = if pt {
                    "Carregar mais coment\u{00e1}rios"
                } else {
                    "Load more comments"
                };
                let manual_click = ui
                    .add_enabled_ui(!this.watch.extras.innertube_comments_loading, |ui| {
                        styled_pill_button(ui, load_more_label, false, &palette)
                    })
                    .inner
                    .clicked();
                if manual_click {
                    this.start_load_innertube_comments(cont, false, ctx);
                }
            } else if this.watch.extras.comments_next.is_some() {
                ui.add_space(8.0);
                let (sentinel_rect, _) =
                    ui.allocate_exact_size(egui::vec2(32.0, 8.0), egui::Sense::hover());
                let auto_trigger = ui.is_rect_visible(sentinel_rect)
                    && this.watch.extras.comments_loading.is_none()
                    && this.watch.extras.comments_error.is_none();
                let manual_click = ui
                    .add_enabled_ui(this.watch.extras.comments_loading.is_none(), |ui| {
                        styled_pill_button(
                            ui,
                            if pt {
                                "Carregar mais coment\u{00e1}rios"
                            } else {
                                "Load more comments"
                            },
                            false,
                            &palette,
                        )
                    })
                    .inner
                    .clicked();
                if auto_trigger || manual_click {
                    this.watch
                        .extras
                        .start_comments(this.auth.session.as_ref(), true, ctx);
                }
            }
        };

        if scroll_id == "shorts_comments" {
            egui::ScrollArea::vertical()
                .id_salt(format!("{scroll_id}_comments"))
                .show(ui, |ui| {
                    render_comments_body(self, ui);
                });
        } else {
            render_comments_body(self, ui);
        }
    }
}
