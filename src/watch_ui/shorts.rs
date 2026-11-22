use super::metadata_sidebar::is_watch_channel_verified;
use super::*;
use crate::app_state::ShortsSidePanel;
use eframe::egui::RichText;

#[derive(Default)]
struct ShortsWatchActions {
    rating_to_apply: Option<Option<oauth::VideoRating>>,
    toggle_subscribe: bool,
    enqueue_video: bool,
    share_video: bool,
    toggle_save_menu: bool,
    toggle_comments: bool,
    toggle_description: bool,
    toggle_loop: bool,
    prev_short: bool,
    next_short: bool,
    open_channel_target: Option<ChannelTarget>,
}

pub(crate) fn format_short_compact_count(count: u64, pt: bool) -> String {
    if count >= 1_000_000_000 {
        let whole = count / 1_000_000_000;
        let tenths = (count / 100_000_000) % 10;
        return if whole >= 10 || tenths == 0 {
            if pt {
                format!("{whole} bi")
            } else {
                format!("{whole}B")
            }
        } else if pt {
            format!("{whole},{tenths} bi")
        } else {
            format!("{whole}.{tenths}B")
        };
    }
    if count >= 1_000_000 {
        let whole = count / 1_000_000;
        let tenths = (count / 100_000) % 10;
        return if whole >= 10 || tenths == 0 {
            if pt {
                format!("{whole} mi")
            } else {
                format!("{whole}M")
            }
        } else if pt {
            format!("{whole},{tenths} mi")
        } else {
            format!("{whole}.{tenths}M")
        };
    }
    if count >= 1_000 {
        let whole = count / 1_000;
        let tenths = (count / 100) % 10;
        return if whole >= 10 || tenths == 0 {
            if pt {
                format!("{whole} mil")
            } else {
                format!("{whole}K")
            }
        } else if pt {
            format!("{whole},{tenths} mil")
        } else {
            format!("{whole}.{tenths}K")
        };
    }
    count.to_string()
}

pub(crate) fn strip_views_word(text: &str) -> String {
    let mut s = text
        .trim()
        .trim_start_matches(['\u{25b7}', '\u{25b6}', ' '])
        .trim()
        .to_owned();
    let lower = s.to_lowercase();
    for suffix in [
        "visualiza\u{00e7}\u{00f5}es",
        "visualiza\u{00e7}\u{00e3}o",
        "visualizacoes",
        "visualizacao",
        "views",
        "view",
    ] {
        if lower.ends_with(suffix) {
            let new_len = s.len().saturating_sub(suffix.len());
            if s.is_char_boundary(new_len) {
                s.truncate(new_len);
                s = s.trim().trim_end_matches('\u{2022}').trim().to_owned();
            }
            break;
        }
    }
    s
}

fn parse_relative_short_age(raw: &str, pt: bool) -> Option<(String, String)> {
    let trimmed = raw.trim();
    let lower = trimmed.to_lowercase();
    let body = if let Some(rest) = lower
        .strip_prefix("h\u{00e1} ")
        .or_else(|| lower.strip_prefix("ha "))
    {
        rest.trim()
    } else {
        lower.strip_suffix(" ago")?.trim()
    };

    let mut parts = body.split_whitespace();
    let num = parts.next()?;
    if !num
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }
    let unit_raw = parts.next().unwrap_or("");
    let unit = if unit_raw.starts_with("hora") || unit_raw.starts_with("hour") || unit_raw == "h" {
        "h"
    } else if unit_raw.starts_with("dia") || unit_raw.starts_with("day") || unit_raw == "d" {
        "d"
    } else if unit_raw.starts_with("min") {
        "min"
    } else if unit_raw.starts_with("seg") || unit_raw.starts_with("sec") || unit_raw == "s" {
        "s"
    } else if unit_raw.starts_with("sem") || unit_raw.starts_with("week") || unit_raw == "w" {
        if pt { "sem" } else { "w" }
    } else if unit_raw.starts_with("m\u{00ea}s")
        || unit_raw.starts_with("mes")
        || unit_raw.starts_with("month")
    {
        if pt { "m" } else { "mo" }
    } else if unit_raw.starts_with("ano") || unit_raw.starts_with("year") || unit_raw == "y" {
        if pt { "a" } else { "y" }
    } else {
        unit_raw
    };
    let label = if pt { "Atr\u{00e1}s" } else { "Ago" };
    Some((format!("{num} {unit}").trim().to_owned(), label.to_owned()))
}

fn parse_ymd_digits(raw: &str) -> Option<(u32, u32, u32)> {
    let digits: String = raw.trim().chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 8 {
        return None;
    }
    let year = digits[0..4].parse::<u32>().ok()?;
    let month = digits[4..6].parse::<u32>().ok()?;
    let day = digits[6..8].parse::<u32>().ok()?;
    if !(1970..=2100).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn short_month_label(month: u32, pt: bool) -> &'static str {
    const PT_MONTHS: [&str; 12] = [
        "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.",
        "dez.",
    ];
    const EN_MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let idx = month.saturating_sub(1).min(11) as usize;
    if pt { PT_MONTHS[idx] } else { EN_MONTHS[idx] }
}

pub(crate) fn format_short_stat_date(
    published_date_text: Option<&str>,
    published_time_text: Option<&str>,
    upload_date: Option<&str>,
    pt: bool,
) -> (String, String) {
    if let Some(date_raw) = published_date_text.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(rel) = parse_relative_short_age(date_raw, pt) {
            return rel;
        }
        let tokens: Vec<&str> = date_raw
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|t| !t.is_empty())
            .collect();
        if let Some(year_idx) = tokens
            .iter()
            .rposition(|t| t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()))
        {
            let year = tokens[year_idx].to_owned();
            let rest: String = tokens
                .iter()
                .enumerate()
                .filter(|(i, t)| *i != year_idx && !t.eq_ignore_ascii_case("de"))
                .map(|(_, t)| *t)
                .collect::<Vec<_>>()
                .join(" ");
            if !rest.is_empty() {
                return (rest, year);
            }
        }
        return (
            date_raw.to_owned(),
            if pt { "Publicado" } else { "Published" }.to_owned(),
        );
    }

    if let Some(rel_raw) = published_time_text.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(rel) = parse_relative_short_age(rel_raw, pt) {
            return rel;
        }
        return (
            rel_raw.to_owned(),
            if pt { "Publicado" } else { "Published" }.to_owned(),
        );
    }

    if let Some(raw_ymd) = upload_date.map(str::trim).filter(|s| !s.is_empty())
        && let Some((year, month, day)) = parse_ymd_digits(raw_ymd)
    {
        return (
            format!("{day} {}", short_month_label(month, pt)),
            format!("{year:04}"),
        );
    }

    (
        "\u{2014}".to_owned(),
        if pt { "Data" } else { "Date" }.to_owned(),
    )
}

