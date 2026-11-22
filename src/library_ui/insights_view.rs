use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, RichText};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalInsightsCacheKey {
    pub(crate) history_len: usize,
    pub(crate) history_first_sig: Option<(String, u64, Option<u64>, Option<u64>)>,
    pub(crate) history_last_sig: Option<(String, u64)>,
    pub(crate) library_revision: u64,
    pub(crate) library_bucket_revisions: (u64, u64, u64, u64),
    pub(crate) library_counts: (usize, usize, usize, usize),
    pub(crate) followed_len: usize,
    pub(crate) followed_first_id: Option<String>,
    pub(crate) profile_token_count: usize,
    pub(crate) feed_signal_revision: u64,
    pub(crate) feed_generation: u64,
}

#[derive(Clone)]
pub(crate) struct CachedLocalInsights {
    pub(crate) key: LocalInsightsCacheKey,
    pub(crate) insights: insights::LocalInsights,
}

pub(crate) fn resolve_local_insights_cache<F>(
    existing: Option<&Arc<CachedLocalInsights>>,
    key: LocalInsightsCacheKey,
    build: F,
) -> Arc<CachedLocalInsights>
where
    F: FnOnce() -> insights::LocalInsights,
{
    if let Some(cached) = existing
        && cached.key == key
    {
        return Arc::clone(cached);
    }
    Arc::new(CachedLocalInsights {
        key,
        insights: build(),
    })
}

impl FastTube {
    fn local_insights_cache_key(&self) -> LocalInsightsCacheKey {
        let entries = self.history.data.entries();
        LocalInsightsCacheKey {
            history_len: entries.len(),
            history_first_sig: entries.first().map(|e| {
                (
                    e.video_id.clone(),
                    e.position_ms,
                    e.duration_ms,
                    e.watched_at_ms,
                )
            }),
            history_last_sig: entries.last().map(|e| (e.video_id.clone(), e.position_ms)),
            library_revision: self.library.revision,
            library_bucket_revisions: (
                self.library.data.favorites_revision(),
                self.library.data.watch_later_revision(),
                self.library.data.collections_revision(),
                self.library.data.bookmarks_revision(),
            ),
            library_counts: (
                self.library.data.favorites.len(),
                self.library.data.watch_later.len(),
                self.library.data.collections.len(),
                self.library.data.bookmarks.len(),
            ),
            followed_len: self.followed_channels.data.len(),
            followed_first_id: self
                .followed_channels
                .data
                .channels
                .first()
                .map(|c| c.channel_id.clone()),
            profile_token_count: self.feed.cache.profile.token_weights.len(),
            feed_signal_revision: self.feed.signal_revision,
            feed_generation: self.feed.generation,
        }
    }

    pub(crate) fn insights_section_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.ensure_feed_cache();
        let palette = self.palette();
        let pt = self.ui_pt();
        let now_unix = (history::current_unix_epoch_ms() / 1000) as i64;
        let tf_id = egui::Id::new("fasttube_insights_timeframe");
        let default_tf = {
            let entries = self.history.data.entries();
            let cutoff_30d_ms = (now_unix.saturating_sub(30 * 86_400).max(0) as u64) * 1000;
            if entries.is_empty()
                || entries
                    .iter()
                    .any(|e| e.watched_at_ms.is_none_or(|ms| ms >= cutoff_30d_ms))
            {
                insights::InsightsTimeframe::Days30
            } else {
                insights::InsightsTimeframe::AllTime
            }
        };
        let mut timeframe = ctx.data(|d| {
            d.get_temp::<insights::InsightsTimeframe>(tf_id)
                .unwrap_or(default_tf)
        });