pub(crate) fn resolve_short_channel_handle(
    channel_target: Option<&ChannelTarget>,
    channel_name: &str,
) -> String {
    if let Some(ChannelTarget::Handle(handle)) = channel_target {
        let trimmed = handle.trim();
        if !trimmed.is_empty() {
            return if trimmed.starts_with('@') {
                trimmed.to_owned()
            } else {
                format!("@{trimmed}")
            };
        }
    }
    channel_name.trim().to_owned()
}

struct RailCircleStyle {
    size: f32,
    fill: Color32,
    hover_fill: Color32,
    active_fill: Option<Color32>,
    enabled: bool,
}

fn paint_rail_icon_circle<F>(
    ui: &mut egui::Ui,
    style: RailCircleStyle,
    tooltip: &str,
    paint_icon: F,
) -> egui::Response
where
    F: FnOnce(&egui::Painter, egui::Pos2),
{
    let sense = if style.enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(style.size, style.size), sense);
    let bg = if !style.enabled {
        style.fill.gamma_multiply(0.45)
    } else if let Some(active) = style.active_fill {
        active
    } else if response.hovered() {
        style.hover_fill
    } else {
        style.fill
    };
    ui.painter()
        .circle_filled(rect.center(), style.size * 0.5, bg);
    paint_icon(ui.painter(), rect.center());
    paint_focus_ring_if_focused(ui, &response, style.size * 0.5);
    let resp = if style.enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    resp.on_hover_text(tooltip)
}

struct ShortMetadataDisplay<'a> {
    handle_label: &'a str,
    avatar_fallback: &'a str,
    channel_avatar_url: Option<&'a str>,
    channel_target: Option<&'a ChannelTarget>,
    subscribed: Option<bool>,
    sub_pending: bool,
    in_video: bool,
}

struct ShortDescriptionMeta<'a> {
    channel_target: Option<&'a ChannelTarget>,
    channel_avatar_url: Option<&'a str>,
    channel_verified: bool,
    like_stat_value: &'a str,
    views_stat_value: &'a str,
    date_primary: &'a str,
    date_secondary: &'a str,
}

impl FastTube {
    fn render_short_metadata_block(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        meta: ShortMetadataDisplay<'_>,
        palette: &ThemePalette,
        actions: &mut ShortsWatchActions,
    ) {
        let ShortMetadataDisplay {
            handle_label,
            avatar_fallback,
            channel_avatar_url,
            channel_target,
            subscribed,
            sub_pending,
            in_video,
        } = meta;
        let pt = self.ui_pt();
        let text_fg = if in_video {
            Color32::WHITE
        } else {
            palette.channel_fg
        };
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if self
                .paint_avatar_or_initials(
                    ui,
                    ctx,
                    channel_avatar_url,
                    avatar_fallback,
                    34.0,
                    channel_target.is_some(),
                )
                .clicked()
            {
                actions.open_channel_target = channel_target.cloned();
            }
            if !handle_label.is_empty()
                && ui
                    .add(
                        egui::Button::new(
                            RichText::new(truncate_text_chars(handle_label, 24))
                                .size(14.0)
                                .strong()
                                .color(text_fg),
                        )
                        .frame(false),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
            {
                actions.open_channel_target = channel_target.cloned();
            }
            if channel_target.is_some() {
                let is_sub = subscribed == Some(true);
                let sub_label = if self.auth.authenticated {
                    if is_sub {
                        if pt {
                            "\u{2713} Inscrito"
                        } else {
                            "\u{2713} Subscribed"
                        }
                    } else if pt {
                        "Inscrever-se"
                    } else {
                        "Subscribe"
                    }
                } else if is_sub {
                    if pt {
                        "\u{2713} Seguindo"
                    } else {
                        "\u{2713} Following"
                    }
                } else if pt {
                    "Seguir"
                } else {
                    "Follow"
                };
                if ui
                    .add_enabled_ui(!sub_pending, |ui| {
                        styled_pill_button(ui, sub_label, !is_sub, palette)
                    })
                    .inner
                    .clicked()
                {
                    actions.toggle_subscribe = true;
                }
            }
        });

        let title_w = ui.available_width().max(120.0);
        let galley = layout_two_line_clamped_galley(
            ui,
            &video.title,
            title_w,
            egui::FontId::proportional(14.5),
            text_fg,
        );
        let (t_rect, title_resp) = ui.allocate_exact_size(
            egui::vec2(title_w, galley.size().y.max(18.0)),
            egui::Sense::click(),
        );
        ui.painter().galley(t_rect.min, galley, text_fg);
        paint_focus_ring_if_focused(ui, &title_resp, 6.0);
        if title_resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(if pt {
                "Clique para ver a descri\u{00e7}\u{00e3}o"
            } else {
                "Click to view description"
            })
            .clicked()
        {
            actions.toggle_description = true;
        }
    }

    fn render_shorts_description_panel(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        video: &Video,
        meta: ShortDescriptionMeta<'_>,
        palette: &ThemePalette,
        actions: &mut ShortsWatchActions,
    ) {
        let ShortDescriptionMeta {
            channel_target,
            channel_avatar_url,
            channel_verified,
            like_stat_value,
            views_stat_value,
            date_primary,
            date_secondary,
        } = meta;
        let pt = self.ui_pt();
        let panel_inner_w = ui.available_width().max(260.0);
        ui.horizontal_top(|ui| {
            let header_w = (panel_inner_w - 40.0).max(120.0);
            ui.allocate_ui_with_layout(
                egui::vec2(header_w, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&video.title)
                                .size(18.0)
                                .strong()
                                .color(palette.channel_fg),
                        )
                        .wrap(),
                    );
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                let (close_rect, close_resp) =
                    ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
                if close_resp.hovered() {
                    ui.painter()
                        .circle_filled(close_rect.center(), 16.0, palette.header_button_bg);
                }
                paint_close_x_icon(ui, close_rect, palette.header_button_fg);
                paint_focus_ring_if_focused(ui, &close_resp, 16.0);
                if close_resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(if pt { "Fechar" } else { "Close" })
                    .clicked()
                {
                    actions.toggle_description = true;
                }
            });
        });

        ui.add_space(10.0);
        let ch_display = if !video.channel.trim().is_empty() {
            video.channel.trim()
        } else {
            channel_target
                .map(channel_target_display_label)
                .unwrap_or(if pt { "Canal" } else { "Channel" })
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if self
                .paint_avatar_or_initials(
                    ui,
                    ctx,
                    channel_avatar_url,
                    ch_display,
                    32.0,
                    channel_target.is_some(),
                )
                .clicked()
            {
                actions.open_channel_target = channel_target.cloned();
            }
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(truncate_text_chars(ch_display, 34))
                            .size(14.5)
                            .strong()
                            .color(palette.channel_fg),
                    )
                    .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                actions.open_channel_target = channel_target.cloned();
            }
            if channel_verified {
                paint_verified_badge_sized(ui, palette, 14.0);
            }
        });

        ui.add_space(14.0);
        let col_w = ((panel_inner_w - 16.0).max(150.0) / 3.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let stats = [
                (
                    like_stat_value,
                    if pt {
                        "Marca\u{00e7}\u{00f5}es Gostei"
                    } else {
                        "Likes"
                    },
                ),
                (
                    views_stat_value,
                    if pt {
                        "Visualiza\u{00e7}\u{00f5}es"
                    } else {
                        "Views"
                    },
                ),
                (date_primary, date_secondary),
            ];
            for (val, label) in stats {
                ui.allocate_ui_with_layout(
                    egui::vec2(col_w, 40.0),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new(val)
                                .size(17.5)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        ui.label(
                            RichText::new(label)
                                .size(11.5)
                                .color(palette.neutral_status_fg),
                        );
                    },
                );
            }
        });

        ui.add_space(14.0);
        egui::ScrollArea::vertical()
            .id_salt("shorts_description_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let has_desc = self
                    .watch
                    .metadata_cache
                    .cleaned_description
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty());
                if has_desc && self.watch.metadata_cache.description_lines.is_none() {
                    let parsed = self
                        .watch
                        .metadata_cache
                        .cleaned_description
                        .as_deref()
                        .map(watch_extras::parse_timestamp_lines)
                        .unwrap_or_default();
                    self.watch.metadata_cache.description_lines = Some(parsed);
                }
                let lines = std::mem::take(&mut self.watch.metadata_cache.description_lines);
                let chapters = self.watch.metadata_cache.description_chapters.clone();

                egui::Frame::new()
                    .fill(palette.card_alt_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        ui.set_width((panel_inner_w - 28.0).max(120.0));
                        ui.set_max_width((panel_inner_w - 28.0).max(120.0));
                        if let Some(lines_vec) = lines.as_deref().filter(|l| !l.is_empty()) {
                            self.render_rich_description_lines_ui(
                                ui,
                                ctx,
                                lines_vec,
                                palette.description_fg,
                                13.5,
                            );
                        } else {
                            ui.label(
                                RichText::new(if pt {
                                    "Sem descri\u{00e7}\u{00e3}o."
                                } else {
                                    "No description."
                                })
                                .size(13.0)
                                .color(palette.neutral_status_fg),
                            );
                        }
                    });

                if self.watch.metadata_cache.video_url.as_deref() == Some(video.url.as_str()) {
                    self.watch.metadata_cache.description_lines = lines;
                }

                if self.playback.audio_tracks.len() > 1 {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(if pt {
                            "Como este conte\u{00fa}do foi criado"
                        } else {
                            "How this content was made"
                        })
                        .size(14.0)
                        .strong()
                        .color(palette.channel_fg),
                    );
                    ui.add_space(4.0);
                    let active_track = self
                        .playback
                        .audio_tracks
                        .iter()
                        .find(|t| t.selected)
                        .map(|t| t.label.as_str())
                        .unwrap_or_default();
                    if !active_track.is_empty() {
                        ui.label(
                            RichText::new(active_track)
                                .size(12.5)
                                .color(palette.neutral_status_fg),
                        );
                    }
                }

                if !chapters.is_empty() {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(if pt { "Cap\u{00ed}tulos" } else { "Chapters" })
                            .size(14.0)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    ui.add_space(6.0);
                    let mut seek_to_secs = None;
                    for ch in &chapters {
                        let label = format!("{}  {}", ch.label, ch.title);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label)
                                        .size(12.5)
                                        .color(palette.header_button_fg),
                                )
                                .fill(palette.card_alt_bg)
                                .corner_radius(8),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            seek_to_secs = Some(ch.seconds);
                        }
                    }
                    if let Some(secs) = seek_to_secs {
                        self.seek_to_seconds(secs as f64);
                    }
                }
            });
    }

    pub(crate) fn shorts_watch_page_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<Video> {
        let video = self.selected.clone()?;
        let pt = self.ui_pt();
        let palette = self.palette();
        let dark_theme = self.is_dark_theme();

        // Trigger next-Short preload once per active Short URL.
        let prefetch_id = egui::Id::new("fasttube_shorts_last_prefetched_url");
        let last_prefetched = ctx.data(|d| d.get_temp::<String>(prefetch_id));
        if last_prefetched.as_deref() != Some(video.url.as_str()) {
            ctx.data_mut(|d| d.insert_temp(prefetch_id, video.url.clone()));
            self.prefetch_next_short(ctx);
        }

        self.ensure_watch_metadata_cache(&video);

        let (
            channel_target,
            channel_id_hint,
            channel_avatar_url,
            channel_verified,
            view_count,
            view_count_meta_text,
            published_time_text,
            upload_date,
        ) = {
            let metadata = self.metadata_for(&video.url);
            let live_avatar = self
                .watch
                .live
                .info
                .as_ref()
                .filter(|_| self.watch.live.video_url.as_deref() == Some(video.url.as_str()))
                .and_then(|info| info.channel_avatar_url.clone());
            let target = metadata.and_then(search::channel_target_for_metadata);
            let verified =
                is_watch_channel_verified(metadata, self.channels.page.as_ref(), target.as_ref());
            (
                target,
                metadata.and_then(|meta| meta.channel_id.clone()),
                live_avatar.or_else(|| metadata.and_then(|meta| meta.channel_avatar_url.clone())),
                verified,
                metadata.and_then(|meta| meta.view_count),
                metadata.and_then(|meta| meta.view_count_text.clone()),
                metadata.and_then(|meta| meta.published_time_text.clone()),
                metadata.and_then(|meta| meta.upload_date.clone()),
            )
        };
        let handle_label = resolve_short_channel_handle(channel_target.as_ref(), &video.channel);
        let avatar_fallback = if !video.channel.trim().is_empty() {
            video.channel.trim()
        } else {
            &handle_label
        };

        let viewport_rect = ui.max_rect();
        let available_width = ui.available_width().max(320.0);
        let available_height = ui.available_height().max(420.0);
        let nav_gutter_w = 68.0_f32;
        let rail_w = 72.0_f32;
        let rail_gap = 12.0_f32;
        let stage_avail_h = (available_height - 20.0).max(400.0);
        let max_stage_w = (available_width - rail_w - rail_gap - nav_gutter_w - 32.0).max(220.0);
        let stage_size = compute_player_size_with_theater(
            max_stage_w,
            stage_avail_h,
            false,
            false,
            WatchLayoutMode::Short,
        );
        let stage_w = stage_size.x;
        let stage_h = stage_size.y;

        if self.watch.shorts_side_panel == ShortsSidePanel::None
            && self.watch.extras.tab == watch_extras::Tab::Comments
        {
            self.watch.shorts_side_panel = ShortsSidePanel::Comments;
        }
        if self.watch.shorts_side_panel == ShortsSidePanel::Comments
            && !self.watch.extras.comments_loaded
            && self.watch.extras.comments_loading.is_none()
            && !self.watch.extras.innertube_comments_loading
            && self.watch.extras.comments_error.is_none()
            && let Some(cont) = self.watch.extras.innertube_comments_continuation.clone()
        {
            self.start_load_innertube_comments(cont, true, ctx);
        }
        let side_panel = self.watch.shorts_side_panel;
        let panel_gap = 20.0_f32;
        let min_panel_w = 340.0_f32;
        let side_area_avail = available_width - nav_gutter_w - 24.0;
        let panel_w = if side_panel != ShortsSidePanel::None
            && side_area_avail >= stage_w + rail_gap + rail_w + panel_gap + min_panel_w
        {
            (side_area_avail - stage_w - rail_gap - rail_w - panel_gap).clamp(min_panel_w, 420.0)
        } else {
            0.0
        };
        let cluster_w = stage_w
            + rail_gap
            + rail_w
            + if panel_w > 0.0 {
                panel_gap + panel_w
            } else {
                0.0
            };
        let usable_w = (available_width - nav_gutter_w).max(cluster_w + 16.0);
        let stage_left_offset = ((usable_w - cluster_w) * 0.5).max(12.0);
        let top_pad = ((available_height - stage_h) * 0.5).clamp(8.0, 20.0);
        let metadata_outside_left =
            side_panel == ShortsSidePanel::None && stage_left_offset >= 250.0;

        let saved_in_library = self.video_saved_in_library(&video);
        let save_menu_open = self.playback.save_menu_open;
        let subscribed = if self.auth.authenticated {
            self.subscription_status_for(channel_target.as_ref(), channel_id_hint.as_deref())
                .map(|s| s.subscription_id.is_some())
        } else {
            Some(
                self.followed_channels
                    .data
                    .is_following_target(channel_target.as_ref(), channel_id_hint.as_deref()),
            )
        };
        let sub_pending = self.auth.authenticated && self.auth.sub_toggle_task.is_running();
        let video_id = history::youtube_video_id(&video.url);
        let current_rating = video_id
            .as_ref()
            .and_then(|id| self.auth.ratings.get(id).copied());
        let rating_pending = self.auth.rating_task.is_running();
        let ryd_stats = if self.settings.prefs.value.ryd_enabled {
            video_id
                .as_deref()
                .and_then(|id| self.watch.community_cache.get(id))
                .and_then(|c| c.ryd.clone())
        } else {
            None
        };
        let next_short_preview = self.next_or_prev_short(true);
        let prev_short_available = self.next_or_prev_short(false).is_some();
        let has_next_short = next_short_preview.is_some();
        let short_index_and_total = self.short_index_and_total();
        let loop_video = self.playback.loop_video;
        let share_recently_copied = self
            .watch
            .share_copied_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(2));

        let like_caption = self
            .watch
            .extras
            .like_count_text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                ryd_stats
                    .as_ref()
                    .map(|r| format_short_compact_count(r.likes, pt))
            })
            .unwrap_or_else(|| if pt { "Gostei" } else { "Like" }.to_owned());
        let like_stat_value = self
            .watch
            .extras
            .like_count_text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                ryd_stats
                    .as_ref()
                    .map(|r| format_short_compact_count(r.likes, pt))
            })
            .unwrap_or_else(|| "\u{2014}".to_owned());
        let views_stat_value = self
            .watch
            .extras
            .view_count_text
            .as_deref()
            .map(strip_views_word)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                view_count_meta_text
                    .as_deref()
                    .map(strip_views_word)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| view_count.map(|v| format_short_compact_count(v, pt)))
            .unwrap_or_else(|| "\u{2014}".to_owned());
        let (date_primary, date_secondary) = format_short_stat_date(
            self.watch.extras.published_date_text.as_deref(),
            published_time_text.as_deref(),
            upload_date.as_deref(),
            pt,
        );
        let comment_caption = self
            .watch
            .extras
            .comments_total
            .map(|c| format_short_compact_count(c, pt))
            .unwrap_or_else(|| if pt { "Coment." } else { "Comments" }.to_owned());

        let mut actions = ShortsWatchActions::default();
        let mut open_report_modal = false;
        let mut toggle_player_settings = false;
        let mut drawer_rect = egui::Rect::NOTHING;
        let mut video_rect = egui::Rect::NOTHING;
        let mut block_rects: Vec<egui::Rect> = Vec::new();
        let top_origin = ui.cursor().top();

        let rail_circle_fill = if dark_theme {
            Color32::from_rgb(39, 39, 39)
        } else {
            palette.header_button_bg
        };
        let rail_circle_hover = if dark_theme {
            Color32::from_rgb(62, 62, 62)
        } else {
            palette.active_button_bg
        };
        let rail_icon_fg = palette.channel_fg;

        self.thumbnails.begin_pass();
        let stage_resp = ui.vertical(|ui| {
            ui.add_space(top_pad);
            ui.horizontal_top(|ui| {
                ui.add_space(stage_left_offset);

                // Centered 9:16 video stage + optional next-Short peek strip.
                let stage_col = ui.vertical(|ui| {
                    ui.set_width(stage_w);
                    ui.set_max_width(stage_w);
                    let stage_top_left = ui.cursor().min;
                    self.watch_primary_ui(ui, ctx, stage_w, stage_avail_h, WatchLayoutMode::Short);
                    video_rect = egui::Rect::from_min_size(stage_top_left, stage_size);

                    if let Some(next_short) = &next_short_preview {
                        let remaining_below =
                            (top_origin + available_height) - (video_rect.bottom() + 10.0);
                        if remaining_below >= 26.0 {
                            let peek_h = remaining_below.clamp(20.0, 32.0);
                            ui.add_space(10.0);
                            let peek_resp = self
                                .thumbnails
                                .show_cover_rounded(
                                    ui,
                                    ctx,
                                    &next_short.thumbnail,
                                    egui::vec2(stage_w, peek_h),
                                    12.0,
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(&next_short.title);
                            if peek_resp.clicked() {
                                actions.next_short = true;
                            }
                        }
                    }
                });
                let _ = stage_col;

                ui.add_space(rail_gap);

                // Right vertical YouTube Shorts action rail (Like, Comments, Share, Channel).
                ui.vertical(|ui| {
                    ui.set_width(rail_w);
                    ui.set_max_width(rail_w);
                    ui.spacing_mut().item_spacing.y = 14.0;
                    let rail_content_h = 336.0_f32;
                    ui.add_space((stage_h - rail_content_h).max(12.0));

                    let like_active = current_rating == Some(oauth::VideoRating::Like);
                    ui.vertical_centered(|ui| {
                        let resp = paint_rail_icon_circle(
                            ui,
                            RailCircleStyle {
                                size: 48.0,
                                fill: rail_circle_fill,
                                hover_fill: rail_circle_hover,
                                active_fill: like_active.then_some(Color32::from_rgb(255, 0, 51)),
                                enabled: !rating_pending && video_id.is_some(),
                            },
                            if pt { "Gostei" } else { "Like" },
                            |painter, center| {
                                paint_shorts_heart_icon(
                                    painter,
                                    center,
                                    if like_active {
                                        Color32::WHITE
                                    } else {
                                        rail_icon_fg
                                    },
                                    like_active,
                                );
                            },
                        );
                        if resp.clicked() {
                            actions.rating_to_apply = Some(if like_active {
                                None
                            } else {
                                Some(oauth::VideoRating::Like)
                            });
                        }
                        ui.label(
                            RichText::new(&like_caption)
                                .size(12.0)
                                .color(palette.channel_fg),
                        );
                    });

                    let comments_active = side_panel == ShortsSidePanel::Comments;
                    ui.vertical_centered(|ui| {
                        let resp = paint_rail_icon_circle(
                            ui,
                            RailCircleStyle {
                                size: 48.0,
                                fill: rail_circle_fill,
                                hover_fill: rail_circle_hover,
                                active_fill: comments_active.then_some(palette.active_button_bg),
                                enabled: true,
                            },
                            if pt { "Coment\u{00e1}rios" } else { "Comments" },
                            |painter, center| {
                                paint_shorts_comment_icon(
                                    painter,
                                    center,
                                    if comments_active {
                                        palette.active_button_fg
                                    } else {
                                        rail_icon_fg
                                    },
                                );
                            },
                        );
                        if resp.clicked() {
                            actions.toggle_comments = true;
                        }
                        ui.label(
                            RichText::new(&comment_caption)
                                .size(12.0)
                                .color(palette.channel_fg),
                        );
                    });

                    ui.vertical_centered(|ui| {
                        let resp = paint_rail_icon_circle(
                            ui,
                            RailCircleStyle {
                                size: 48.0,
                                fill: rail_circle_fill,
                                hover_fill: rail_circle_hover,
                                active_fill: share_recently_copied
                                    .then_some(palette.active_button_bg),
                                enabled: true,
                            },
                            if pt { "Compartilhar" } else { "Share" },
                            |painter, center| {
                                paint_shorts_share_icon(
                                    painter,
                                    center,
                                    if share_recently_copied {
                                        palette.active_button_fg
                                    } else {
                                        rail_icon_fg
                                    },
                                );
                            },
                        );
                        if resp.clicked() {
                            actions.share_video = true;
                        }
                        ui.label(
                            RichText::new(if share_recently_copied {
                                "\u{2713}"
                            } else if pt {
                                "Compartilhar"
                            } else {
                                "Share"
                            })
                            .size(11.5)
                            .color(palette.channel_fg),
                        );
                    });

                    // Rounded square channel thumbnail matching YouTube Shorts rail.
                    ui.vertical_centered(|ui| {
                        let ch_size = egui::vec2(40.0, 40.0);
                        let ch_resp = if let Some(av_url) = channel_avatar_url.as_deref() {
                            self.thumbnails
                                .show_cover_rounded(ui, ctx, av_url, ch_size, 8.0)
                        } else {
                            let (r, resp) = ui.allocate_exact_size(ch_size, egui::Sense::click());
                            ui.painter().rect_filled(r, 8.0, rail_circle_fill);
                            let initial = avatar_fallback
                                .chars()
                                .next()
                                .unwrap_or('C')
                                .to_uppercase()
                                .to_string();
                            ui.painter().text(
                                r.center(),
                                egui::Align2::CENTER_CENTER,
                                initial,
                                egui::FontId::proportional(16.0),
                                rail_icon_fg,
                            );
                            resp
                        };
                        ui.painter().rect_stroke(
                            ch_resp.rect,
                            8.0,
                            egui::Stroke::new(1.0, palette.card_alt_stroke),
                            egui::StrokeKind::Inside,
                        );
                        paint_focus_ring_if_focused(ui, &ch_resp, 8.0);
                        if ch_resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(avatar_fallback)
                            .clicked()
                        {
                            actions.open_channel_target = channel_target.clone();
                        }
                    });
                });

                // Side-by-side Description or Comments panel when viewport fits.
                if side_panel != ShortsSidePanel::None && panel_w > 0.0 {
                    ui.add_space(panel_gap);
                    let drawer_col = ui.vertical(|ui| {
                        let inner_w = (panel_w - 36.0).max(260.0);
                        ui.set_width(panel_w);
                        ui.set_max_width(panel_w);
                        egui::Frame::new()
                            .fill(palette.card_bg)
                            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                            .corner_radius(16)
                            .inner_margin(egui::Margin::symmetric(18, 16))
                            .show(ui, |ui| {
                                ui.set_width(inner_w);
                                ui.set_max_width(inner_w);
                                ui.set_height((stage_h - 32.0).max(340.0));
                                match side_panel {
                                    ShortsSidePanel::Description => {
                                        self.render_shorts_description_panel(
                                            ui,
                                            ctx,
                                            &video,
                                            ShortDescriptionMeta {
                                                channel_target: channel_target.as_ref(),
                                                channel_avatar_url: channel_avatar_url.as_deref(),
                                                channel_verified,
                                                like_stat_value: &like_stat_value,
                                                views_stat_value: &views_stat_value,
                                                date_primary: &date_primary,
                                                date_secondary: &date_secondary,
                                            },
                                            &palette,
                                            &mut actions,
                                        );
                                    }
                                    ShortsSidePanel::Comments => {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(if pt {
                                                    "Coment\u{00e1}rios"
                                                } else {
                                                    "Comments"
                                                })
                                                .size(16.0)
                                                .strong()
                                                .color(palette.channel_fg),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    let (close_rect, close_resp) = ui
                                                        .allocate_exact_size(
                                                            egui::vec2(30.0, 30.0),
                                                            egui::Sense::click(),
                                                        );
                                                    if close_resp.hovered() {
                                                        ui.painter().circle_filled(
                                                            close_rect.center(),
                                                            15.0,
                                                            palette.header_button_bg,
                                                        );
                                                    }
                                                    paint_close_x_icon(
                                                        ui,
                                                        close_rect,
                                                        palette.header_button_fg,
                                                    );
                                                    if close_resp
                                                        .on_hover_cursor(
                                                            egui::CursorIcon::PointingHand,
                                                        )
                                                        .clicked()
                                                    {
                                                        actions.toggle_comments = true;
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(8.0);
                                        Self::paint_hairline_divider(ui, &palette);
                                        ui.add_space(6.0);
                                        self.comments_ui(ui, ctx, "shorts_comments");
                                    }
                                    ShortsSidePanel::None => {}
                                }
                            })
                    });
                    drawer_rect = drawer_col.inner.response.rect;
                }
            });

            // Stacked fallback for Description / Comments panel on narrow viewports.
            if side_panel != ShortsSidePanel::None && panel_w == 0.0 {
                ui.add_space(12.0);
                let drawer_frame = egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(16)
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.set_max_height(380.0);
                        match side_panel {
                            ShortsSidePanel::Description => {
                                self.render_shorts_description_panel(
                                    ui,
                                    ctx,
                                    &video,
                                    ShortDescriptionMeta {
                                        channel_target: channel_target.as_ref(),
                                        channel_avatar_url: channel_avatar_url.as_deref(),
                                        channel_verified,
                                        like_stat_value: &like_stat_value,
                                        views_stat_value: &views_stat_value,
                                        date_primary: &date_primary,
                                        date_secondary: &date_secondary,
                                    },
                                    &palette,
                                    &mut actions,
                                );
                            }
                            ShortsSidePanel::Comments => {
                                self.comments_ui(ui, ctx, "shorts_comments");
                            }
                            ShortsSidePanel::None => {}
                        }
                    });
                drawer_rect = drawer_frame.response.rect;
            }

            if save_menu_open {
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(340.0);
                        self.save_menu_ui(ui, &video, ctx);
                    });
            }

            if is_actionable_inline_message(&self.message) {
                ui.add_space(8.0);
                if inline_error_banner_ui(ui, &self.message, &palette) {
                    self.message.clear();
                }
            }
        });
        let stage_outer_rect = stage_resp.response.rect;

        // Paint headless placeholder top-left circles & red progress track if libmpv is inactive.
        if self.player.is_none() && video_rect.width() > 80.0 && video_rect.height() > 80.0 {
            let p = ui.painter().with_clip_rect(video_rect);
            let play_center = egui::pos2(video_rect.left() + 32.0, video_rect.top() + 32.0);
            p.circle_filled(
                play_center,
                20.0,
                Color32::from_rgba_unmultiplied(15, 15, 15, 150),
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(play_center.x - 4.5, play_center.y - 6.5),
                    egui::pos2(play_center.x - 4.5, play_center.y + 6.5),
                    egui::pos2(play_center.x + 6.5, play_center.y),
                ],
                Color32::WHITE,
                egui::Stroke::NONE,
            ));
            let vol_center = egui::pos2(video_rect.left() + 80.0, video_rect.top() + 32.0);
            p.circle_filled(
                vol_center,
                20.0,
                Color32::from_rgba_unmultiplied(15, 15, 15, 150),
            );
            let speaker = vec![
                egui::pos2(vol_center.x - 7.5, vol_center.y - 2.5),
                egui::pos2(vol_center.x - 4.0, vol_center.y - 2.5),
                egui::pos2(vol_center.x + 0.5, vol_center.y - 6.5),
                egui::pos2(vol_center.x + 0.5, vol_center.y + 6.5),
                egui::pos2(vol_center.x - 4.0, vol_center.y + 2.5),
                egui::pos2(vol_center.x - 7.5, vol_center.y + 2.5),
            ];
            p.add(egui::Shape::convex_polygon(
                speaker,
                Color32::WHITE,
                egui::Stroke::NONE,
            ));
            let track_rect = egui::Rect::from_min_max(
                egui::pos2(video_rect.left() + 4.0, video_rect.bottom() - 4.5),
                egui::pos2(video_rect.right() - 4.0, video_rect.bottom() - 1.5),
            );
            p.rect_filled(
                track_rect,
                1.5,
                Color32::from_rgba_unmultiplied(255, 255, 255, 72),
            );
        }

        // Top-right ⋮ menu button inside the 9:16 stage.
        if video_rect.width() > 80.0 && video_rect.height() > 80.0 {
            let more_rect = egui::Rect::from_min_size(
                egui::pos2(video_rect.right() - 52.0, video_rect.top() + 12.0),
                egui::vec2(40.0, 40.0),
            );
            block_rects.push(more_rect);
            let more_resp = ui
                .scope_builder(egui::UiBuilder::new().max_rect(more_rect), |ui| {
                    paint_rail_icon_circle(
                        ui,
                        RailCircleStyle {
                            size: 40.0,
                            fill: Color32::from_rgba_unmultiplied(15, 15, 15, 150),
                            hover_fill: Color32::from_rgba_unmultiplied(15, 15, 15, 210),
                            active_fill: None,
                            enabled: true,
                        },
                        if pt {
                            "Mais op\u{00e7}\u{00f5}es"
                        } else {
                            "More options"
                        },
                        |painter, center| {
                            paint_shorts_more_dots_icon(painter, center, Color32::WHITE);
                        },
                    )
                })
                .inner;
            let dislike_active = current_rating == Some(oauth::VideoRating::Dislike);
            egui::Popup::menu(&more_resp).show(|ui| {
                ui.set_min_width(196.0);
                if styled_menu_item_button(
                    ui,
                    if pt {
                        "Descri\u{00e7}\u{00e3}o"
                    } else {
                        "Description"
                    },
                    side_panel == ShortsSidePanel::Description,
                    &palette,
                )
                .clicked()
                {
                    actions.toggle_description = true;
                    ui.close();
                }
                if styled_menu_item_button(
                    ui,
                    if loop_video {
                        if pt {
                            "\u{2713} Repetir Short"
                        } else {
                            "\u{2713} Loop Short"
                        }
                    } else if pt {
                        "Repetir Short"
                    } else {
                        "Loop Short"
                    },
                    loop_video,
                    &palette,
                )
                .clicked()
                {
                    actions.toggle_loop = true;
                    ui.close();
                }
                if self.player.is_some()
                    && styled_menu_item_button(
                        ui,
                        if pt {
                            "Configura\u{00e7}\u{00f5}es do player"
                        } else {
                            "Player settings"
                        },
                        self.playback.player_menu.is_some(),
                        &palette,
                    )
                    .clicked()
                {
                    toggle_player_settings = true;
                    ui.close();
                }
                if styled_menu_item_button(
                    ui,
                    if dislike_active {
                        if pt {
                            "\u{2713} N\u{00e3}o gostei"
                        } else {
                            "\u{2713} Disliked"
                        }
                    } else if pt {
                        "N\u{00e3}o gostei"
                    } else {
                        "Dislike"
                    },
                    dislike_active,
                    &palette,
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                {
                    if !rating_pending && video_id.is_some() {
                        actions.rating_to_apply = Some(if dislike_active {
                            None
                        } else {
                            Some(oauth::VideoRating::Dislike)
                        });
                    }
                    ui.close();
                }
                if styled_menu_item_button(
                    ui,
                    if saved_in_library {
                        if pt {
                            "\u{2713} Salvo"
                        } else {
                            "\u{2713} Saved"
                        }
                    } else if pt {
                        "Salvar na biblioteca"
                    } else {
                        "Save to library"
                    },
                    saved_in_library,
                    &palette,
                )
                .clicked()
                {
                    actions.toggle_save_menu = true;
                    ui.close();
                }
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
                    actions.enqueue_video = true;
                    ui.close();
                }
                ui.separator();
                if styled_menu_item_button(
                    ui,
                    if pt { "Denunciar" } else { "Report" },
                    false,
                    &palette,
                )
                .clicked()
                {
                    open_report_modal = true;
                    ui.close();
                }
            });
        }

        // Bottom-left metadata: placed outside left of stage when no side panel is open and wide enough,
        // otherwise overlaid at bottom-left inside the 9:16 card above a bottom scrim.
        if video_rect.width() > 80.0 && video_rect.height() > 140.0 {
            let meta_w = if metadata_outside_left {
                (stage_left_offset - 28.0).clamp(210.0, 320.0)
            } else {
                (video_rect.width() - 28.0).max(120.0)
            };
            let title_galley = layout_two_line_clamped_galley(
                ui,
                &video.title,
                meta_w,
                egui::FontId::proportional(14.5),
                Color32::WHITE,
            );
            let meta_h = (34.0 + 8.0 + title_galley.size().y.max(18.0) + 6.0).min(stage_h * 0.45);
            let meta_rect = if metadata_outside_left {
                let right = (video_rect.left() - 20.0).max(viewport_rect.left() + meta_w);
                egui::Rect::from_min_max(
                    egui::pos2(
                        right - meta_w,
                        (video_rect.bottom() - meta_h - 6.0).max(video_rect.top()),
                    ),
                    egui::pos2(right, video_rect.bottom() - 6.0),
                )
            } else {
                let scrim_top = (video_rect.bottom() - 124.0).max(video_rect.top() + 56.0);
                let scrim_painter = ui.painter().with_clip_rect(video_rect);
                let bands = 6_usize;
                let scrim_h = video_rect.bottom() - 5.0 - scrim_top;
                if scrim_h > 4.0 {
                    let band_h = scrim_h / bands as f32;
                    for i in 0..bands {
                        let t = (i + 1) as f32 / bands as f32;
                        let alpha = (t.powf(1.3) * 165.0).round() as u8;
                        scrim_painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(video_rect.left(), scrim_top + i as f32 * band_h),
                                egui::pos2(video_rect.right(), scrim_top + (i + 1) as f32 * band_h),
                            ),
                            0.0,
                            Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
                        );
                    }
                }
                let r = egui::Rect::from_min_max(
                    egui::pos2(
                        video_rect.left() + 14.0,
                        (video_rect.bottom() - meta_h - 12.0).max(video_rect.top() + 56.0),
                    ),
                    egui::pos2(
                        video_rect.left() + 14.0 + meta_w,
                        video_rect.bottom() - 12.0,
                    ),
                );
                block_rects.push(r);
                r
            };

            ui.scope_builder(egui::UiBuilder::new().max_rect(meta_rect), |ui| {
                ui.vertical(|ui| {
                    ui.set_width(meta_rect.width());
                    self.render_short_metadata_block(
                        ui,
                        ctx,
                        &video,
                        ShortMetadataDisplay {
                            handle_label: &handle_label,
                            avatar_fallback,
                            channel_avatar_url: channel_avatar_url.as_deref(),
                            channel_target: channel_target.as_ref(),
                            subscribed,
                            sub_pending,
                            in_video: !metadata_outside_left,
                        },
                        &palette,
                        &mut actions,
                    );
                });
            });
        }

        // Far-right vertically centered circular navigation arrows (↑ / counter / ↓).
        if video_rect.height() > 120.0 {
            let nav_w = 54.0_f32;
            let nav_h = 128.0_f32;
            let nav_right = (viewport_rect.right() - 12.0).max(nav_w + 8.0);
            let nav_rect = egui::Rect::from_min_size(
                egui::pos2(
                    nav_right - nav_w,
                    (video_rect.center().y - nav_h * 0.5).max(viewport_rect.top()),
                ),
                egui::vec2(nav_w, nav_h),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(nav_rect), |ui| {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    if paint_rail_icon_circle(
                        ui,
                        RailCircleStyle {
                            size: 46.0,
                            fill: rail_circle_fill,
                            hover_fill: rail_circle_hover,
                            active_fill: None,
                            enabled: prev_short_available,
                        },
                        if pt {
                            "Short anterior (\u{2191} / K / Roda)"
                        } else {
                            "Previous Short (\u{2191} / K / Wheel)"
                        },
                        |painter, center| {
                            paint_shorts_arrow_icon(painter, center, rail_icon_fg, true);
                        },
                    )
                    .clicked()
                    {
                        actions.prev_short = true;
                    }
                    if let Some((idx, total)) = short_index_and_total {
                        ui.label(
                            RichText::new(format!("{idx}/{total}"))
                                .size(11.0)
                                .color(palette.neutral_status_fg),
                        );
                    }
                    if paint_rail_icon_circle(
                        ui,
                        RailCircleStyle {
                            size: 46.0,
                            fill: rail_circle_fill,
                            hover_fill: rail_circle_hover,
                            active_fill: None,
                            enabled: has_next_short,
                        },
                        if pt {
                            "Pr\u{00f3}ximo Short (\u{2193} / J / Roda)"
                        } else {
                            "Next Short (\u{2193} / J / Wheel)"
                        },
                        |painter, center| {
                            paint_shorts_arrow_icon(painter, center, rail_icon_fg, false);
                        },
                    )
                    .clicked()
                    {
                        actions.next_short = true;
                    }
                });
            });
        }

        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new("fasttube_shorts_overlay_block_rects"),
                block_rects,
            );
        });
        self.thumbnails.end_pass(ctx);

        // Mouse-wheel snap between Shorts (debounced, ignored while hovering side panel).
        if !self.in_app_ui_pass && !self.ui.settings_open && !self.watch.extras.report_modal_open {
            let (pointer_pos, wheel_y) =
                ctx.input(|i| (i.pointer.hover_pos(), i.smooth_scroll_delta.y));
            let over_drawer = side_panel != ShortsSidePanel::None
                && pointer_pos.is_some_and(|p| drawer_rect.contains(p));
            let over_stage = pointer_pos.is_none_or(|p| stage_outer_rect.expand(120.0).contains(p))
                && !over_drawer;
            if over_stage && wheel_y.abs() >= 0.5 {
                let snap_id = egui::Id::new("fasttube_shorts_wheel_snap");
                let mut snap_state = ctx
                    .data(|d| d.get_temp::<ShortsWheelSnapState>(snap_id))
                    .unwrap_or_default();
                match snap_state.feed_wheel_delta(wheel_y, Instant::now()) {
                    ShortsWheelSnapDirection::Next if has_next_short => actions.next_short = true,
                    ShortsWheelSnapDirection::Previous if prev_short_available => {
                        actions.prev_short = true;
                    }
                    _ => {}
                }
                ctx.data_mut(|d| d.insert_temp(snap_id, snap_state));
            }
        }

        if !self.in_app_ui_pass && !ctx.egui_wants_keyboard_input() {
            let (key_prev, key_next) = ctx.input(|input| {
                (
                    input.key_pressed(egui::Key::ArrowUp) || input.key_pressed(egui::Key::K),
                    input.key_pressed(egui::Key::ArrowDown) || input.key_pressed(egui::Key::J),
                )
            });
            if key_prev {
                actions.prev_short = true;
            } else if key_next {
                actions.next_short = true;
            }
        }

        if toggle_player_settings {
            self.playback.player_menu = if self.playback.player_menu.is_some() {
                None
            } else {
                Some(overlay::OverlayMenu::Root)
            };
        }
        if actions.toggle_description {
            self.watch.shorts_side_panel =
                if self.watch.shorts_side_panel == ShortsSidePanel::Description {
                    ShortsSidePanel::None
                } else {
                    ShortsSidePanel::Description
                };
            if self.watch.extras.tab == watch_extras::Tab::Comments {
                self.watch.extras.tab = watch_extras::Tab::Results;
            }
        }
        if open_report_modal {
            self.watch.extras.report_reason.clear();
            self.watch.extras.report_details.clear();
            self.watch.extras.report_modal_open = true;
        }
        if let (Some(id), Some(rating)) = (video_id, actions.rating_to_apply) {
            self.start_video_rating(id, rating, ctx);
        }
        if actions.toggle_subscribe {
            if self.auth.authenticated {
                if let Some(target) = channel_target.clone() {
                    self.start_toggle_subscription(target, channel_id_hint.clone(), ctx);
                }
            } else {
                let cid = channel_id_hint.clone().or_else(|| {
                    channel_target.as_ref().map(|t| match t {
                        ChannelTarget::ChannelId(id) | ChannelTarget::Handle(id) => id.clone(),
                    })
                });
                if let Some(cid) = cid {
                    let handle = channel_target.as_ref().and_then(|t| match t {
                        ChannelTarget::Handle(h) => Some(h.clone()),
                        ChannelTarget::ChannelId(_) => None,
                    });
                    let url = channel_target
                        .as_ref()
                        .map(ChannelTarget::canonical_url)
                        .unwrap_or_else(|| format!("https://www.youtube.com/channel/{cid}"));
                    let title = if video.channel.trim().is_empty() {
                        cid.clone()
                    } else {
                        video.channel.clone()
                    };
                    self.toggle_local_follow_channel(cid, title, handle, url, channel_avatar_url);
                }
            }
        }
        if actions.toggle_comments {
            if self.watch.shorts_side_panel == ShortsSidePanel::Comments {
                self.watch.shorts_side_panel = ShortsSidePanel::None;
                self.watch.extras.tab = watch_extras::Tab::Results;
            } else {
                self.watch.shorts_side_panel = ShortsSidePanel::Comments;
                self.watch.extras.tab = watch_extras::Tab::Comments;
                if !self.watch.extras.comments_loaded
                    && self.watch.extras.comments_loading.is_none()
                    && !self.watch.extras.innertube_comments_loading
                {
                    if let Some(cont) = self.watch.extras.innertube_comments_continuation.clone() {
                        self.start_load_innertube_comments(cont, true, ctx);
                    } else {
                        self.watch
                            .extras
                            .start_comments(self.auth.session.as_ref(), false, ctx);
                    }
                }
            }
        }
        if actions.enqueue_video {
            let _ = self.enqueue_video(video.clone());
        }
        if actions.share_video {
            self.open_share_modal(&video);
        }
        if actions.toggle_save_menu {
            self.playback.save_menu_open = !self.playback.save_menu_open;
            if self.playback.save_menu_open
                && self.auth.authenticated
                && self.auth.remote_playlists.is_empty()
            {
                self.start_fetch_remote_playlists(ctx);
            }
        }
        if actions.toggle_loop {
            self.playback.loop_video = !self.playback.loop_video;
            #[cfg(windows)]
            if let Some(player) = &self.player
                && let Err(error) = player.set(
                    "loop-file",
                    if self.playback.loop_video {
                        "inf"
                    } else {
                        "no"
                    },
                )
            {
                self.message = error.clone();
                self.toasts.error(error);
            }
            self.toasts.info(if self.playback.loop_video {
                self.pick(
                    "Repeti\u{00e7}\u{00e3}o de Short ativada.",
                    "Short loop enabled.",
                )
            } else {
                self.pick(
                    "Repeti\u{00e7}\u{00e3}o de Short desativada.",
                    "Short loop disabled.",
                )
            });
        }
        if let Some(target) = actions.open_channel_target {
            self.open_channel(target, ctx);
        }
        self.watch_report_modal_ui(ctx);
        if actions.prev_short {
            return self.next_or_prev_short(false);
        }
        if actions.next_short {
            return self.next_or_prev_short(true);
        }
        None
    }
}