        let insights_cache_id = egui::Id::new(("fasttube_local_insights_cache", timeframe));
        let insights_key = self.local_insights_cache_key();
        let existing_insights =
            ctx.data(|d| d.get_temp::<Arc<CachedLocalInsights>>(insights_cache_id));
        let cached_insights =
            resolve_local_insights_cache(existing_insights.as_ref(), insights_key, || {
                insights::compute_with_timeframe(
                    &self.history.data,
                    &self.library.data,
                    &self.feed.cache.profile,
                    &self.followed_channels.data,
                    timeframe,
                    now_unix,
                )
            });
        if existing_insights
            .as_ref()
            .is_none_or(|prev| !Arc::ptr_eq(prev, &cached_insights))
        {
            ctx.data_mut(|d| d.insert_temp(insights_cache_id, Arc::clone(&cached_insights)));
        }
        let summary = &cached_insights.insights;

        ui.horizontal_wrapped(|ui| {
            ui.heading(if pt {
                "Insights locais de uso"
            } else {
                "Local usage insights"
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for tf in insights::InsightsTimeframe::ALL.into_iter().rev() {
                    if styled_pill_button(ui, tf.label(pt), timeframe == tf, &palette).clicked()
                        && timeframe != tf
                    {
                        timeframe = tf;
                        ctx.data_mut(|d| d.insert_temp(tf_id, tf));
                    }
                }
            });
        });
        ui.label(
            RichText::new(if pt {
                "Estatísticas calculadas 100% localmente a partir do seu histórico, biblioteca e canais seguidos."
            } else {
                "Statistics computed 100% locally from your watch history, library, and followed channels."
            })
            .color(palette.neutral_status_fg),
        );
        ui.add_space(12.0);

        let watch_human = insights::format_watch_duration_human(summary.total_watch_ms, pt);
        let avg_watch_human = insights::format_watch_duration_human(summary.average_watch_ms(), pt);
        let completion_rate_str = format!("{:.1}%", summary.completion_rate_percent());
        let stat_cards: [(&str, String); 8] = [
            (
                if pt { "Tempo assistido" } else { "Watch time" },
                watch_human,
            ),
            (
                if pt {
                    "Média por vídeo"
                } else {
                    "Average per video"
                },
                avg_watch_human,
            ),
            (
                if pt {
                    "Vídeos no histórico"
                } else {
                    "Videos in history"
                },
                summary.total_videos.to_string(),
            ),
            (
                if pt {
                    "Taxa de conclusão"
                } else {
                    "Completion rate"
                },
                completion_rate_str,
            ),
            (
                if pt { "Concluídos" } else { "Completed" },
                format!(
                    "{} ({} {})",
                    summary.completed_videos,
                    summary.in_progress_videos,
                    if pt { "em andamento" } else { "in progress" }
                ),
            ),
            (
                if pt {
                    "Canais seguidos locais"
                } else {
                    "Local followed channels"
                },
                summary.followed_channels_count.to_string(),
            ),
            (
                if pt { "Biblioteca" } else { "Library" },
                format!(
                    "{} fav · {} {} · {} {}",
                    summary.favorites_count,
                    summary.watch_later_count,
                    if pt { "depois" } else { "later" },
                    summary.collections_count,
                    if pt { "coleções" } else { "collections" }
                ),
            ),
            (
                if pt { "Marcadores" } else { "Bookmarks" },
                summary.bookmarks_count.to_string(),
            ),
        ];

        let mut drill_channel: Option<ChannelTarget> = None;
        let mut drill_video: Option<Video> = None;

        egui::ScrollArea::vertical()
            .id_salt("browse_insights_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail = ui.available_width().max(240.0);
                let cols: usize = if avail >= 860.0 {
                    4
                } else if avail >= 640.0 {
                    3
                } else if avail >= 420.0 {
                    2
                } else {
                    1
                };
                let card_w =
                    ((avail - 12.0 * (cols.saturating_sub(1) as f32)) / cols as f32).max(150.0);
                for row in stat_cards.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        for (title, value) in row {
                            egui::Frame::new()
                                .fill(palette.card_bg)
                                .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(10)
                                .inner_margin(egui::Margin::symmetric(14, 12))
                                .show(ui, |ui| {
                                    ui.set_min_width(card_w - 28.0);
                                    ui.set_max_width(card_w - 28.0);
                                    ui.label(
                                        RichText::new(*title)
                                            .small()
                                            .color(palette.neutral_status_fg),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(value)
                                            .size(18.0)
                                            .strong()
                                            .color(palette.channel_fg),
                                    );
                                });
                        }
                    });
                    ui.add_space(10.0);
                }

                if !summary.daily_buckets.is_empty() {
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(palette.card_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(10)
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new(if pt {
                                    format!("Tempo diário assistido ({})", timeframe.label(pt))
                                } else {
                                    format!("Daily watch time ({})", timeframe.label(pt))
                                })
                                .size(15.0)
                                .strong()
                                .color(palette.channel_fg),
                            );
                            ui.add_space(10.0);
                            let chart_w = ui.available_width().max(220.0);
                            let chart_h = 112.0;
                            let (chart_rect, _) = ui.allocate_exact_size(
                                egui::vec2(chart_w, chart_h),
                                egui::Sense::hover(),
                            );
                            let max_bucket_ms = summary
                                .daily_buckets
                                .iter()
                                .map(|b| b.watch_ms)
                                .max()
                                .unwrap_or(1)
                                .max(1);
                            let n = summary.daily_buckets.len().max(1);
                            let gap = if n > 14 { 3.0 } else { 8.0 };
                            let bar_w = ((chart_w - gap * (n.saturating_sub(1) as f32)) / n as f32)
                                .max(4.0);
                            let bar_area_h = chart_h - 22.0;
                            for (i, bucket) in summary.daily_buckets.iter().enumerate() {
                                let x = chart_rect.left() + i as f32 * (bar_w + gap);
                                let frac = if bucket.watch_ms == 0 {
                                    0.04
                                } else {
                                    (bucket.watch_ms as f32 / max_bucket_ms as f32).clamp(0.08, 1.0)
                                };
                                let bh = (bar_area_h * frac).max(3.0);
                                let bar_rect = egui::Rect::from_min_max(
                                    egui::pos2(x, chart_rect.top() + bar_area_h - bh),
                                    egui::pos2(x + bar_w, chart_rect.top() + bar_area_h),
                                );
                                let color = if bucket.watch_ms > 0 {
                                    palette.info_accent_fg
                                } else {
                                    palette.card_alt_stroke
                                };
                                ui.painter().rect_filled(bar_rect, 3.0, color);
                                let bar_resp = ui.interact(
                                    bar_rect,
                                    ui.make_persistent_id(("insights_bar", i)),
                                    egui::Sense::hover(),
                                );
                                if bar_resp.hovered() {
                                    let dur_lbl =
                                        insights::format_watch_duration_human(bucket.watch_ms, pt);
                                    bar_resp.on_hover_text(format!(
                                        "{}: {} ({} {})",
                                        bucket.date_label,
                                        dur_lbl,
                                        bucket.videos_count,
                                        if pt { "vídeos" } else { "videos" }
                                    ));
                                }
                                let show_label = n <= 10 || i == 0 || i + 1 == n || i % 5 == 0;
                                if show_label {
                                    ui.painter().text(
                                        egui::pos2(bar_rect.center().x, chart_rect.bottom() - 2.0),
                                        egui::Align2::CENTER_BOTTOM,
                                        &bucket.date_label,
                                        egui::FontId::proportional(10.0),
                                        palette.neutral_status_fg,
                                    );
                                }
                            }
                        });
                }

                ui.add_space(8.0);
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            RichText::new(if pt {
                                "Top canais mais assistidos"
                            } else {
                                "Top watched channels"
                            })
                            .size(15.0)
                            .strong()
                            .color(palette.channel_fg),
                        );
                        ui.add_space(8.0);
                        if summary.top_channels.is_empty() {
                            ui.label(
                                RichText::new(if pt {
                                    "Assista a alguns vídeos para ver o ranking dos seus canais."
                                } else {
                                    "Watch a few videos to see your top channels ranking."
                                })
                                .small()
                                .color(palette.neutral_status_fg),
                            );
                        } else {
                            let max_ms = summary
                                .top_channels
                                .iter()
                                .map(|c| c.watch_ms)
                                .max()
                                .unwrap_or(1)
                                .max(1);
                            for (idx, stat) in summary.top_channels.iter().enumerate() {
                                let frac = (stat.watch_ms as f32 / max_ms as f32).clamp(0.05, 1.0);
                                let dur = insights::format_watch_duration_human(stat.watch_ms, pt);
                                let chan_id = summary.top_channel_ids.get(idx).cloned();
                                ui.horizontal(|ui| {
                                    let btn = ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&stat.channel)
                                                    .strong()
                                                    .color(palette.channel_fg),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                                    if btn.clicked() {
                                        let target = match chan_id
                                            .as_deref()
                                            .filter(|id| id.starts_with("UC") && id.len() >= 20)
                                        {
                                            Some(uc) => ChannelTarget::ChannelId(uc.to_owned()),
                                            None => {
                                                let handle = if stat.channel.starts_with('@') {
                                                    stat.channel.clone()
                                                } else {
                                                    format!("@{}", stat.channel.replace(' ', ""))
                                                };
                                                ChannelTarget::Handle(handle)
                                            }
                                        };
                                        drill_channel = Some(target);
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{dur} · {} {}",
                                                    stat.videos,
                                                    if pt { "vídeos" } else { "videos" }
                                                ))
                                                .small()
                                                .color(palette.neutral_status_fg),
                                            );
                                        },
                                    );
                                });
                                ui.add(
                                    egui::ProgressBar::new(frac)
                                        .desired_width(ui.available_width())
                                        .desired_height(6.0),
                                );
                                ui.add_space(6.0);
                            }
                        }
                    });

                if !summary.top_videos.is_empty() {
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(palette.card_bg)
                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                        .corner_radius(10)
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new(if pt {
                                    "Top vídeos mais assistidos"
                                } else {
                                    "Top watched videos"
                                })
                                .size(15.0)
                                .strong()
                                .color(palette.channel_fg),
                            );
                            ui.add_space(8.0);
                            for vstat in &summary.top_videos {
                                let dur = insights::format_watch_duration_human(vstat.watch_ms, pt);
                                ui.horizontal(|ui| {
                                    let btn = ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&vstat.title)
                                                    .strong()
                                                    .color(palette.channel_fg),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(&vstat.url);
                                    if btn.clicked() {
                                        drill_video = Some(Video {
                                            title: vstat.title.clone(),
                                            channel: vstat.channel.clone(),
                                            url: vstat.url.clone(),
                                            thumbnail: format!(
                                                "https://i.ytimg.com/vi/{}/hqdefault.jpg",
                                                vstat.video_id
                                            ),
                                            duration: String::new(),
                                        });
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(format!("{} · {dur}", vstat.channel))
                                                    .small()
                                                    .color(palette.neutral_status_fg),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(4.0);
                            }
                        });
                }

                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(
                            RichText::new(if pt {
                                "Tópicos com maior afinidade local"
                            } else {
                                "Top local interest topics"
                            })
                            .size(15.0)
                            .strong()
                            .color(palette.channel_fg),
                        );
                        ui.add_space(8.0);
                        if summary.top_topics.is_empty() {
                            ui.label(
                                RichText::new(if pt {
                                    "Nenhum tópico aprendido no perfil local ainda."
                                } else {
                                    "No learned topics in local profile yet."
                                })
                                .small()
                                .color(palette.neutral_status_fg),
                            );
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                for (topic, score) in &summary.top_topics {
                                    egui::Frame::new()
                                        .fill(palette.card_alt_bg)
                                        .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
                                        .corner_radius(14)
                                        .inner_margin(egui::Margin::symmetric(10, 4))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(format!("{topic} ({score:.1})"))
                                                    .small()
                                                    .color(palette.channel_fg),
                                            );
                                        });
                                }
                            });
                        }
                    });
            });

        if let Some(target) = drill_channel {
            self.open_channel(target, ctx);
        }
        if let Some(video) = drill_video {
            self.playback.pending_open_video = Some(video);
        }
    }
}
