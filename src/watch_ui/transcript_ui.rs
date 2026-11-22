use super::*;
use crate::browse_ui::compute_variable_visible_range;
use eframe::egui::RichText;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TranscriptSearchCacheKey {
    pub(crate) video_url: Option<String>,
    pub(crate) transcript_revision: u64,
    pub(crate) cue_count: usize,
    pub(crate) filter_query: String,
    pub(crate) language_track_url: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CachedTranscriptSearch {
    pub(crate) key: TranscriptSearchCacheKey,
    pub(crate) matched_indices: Vec<usize>,
    pub(crate) match_ranges: Vec<Vec<(usize, usize)>>,
}

pub(crate) fn resolve_transcript_search_cache(
    existing: Option<&Arc<CachedTranscriptSearch>>,
    key: TranscriptSearchCacheKey,
    segments: &[transcript::Cue],
) -> Arc<CachedTranscriptSearch> {
    if let Some(cached) = existing
        && cached.key == key
    {
        return Arc::clone(cached);
    }
    let query = key.filter_query.trim();
    if query.is_empty() {
        return Arc::new(CachedTranscriptSearch {
            key,
            matched_indices: Vec::new(),
            match_ranges: Vec::new(),
        });
    }
    let matched_indices = transcript::search_segment_indices(segments, query);
    let match_ranges = matched_indices
        .iter()
        .map(|&idx| {
            segments
                .get(idx)
                .map(|cue| transcript::search_match_ranges(&cue.text, query))
                .unwrap_or_default()
        })
        .collect();
    Arc::new(CachedTranscriptSearch {
        key,
        matched_indices,
        match_ranges,
    })
}

pub(super) fn estimate_transcript_segment_height(
    cue: &transcript::Cue,
    row_width: f32,
    show_timestamps: bool,
    font_size: f32,
) -> f32 {
    let wrap_w = (row_width - 12.0).max(80.0);
    let char_w = (font_size * 0.54).max(6.5);
    let leading_chars = if show_timestamps { 9.0 } else { 0.0 };
    let chars_per_line = (wrap_w / char_w).floor().max(12.0);
    let total_chars = cue.text.chars().count() as f32 + leading_chars;
    let lines = (total_chars / chars_per_line).ceil().max(1.0);
    let line_h = (font_size * 1.42).round().max(19.0);
    let min_h = if show_timestamps { 32.0 } else { 26.0 };
    (lines * line_h + 12.0).max(min_h)
}

fn estimate_transcript_item_height(
    item: &transcript::TranscriptDisplayItem,
    segments: &[transcript::Cue],
    row_width: f32,
    show_timestamps: bool,
    font_size: f32,
    is_first: bool,
) -> f32 {
    match item {
        transcript::TranscriptDisplayItem::Chapter { title, .. } => {
            let wrap_w = (row_width - 12.0).max(80.0);
            let chars_per_line = (wrap_w / 8.5).floor().max(10.0);
            let lines = ((title.chars().count() as f32) / chars_per_line)
                .ceil()
                .max(1.0);
            let top_pad = if is_first { 2.0 } else { 12.0 };
            top_pad + lines * 21.0 + 6.0
        }
        transcript::TranscriptDisplayItem::Segment { segment_index } => segments
            .get(*segment_index)
            .map(|cue| {
                estimate_transcript_segment_height(cue, row_width, show_timestamps, font_size)
            })
            .unwrap_or(32.0),
    }
}

impl FastTube {
    pub(crate) fn collect_transcript_chapters(&self) -> Vec<transcript::TranscriptChapter> {
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str());
        let mut out: Vec<transcript::TranscriptChapter> =
            if !self.watch.metadata_cache.description_chapters.is_empty() {
                self.watch
                    .metadata_cache
                    .description_chapters
                    .iter()
                    .map(|ch| transcript::TranscriptChapter {
                        title: ch.title.clone(),
                        start_ms: ch.seconds.saturating_mul(1000),
                    })
                    .collect()
            } else if let Some(info) = self.watch.live.info.as_ref().filter(|_| {
                selected_url.is_some() && self.watch.live.video_url.as_deref() == selected_url
            }) && !info.chapters.is_empty()
            {
                info.chapters
                    .iter()
                    .filter(|ch| ch.time_secs.is_finite() && ch.time_secs >= 0.0)
                    .map(|ch| transcript::TranscriptChapter {
                        title: ch.title.clone(),
                        start_ms: (ch.time_secs * 1000.0).round() as u64,
                    })
                    .collect()
            } else if selected_url.is_some()
                && self.watch.overlay_cache.chapters_video_url.as_deref() == selected_url
                && !self.watch.overlay_cache.chapters.is_empty()
            {
                self.watch
                    .overlay_cache
                    .chapters
                    .iter()
                    .filter(|ch| ch.time_secs.is_finite() && ch.time_secs >= 0.0)
                    .map(|ch| transcript::TranscriptChapter {
                        title: ch.title.clone(),
                        start_ms: (ch.time_secs * 1000.0).round() as u64,
                    })
                    .collect()
            } else {
                Vec::new()
            };
        out.retain(|ch| !ch.title.trim().is_empty());
        out.sort_by_key(|ch| ch.start_ms);
        out.dedup_by_key(|ch| ch.start_ms);
        out
    }

    fn transcript_track_display_label(track: &search::SubtitleTrack, pt: bool) -> String {
        let lower = track.language.trim().to_ascii_lowercase();
        let base = lower.split(['-', '_']).next().unwrap_or(lower.as_str());
        let lang_name = match lower.as_str() {
            "pt-br" | "pt_br" => {
                if pt {
                    "Português (Brasil)"
                } else {
                    "Portuguese (Brazil)"
                }
            }
            "pt-pt" | "pt_pt" => {
                if pt {
                    "Português (Portugal)"
                } else {
                    "Portuguese (Portugal)"
                }
            }
            "zh-hans" | "zh-cn" | "zh_cn" => {
                if pt {
                    "Chinês (simplificado)"
                } else {
                    "Chinese (Simplified)"
                }
            }
            "zh-hant" | "zh-tw" | "zh_tw" => {
                if pt {
                    "Chinês (tradicional)"
                } else {
                    "Chinese (Traditional)"
                }
            }
            _ => match base {
                "pt" => {
                    if pt {
                        "Português"
                    } else {
                        "Portuguese"
                    }
                }
                "en" => {
                    if pt {
                        "Inglês"
                    } else {
                        "English"
                    }
                }
                "es" => {
                    if pt {
                        "Espanhol"
                    } else {
                        "Spanish"
                    }
                }
                "fr" => {
                    if pt {
                        "Francês"
                    } else {
                        "French"
                    }
                }
                "de" => {
                    if pt {
                        "Alemão"
                    } else {
                        "German"
                    }
                }
                "it" => {
                    if pt {
                        "Italiano"
                    } else {
                        "Italian"
                    }
                }
                "ja" => {
                    if pt {
                        "Japonês"
                    } else {
                        "Japanese"
                    }
                }
                "ko" => {
                    if pt {
                        "Coreano"
                    } else {
                        "Korean"
                    }
                }
                "zh" => {
                    if pt {
                        "Chinês"
                    } else {
                        "Chinese"
                    }
                }
                "ru" => {
                    if pt {
                        "Russo"
                    } else {
                        "Russian"
                    }
                }
                "ar" => {
                    if pt {
                        "Árabe"
                    } else {
                        "Arabic"
                    }
                }
                "hi" => "Hindi",
                "nl" => {
                    if pt {
                        "Holandês"
                    } else {
                        "Dutch"
                    }
                }
                "pl" => {
                    if pt {
                        "Polonês"
                    } else {
                        "Polish"
                    }
                }
                "tr" => {
                    if pt {
                        "Turco"
                    } else {
                        "Turkish"
                    }
                }
                "uk" => {
                    if pt {
                        "Ucraniano"
                    } else {
                        "Ukrainian"
                    }
                }
                "sv" => {
                    if pt {
                        "Sueco"
                    } else {
                        "Swedish"
                    }
                }
                _ => track.language.trim(),
            },
        };
        if track.automatic {
            if pt {
                format!("{lang_name} (gerada automaticamente)")
            } else {
                format!("{lang_name} (auto-generated)")
            }
        } else {
            lang_name.to_owned()
        }
    }

    pub(crate) fn open_transcript_panel(&mut self, ctx: &egui::Context) {
        self.channels.view_open = false;
        self.watch.extras.open_transcript();
        self.ensure_transcript_loaded(ctx);
    }

    pub(crate) fn ensure_transcript_loaded(&mut self, ctx: &egui::Context) {
        if !self.watch.extras.transcript.is_empty()
            || self.watch.extras.transcript_loading
            || self.watch.extras.transcript_error.is_some()
        {
            return;
        }
        if self.watch.extras.subtitles_loaded {
            if !self.watch.extras.transcript_auto_attempted {
                self.watch.extras.transcript_auto_attempted = true;
                if let Some(track) = self
                    .watch
                    .extras
                    .preferred_transcript_track(self.ui_pt())
                    .cloned()
                {
                    self.watch.extras.start_transcript(&track, ctx);
                }
            }
        } else if self.watch.extras.subtitles_loading.is_none()
            && !self.is_playback_metadata_loading()
            && !self.watch.live.watch_info_loading
        {
            self.watch.extras.start_subtitles(ctx);
        }
    }

    pub(crate) fn apply_ready_subtitle(&mut self) {
        #[cfg(windows)]
        if let Some(sub) = self.watch.extras.take_ready_subtitle() {
            self.playback.last_subtitle_id = Some(format!("yt:{}", sub.url));
            self.watch.extras.active_subtitle_url = Some(sub.url.clone());
            self.watch.extras.subtitles_error = None;
            if let Some(player) = &self.player {
                if !self.watch.extras.active_subtitle_cues.is_empty() && player.is_render_api() {
                    let _ = player.disable_subtitles();
                } else {
                    let path = sub.path.to_string_lossy().into_owned();
                    if let Err(error) = player.add_subtitle(&path, &sub.label, &sub.language) {
                        self.watch.extras.subtitles_error = Some(error);
                    }
                }
            }
        }
        #[cfg(not(windows))]
        let _ = self.watch.extras.take_ready_subtitle();
    }

    /// Computes a zero-allocation signature over the active chapter source so
    /// `sync_transcript_display_cache` skips cloning/sorting chapters on cache hits.
    fn fast_transcript_chapters_signature(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mix = |h: &mut u64, bytes: &[u8]| {
            for &b in bytes {
                *h ^= u64::from(b);
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        let selected_url = self.selected.as_ref().map(|v| v.url.as_str());
        if !self.watch.metadata_cache.description_chapters.is_empty() {
            mix(&mut hash, &[1]);
            mix(
                &mut hash,
                &self
                    .watch
                    .metadata_cache
                    .description_chapters
                    .len()
                    .to_le_bytes(),
            );
            for ch in &self.watch.metadata_cache.description_chapters {
                mix(&mut hash, &ch.seconds.to_le_bytes());
                mix(&mut hash, ch.title.as_bytes());
            }
        } else if let Some(info) = self.watch.live.info.as_ref().filter(|_| {
            selected_url.is_some() && self.watch.live.video_url.as_deref() == selected_url
        }) && !info.chapters.is_empty()
        {
            mix(&mut hash, &[2]);
            mix(&mut hash, &self.watch.live.info_revision.to_le_bytes());
            mix(&mut hash, &info.chapters.len().to_le_bytes());
            for ch in &info.chapters {
                mix(&mut hash, &ch.time_secs.to_bits().to_le_bytes());
                mix(&mut hash, ch.title.as_bytes());
            }
        } else if selected_url.is_some()
            && self.watch.overlay_cache.chapters_video_url.as_deref() == selected_url
            && !self.watch.overlay_cache.chapters.is_empty()
        {
            mix(&mut hash, &[3]);
            mix(
                &mut hash,
                &self.watch.overlay_cache.chapters.len().to_le_bytes(),
            );
            for ch in &self.watch.overlay_cache.chapters {
                mix(&mut hash, &ch.time_secs.to_bits().to_le_bytes());
                mix(&mut hash, ch.title.as_bytes());
            }
        } else {
            mix(&mut hash, &[0]);
        }
        hash
    }

    pub(crate) fn sync_transcript_display_cache(&mut self) {
        let chapters_sig = self.fast_transcript_chapters_signature();
        let raw_len = self.watch.extras.transcript.len();
        let first_cue_sig = self
            .watch
            .extras
            .transcript
            .first()
            .map(|c| (c.start_ms, c.end_ms, c.text.len()));
        let last_cue_sig = self
            .watch
            .extras
            .transcript
            .last()
            .map(|c| (c.start_ms, c.end_ms, c.text.len()));
        let revision = self.watch.extras.transcript_revision;

        let cache = &self.watch.extras.transcript_display_cache;
        let up_to_date = cache.valid
            && cache.revision == revision
            && cache.raw_len == raw_len
            && cache.first_cue_sig == first_cue_sig
            && cache.last_cue_sig == last_cue_sig
            && cache.chapters_sig == chapters_sig;
        if up_to_date {
            return;
        }

        let chapters = self.collect_transcript_chapters();
        let chapter_starts: Vec<u64> = chapters.iter().map(|ch| ch.start_ms).collect();
        let segments =
            transcript::group_cues_for_transcript(&self.watch.extras.transcript, &chapter_starts);
        let items = transcript::build_display_items(&segments, &chapters);
        let item_count = items.len();
        self.watch.extras.transcript_display_cache = watch_extras::TranscriptDisplayCache {
            valid: true,
            revision,
            raw_len,
            first_cue_sig,
            last_cue_sig,
            chapters_sig,
            segments,
            items,
            item_heights: vec![None; item_count],
        };
    }

    fn paint_transcript_icon_button(
        ui: &mut egui::Ui,
        is_close: bool,
        dark_theme: bool,
        hover_text: &str,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
        let fg = if dark_theme {
            Color32::from_rgb(241, 241, 241)
        } else {
            Color32::from_rgb(15, 15, 15)
        };
        if response.hovered() || response.has_focus() {
            let hover_fill = if dark_theme {
                Color32::from_rgba_unmultiplied(255, 255, 255, 26)
            } else {
                Color32::from_rgba_unmultiplied(0, 0, 0, 18)
            };
            ui.painter().circle_filled(rect.center(), 16.0, hover_fill);
        }
        if is_close {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "\u{2715}",
                egui::FontId::proportional(15.0),
                fg,
            );
        } else {
            let c = rect.center();
            for dy in [-5.0_f32, 0.0, 5.0] {
                ui.painter()
                    .circle_filled(egui::pos2(c.x, c.y + dy), 1.45, fg);
            }
        }
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(hover_text)
    }

    fn paint_transcript_chapter_row(
        ui: &mut egui::Ui,
        title: &str,
        row_width: f32,
        is_first_item: bool,
        dark_theme: bool,
        palette: &ThemePalette,
    ) -> (egui::Response, f32) {
        let top_pad = if is_first_item { 2.0 } else { 12.0 };
        let bottom_pad = 6.0;
        let pad_x = 6.0;
        let wrap_w = (row_width - pad_x * 2.0).max(80.0);
        let mut job = egui::text::LayoutJob {
            halign: egui::Align::LEFT,
            wrap: egui::text::TextWrapping {
                max_width: wrap_w,
                ..Default::default()
            },
            ..Default::default()
        };
        job.append(
            title,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(15.5),
                color: palette.channel_fg,
                line_height: Some(21.0),
                ..Default::default()
            },
        );
        let galley = ui.painter().layout_job(job);
        let text_h = galley.size().y.max(21.0);
        let total_h = top_pad + text_h + bottom_pad;
        let (row_rect, response) =
            ui.allocate_exact_size(egui::vec2(row_width, total_h), egui::Sense::click());
        let bg_rect = egui::Rect::from_min_max(
            egui::pos2(row_rect.left(), row_rect.top() + top_pad - 2.0),
            egui::pos2(row_rect.right(), row_rect.bottom() - 2.0),
        );
        if response.hovered() {
            let hover_bg = if dark_theme {
                Color32::from_rgba_unmultiplied(255, 255, 255, 14)
            } else {
                Color32::from_rgba_unmultiplied(0, 0, 0, 10)
            };
            ui.painter().rect_filled(bg_rect, 8.0, hover_bg);
        }
        // Paint bold chapter heading slightly reinforced for clear section hierarchy.
        let text_pos = egui::pos2(row_rect.left() + pad_x, row_rect.top() + top_pad);
        ui.painter()
            .galley(text_pos, galley.clone(), palette.channel_fg);
        ui.painter()
            .galley(text_pos + egui::vec2(0.35, 0.0), galley, palette.channel_fg);
        (
            response.on_hover_cursor(egui::CursorIcon::PointingHand),
            total_h,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_transcript_segment_row(
        ui: &mut egui::Ui,
        cue: &transcript::Cue,
        row_width: f32,
        is_active: bool,
        show_timestamps: bool,
        precomputed_matches: Option<&[(usize, usize)]>,
        font_size: f32,
        dark_theme: bool,
    ) -> (egui::Response, egui::Rect, f32) {
        let pad_x = 6.0;
        let pad_y = 6.0;
        let pill_gap = 8.0;
        let pill_h = 20.0;

        let (text_color, pill_bg, pill_fg, hover_bg, hl_bg, hl_fg) = if dark_theme {
            (
                if is_active {
                    Color32::from_rgb(245, 245, 245)
                } else {
                    Color32::from_rgb(168, 168, 168)
                },
                if is_active {
                    Color32::from_rgb(38, 56, 78)
                } else {
                    Color32::from_rgb(56, 56, 56)
                },
                if is_active {
                    Color32::from_rgb(138, 200, 255)
                } else {
                    Color32::from_rgb(185, 185, 185)
                },
                Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                Color32::from_rgba_unmultiplied(255, 214, 72, 64),
                Color32::from_rgb(255, 242, 185),
            )
        } else {
            (
                if is_active {
                    Color32::from_rgb(15, 15, 15)
                } else {
                    Color32::from_rgb(96, 96, 96)
                },
                if is_active {
                    Color32::from_rgb(222, 241, 255)
                } else {
                    Color32::from_rgb(236, 236, 236)
                },
                if is_active {
                    Color32::from_rgb(6, 95, 212)
                } else {
                    Color32::from_rgb(96, 96, 96)
                },
                Color32::from_rgba_unmultiplied(0, 0, 0, 10),
                Color32::from_rgba_unmultiplied(255, 213, 79, 130),
                Color32::from_rgb(15, 15, 15),
            )
        };

        let ts_info = show_timestamps.then(|| {
            let ts_label = transcript::format_transcript_timestamp_ms(cue.start_ms);
            let ts_galley =
                ui.painter()
                    .layout_no_wrap(ts_label, egui::FontId::proportional(11.5), pill_fg);
            let pill_w = (ts_galley.size().x + 14.0).max(38.0);
            (ts_galley, pill_w)
        });
        let leading_space = ts_info
            .as_ref()
            .map_or(0.0, |(_, pill_w)| pill_w + pill_gap);

        let wrap_w = (row_width - pad_x * 2.0).max(80.0);
        let mut job = egui::text::LayoutJob {
            halign: egui::Align::LEFT,
            wrap: egui::text::TextWrapping {
                max_width: wrap_w,
                ..Default::default()
            },
            first_row_min_height: if show_timestamps { pill_h } else { 0.0 },
            ..Default::default()
        };

        let line_height = Some((font_size * 1.42).round().max(19.0));
        let base_format = egui::TextFormat {
            font_id: egui::FontId::proportional(font_size),
            color: text_color,
            line_height,
            ..Default::default()
        };
        let match_ranges = precomputed_matches.unwrap_or(&[]);
        let has_highlights = !match_ranges.is_empty();
        if !has_highlights {
            job.append(&cue.text, leading_space, base_format);
        } else {
            let hl_format = egui::TextFormat {
                font_id: egui::FontId::proportional(font_size),
                color: hl_fg,
                background: hl_bg,
                line_height,
                ..Default::default()
            };
            let mut cursor = 0usize;
            for &(start, end) in match_ranges {
                if start > cursor
                    && let Some(plain) = cue.text.get(cursor..start)
                {
                    let ls = if job.sections.is_empty() {
                        leading_space
                    } else {
                        0.0
                    };
                    job.append(plain, ls, base_format.clone());
                }
                if let Some(matched) = cue.text.get(start..end) {
                    let ls = if job.sections.is_empty() {
                        leading_space
                    } else {
                        0.0
                    };
                    job.append(matched, ls, hl_format.clone());
                }
                cursor = end;
            }
            if cursor < cue.text.len()
                && let Some(tail) = cue.text.get(cursor..)
            {
                let ls = if job.sections.is_empty() {
                    leading_space
                } else {
                    0.0
                };
                job.append(tail, ls, base_format);
            }
        }

        let galley = ui.painter().layout_job(job);
        let text_h = galley.size().y;
        let row_h = (text_h + pad_y * 2.0).max(if show_timestamps {
            pill_h + pad_y * 2.0
        } else {
            26.0
        });
        let (row_rect, response) =
            ui.allocate_exact_size(egui::vec2(row_width, row_h), egui::Sense::click());

        if response.hovered() {
            ui.painter().rect_filled(row_rect, 8.0, hover_bg);
        }

        let text_pos = egui::pos2(row_rect.left() + pad_x, row_rect.top() + pad_y);
        if let Some((ts_galley, pill_w)) = ts_info {
            let first_row_h = galley.rows.first().map_or(pill_h, |r| r.height());
            let pill_y = text_pos.y + ((first_row_h - pill_h) * 0.5).max(0.0);
            let pill_rect = egui::Rect::from_min_size(
                egui::pos2(row_rect.left() + pad_x, pill_y),
                egui::vec2(pill_w, pill_h),
            );
            ui.painter().rect_filled(pill_rect, pill_h * 0.5, pill_bg);
            let ts_pos = egui::pos2(
                pill_rect.center().x - ts_galley.size().x * 0.5,
                pill_rect.center().y - ts_galley.size().y * 0.5,
            );
            ui.painter().galley(ts_pos, ts_galley, pill_fg);
        }

        if is_active && !has_highlights {
            ui.painter().galley(text_pos, galley.clone(), text_color);
            ui.painter()
                .galley(text_pos + egui::vec2(0.30, 0.0), galley, text_color);
        } else {
            ui.painter().galley(text_pos, galley, text_color);
        }
        (
            response.on_hover_cursor(egui::CursorIcon::PointingHand),
            row_rect,
            row_h,
        )
    }

    pub(crate) fn subtitles_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, scroll_id: &str) {
        if let Some(video) = self.selected.clone() {
            self.ensure_watch_metadata_cache(&video);
        }
        self.ensure_transcript_loaded(ctx);
        self.sync_transcript_display_cache();

        let pt = self.ui_pt();
        let dark_theme = self.is_dark_theme();
        let palette = self.palette();
        let (card_bg, card_stroke, search_bg, muted_fg) = if dark_theme {
            (
                Color32::from_rgb(26, 26, 26),
                Color32::from_rgb(48, 48, 48),
                Color32::from_rgb(45, 45, 45),
                Color32::from_rgb(168, 168, 168),
            )
        } else {
            (
                Color32::WHITE,
                Color32::from_rgb(229, 229, 229),
                Color32::from_rgb(240, 240, 240),
                Color32::from_rgb(96, 96, 96),
            )
        };

        let card_width = ui.available_width().max(220.0);
        let inner_width = (card_width - 32.0).max(160.0);
        let has_segments = !self
            .watch
            .extras
            .transcript_display_cache
            .segments
            .is_empty();
        let font_size = if self.watch.extras.transcript_font_size > 0.0 {
            self.watch.extras.transcript_font_size.clamp(13.0, 20.0)
        } else {
            14.0
        };

        let mut close_transcript = false;
        let mut copy_transcript = false;
        let mut export_transcript = false;
        let mut reload_transcript = false;
        let mut switch_track: Option<search::SubtitleTrack> = None;
        let mut seek_to_ms: Option<u64> = None;
        let mut jump_to_active = false;
        let mut user_clicked_segment = false;

        egui::Frame::new()
            .fill(card_bg)
            .stroke(egui::Stroke::new(1.0, card_stroke))
            .corner_radius(12)
            .inner_margin(egui::Margin {
                left: 16,
                right: 16,
                top: 14,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.set_width(inner_width);
                ui.set_max_width(inner_width);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if pt {
                            "Transcri\u{00e7}\u{00e3}o"
                        } else {
                            "Transcript"
                        })
                        .size(19.0)
                        .strong()
                        .color(palette.channel_fg),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if Self::paint_transcript_icon_button(
                            ui,
                            true,
                            dark_theme,
                            if pt {
                                "Fechar transcri\u{00e7}\u{00e3}o"
                            } else {
                                "Close transcript"
                            },
                        )
                        .clicked()
                        {
                            close_transcript = true;
                        }
                        let more_resp = Self::paint_transcript_icon_button(
                            ui,
                            false,
                            dark_theme,
                            if pt { "Mais a\u{00e7}\u{00f5}es" } else { "More actions" },
                        );
                        egui::Popup::menu(&more_resp).show(|ui| {
                            ui.set_min_width(235.0);
                            let ts_label = if self.watch.extras.transcript_show_timestamps {
                                if pt {
                                    "Ocultar carimbos de data/hora"
                                } else {
                                    "Hide timestamps"
                                }
                            } else if pt {
                                "Mostrar carimbos de data/hora"
                            } else {
                                "Show timestamps"
                            };
                            if styled_menu_item_button(ui, ts_label, false, &palette).clicked() {
                                self.watch.extras.transcript_show_timestamps =
                                    !self.watch.extras.transcript_show_timestamps;
                                ui.close();
                            }
                            let follow_label = if self.watch.extras.transcript_follow {
                                if pt {
                                    "Acompanhar reprodu\u{00e7}\u{00e3}o \u{2713}"
                                } else {
                                    "Sync with video \u{2713}"
                                }
                            } else if pt {
                                "Acompanhar reprodu\u{00e7}\u{00e3}o"
                            } else {
                                "Sync with video"
                            };
                            if styled_menu_item_button(
                                ui,
                                follow_label,
                                self.watch.extras.transcript_follow,
                                &palette,
                            )
                            .clicked()
                            {
                                self.watch.extras.transcript_follow =
                                    !self.watch.extras.transcript_follow;
                                if self.watch.extras.transcript_follow {
                                    self.watch.extras.transcript_user_scrolled = false;
                                    self.watch.extras.transcript_last_scrolled_segment = None;
                                }
                                ui.close();
                            }
                            ui.separator();
                            if has_segments
                                && styled_menu_item_button(
                                    ui,
                                    if pt {
                                        "Copiar transcri\u{00e7}\u{00e3}o"
                                    } else {
                                        "Copy transcript"
                                    },
                                    false,
                                    &palette,
                                )
                                .clicked()
                            {
                                copy_transcript = true;
                                ui.close();
                            }
                            if has_segments
                                && !self.watch.extras.is_exporting_transcript()
                                && styled_menu_item_button(
                                    ui,
                                    if pt {
                                        "Exportar (.txt / .md)"
                                    } else {
                                        "Export (.txt / .md)"
                                    },
                                    false,
                                    &palette,
                                )
                                .clicked()
                            {
                                export_transcript = true;
                                ui.close();
                            }
                            if styled_menu_item_button(
                                ui,
                                if pt {
                                    "Recarregar transcri\u{00e7}\u{00e3}o"
                                } else {
                                    "Reload transcript"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                reload_transcript = true;
                                ui.close();
                            }
                        });
                    });
                });

                ui.add_space(10.0);

                // Rounded search pill matching YouTube's "Pesquisar transcrição".
                let has_query = !self.watch.extras.transcript_query.is_empty();
                let trimmed_query = self.watch.extras.transcript_query.trim().to_owned();
                let search_cache_id = egui::Id::new("watch_transcript_search_display_cache");
                let search_key = TranscriptSearchCacheKey {
                    video_url: self.selected.as_ref().map(|v| v.url.clone()),
                    transcript_revision: self.watch.extras.transcript_revision,
                    cue_count: self.watch.extras.transcript_display_cache.segments.len(),
                    filter_query: trimmed_query.clone(),
                    language_track_url: self.watch.extras.transcript_track_url.clone(),
                };
                let existing_search =
                    ctx.data(|d| d.get_temp::<Arc<CachedTranscriptSearch>>(search_cache_id));
                let mut search_cache = resolve_transcript_search_cache(
                    existing_search.as_ref(),
                    search_key,
                    &self.watch.extras.transcript_display_cache.segments,
                );
                if existing_search
                    .as_ref()
                    .is_none_or(|prev| !Arc::ptr_eq(prev, &search_cache))
                {
                    ctx.data_mut(|d| d.insert_temp(search_cache_id, Arc::clone(&search_cache)));
                }
                let match_count = if trimmed_query.is_empty() {
                    None
                } else {
                    Some(search_cache.matched_indices.len())
                };
                egui::Frame::new()
                    .fill(search_bg)
                    .corner_radius(18)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        let search_inner_w = (inner_width - 28.0).max(100.0);
                        ui.set_width(search_inner_w);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let suffix_w = if has_query { 56.0 } else { 0.0 };
                            let input_w = (search_inner_w - suffix_w).max(60.0);
                            ui.add_sized(
                                [input_w, 20.0],
                                egui::TextEdit::singleline(&mut self.watch.extras.transcript_query)
                                    .id_salt((scroll_id, "transcript_search"))
                                    .frame(egui::Frame::NONE)
                                    .font(egui::FontId::proportional(13.5))
                                    .text_color(palette.channel_fg)
                                    .hint_text(
                                        RichText::new(if pt {
                                            "Pesquisar transcri\u{00e7}\u{00e3}o"
                                        } else {
                                            "Search transcript"
                                        })
                                        .color(muted_fg),
                                    ),
                            );
                            if has_query {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("\u{2715}")
                                                        .size(12.0)
                                                        .color(muted_fg),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.watch.extras.transcript_query.clear();
                                        }
                                        if let Some(count) = match_count {
                                            ui.label(
                                                RichText::new(count.to_string())
                                                    .size(11.5)
                                                    .color(muted_fg),
                                            );
                                        }
                                    },
                                );
                            }
                        });
                    });
                // Re-resolve immediately when the search box edited or cleared the query this frame
                // so the cue list never renders a 1-frame stale search result.
                let updated_trimmed_query = self.watch.extras.transcript_query.trim();
                if updated_trimmed_query != trimmed_query {
                    let updated_key = TranscriptSearchCacheKey {
                        video_url: self.selected.as_ref().map(|v| v.url.clone()),
                        transcript_revision: self.watch.extras.transcript_revision,
                        cue_count: self.watch.extras.transcript_display_cache.segments.len(),
                        filter_query: updated_trimmed_query.to_owned(),
                        language_track_url: self.watch.extras.transcript_track_url.clone(),
                    };
                    search_cache = resolve_transcript_search_cache(
                        Some(&search_cache),
                        updated_key,
                        &self.watch.extras.transcript_display_cache.segments,
                    );
                    ctx.data_mut(|d| d.insert_temp(search_cache_id, Arc::clone(&search_cache)));
                }

                if let Some((ok, status)) = &self.watch.extras.transcript_export_status {
                    ui.add_space(6.0);
                    let color = if *ok {
                        palette.notice_fg
                    } else {
                        palette.warning_fg
                    };
                    ui.add(egui::Label::new(RichText::new(status).size(12.0).color(color)).wrap());
                }

                ui.add_space(10.0);

                let waiting_for_subtitles = !self.watch.extras.subtitles_loaded
                    && (self.watch.extras.subtitles_loading.is_some()
                        || self.is_playback_metadata_loading()
                        || self.watch.live.watch_info_loading);
                let is_loading = self.watch.extras.transcript_loading || waiting_for_subtitles;
                let active_error = self.watch.extras.transcript_error.as_deref().or_else(|| {
                    self.watch
                        .extras
                        .subtitles
                        .is_empty()
                        .then_some(self.watch.extras.subtitles_error.as_deref())
                        .flatten()
                });

                if is_loading {
                    skeleton::transcript_skeleton(ui, inner_width, dark_theme);
                } else if let Some(error) = active_error {
                    ui.add(
                        egui::Label::new(
                            RichText::new(error).size(13.0).color(palette.warning_fg),
                        )
                        .wrap(),
                    );
                    ui.add_space(8.0);
                    if styled_compact_button(
                        ui,
                        if pt {
                            "Tentar novamente"
                        } else {
                            "Try again"
                        },
                        false,
                        &palette,
                    )
                    .clicked()
                    {
                        reload_transcript = true;
                    }
                } else if !has_segments {
                    ui.vertical_centered(|ui| {
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(if pt {
                                "Transcri\u{00e7}\u{00e3}o indispon\u{00ed}vel para este v\u{00ed}deo."
                            } else {
                                "Transcript unavailable for this video."
                            })
                            .size(13.5)
                            .color(muted_fg),
                        );
                        ui.add_space(8.0);
                        if styled_compact_button(
                            ui,
                            if pt { "Buscar novamente" } else { "Retry" },
                            false,
                            &palette,
                        )
                        .clicked()
                        {
                            reload_transcript = true;
                        }
                        ui.add_space(10.0);
                    });
                } else {
                    let query_is_empty = self.watch.extras.transcript_query.trim().is_empty();
                    let show_timestamps = self.watch.extras.transcript_show_timestamps;
                    let active_segment = self.current_player_live_pos_ms().and_then(|pos| {
                        transcript::active_transcript_segment_index(
                            &self.watch.extras.transcript_display_cache.segments,
                            pos,
                        )
                    });
                    let should_scroll_to_active = query_is_empty
                        && self.watch.extras.transcript_follow
                        && !self.watch.extras.transcript_user_scrolled
                        && active_segment.is_some()
                        && active_segment != self.watch.extras.transcript_last_scrolled_segment;

                    if query_is_empty
                        && self.watch.extras.transcript_follow
                        && self.watch.extras.transcript_user_scrolled
                        && active_segment.is_some()
                    {
                        ui.horizontal(|ui| {
                            if styled_compact_button(
                                ui,
                                if pt {
                                    "Ir para o trecho atual"
                                } else {
                                    "Jump to active segment"
                                },
                                false,
                                &palette,
                            )
                            .clicked()
                            {
                                jump_to_active = true;
                            }
                        });
                        ui.add_space(6.0);
                    }

                    let mut cache =
                        std::mem::take(&mut self.watch.extras.transcript_display_cache);
                    let list_width = (inner_width - 8.0).max(140.0);
                    let width_key = list_width.round() as i32;
                    let mut did_auto_scroll = false;

                    let scroll_out = egui::ScrollArea::vertical()
                        .id_salt(format!("{scroll_id}_transcript_scroll"))
                        .max_height(460.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.set_width(list_width);
                            ui.set_max_width(list_width);
                            if !query_is_empty {
                                let matched = &search_cache.matched_indices;
                                if matched.is_empty() {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(16.0);
                                        ui.label(
                                            RichText::new(if pt {
                                                "Nenhum resultado encontrado."
                                            } else {
                                                "No results found."
                                            })
                                            .size(13.5)
                                            .color(muted_fg),
                                        );
                                        ui.add_space(16.0);
                                    });
                                } else {
                                    let visible_limit = matched.len().min(300);
                                    let search_heights: Vec<f32> = (0..visible_limit)
                                        .map(|pos| {
                                            let seg_idx = matched[pos];
                                            estimate_transcript_segment_height(
                                                &cache.segments[seg_idx],
                                                list_width,
                                                show_timestamps,
                                                font_size,
                                            )
                                        })
                                        .collect();
                                    let row_gap = ui.spacing().item_spacing.y;
                                    let list_top = ui.cursor().top();
                                    let clip = ui.clip_rect();
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    let (visible_range, top_spacer_h, bottom_spacer_h) =
                                        compute_variable_visible_range(
                                            &search_heights,
                                            row_gap,
                                            list_top,
                                            clip.top(),
                                            clip.bottom(),
                                            4,
                                        );
                                    if top_spacer_h > 0.0 {
                                        ui.allocate_exact_size(
                                            egui::vec2(list_width, top_spacer_h),
                                            egui::Sense::hover(),
                                        );
                                    }
                                    let rendered_end = visible_range.end;
                                    for pos in visible_range {
                                        let seg_idx = matched[pos];
                                        let cue = &cache.segments[seg_idx];
                                        let is_active = active_segment == Some(seg_idx);
                                        let ranges = search_cache
                                            .match_ranges
                                            .get(pos)
                                            .map(Vec::as_slice);
                                        let (resp, _, _) = Self::paint_transcript_segment_row(
                                            ui,
                                            cue,
                                            list_width,
                                            is_active,
                                            show_timestamps,
                                            ranges,
                                            font_size,
                                            dark_theme,
                                        );
                                        if resp.clicked() {
                                            seek_to_ms = Some(cue.start_ms);
                                            user_clicked_segment = true;
                                        }
                                        if pos + 1 < rendered_end {
                                            ui.add_space(row_gap);
                                        }
                                    }
                                    if bottom_spacer_h > 0.0 {
                                        ui.allocate_exact_size(
                                            egui::vec2(list_width, bottom_spacer_h),
                                            egui::Sense::hover(),
                                        );
                                    }
                                }
                            } else {
                                let row_gap = ui.spacing().item_spacing.y;
                                let visible_rect = ui.clip_rect().expand2(egui::vec2(0.0, 260.0));
                                let mut pending_spacer_h = 0.0_f32;
                                let flush_spacer = |ui: &mut egui::Ui, pending: &mut f32| {
                                    if *pending > 0.0 {
                                        ui.allocate_exact_size(
                                            egui::vec2(list_width, *pending),
                                            egui::Sense::hover(),
                                        );
                                        *pending = 0.0;
                                    }
                                };
                                for (item_idx, item) in cache.items.iter().enumerate() {
                                    let is_target_active = matches!(
                                        item,
                                        transcript::TranscriptDisplayItem::Segment { segment_index }
                                            if active_segment == Some(*segment_index)
                                    );
                                    let cursor_top = ui.cursor().top() + pending_spacer_h;
                                    let row_h = cache
                                        .item_heights
                                        .get(item_idx)
                                        .and_then(|entry| *entry)
                                        .and_then(|(w_key, ts_flag, h)| {
                                            (w_key == width_key
                                                && ts_flag == show_timestamps
                                                && h > 0.0)
                                                .then_some(h)
                                        })
                                        .unwrap_or_else(|| {
                                            estimate_transcript_item_height(
                                                item,
                                                &cache.segments,
                                                list_width,
                                                show_timestamps,
                                                font_size,
                                                item_idx == 0,
                                            )
                                        });
                                    let is_offscreen = !(should_scroll_to_active && is_target_active)
                                        && (cursor_top + row_h < visible_rect.top()
                                            || cursor_top > visible_rect.bottom());
                                    if is_offscreen {
                                        if pending_spacer_h > 0.0 {
                                            pending_spacer_h += row_gap;
                                        }
                                        pending_spacer_h += row_h;
                                        continue;
                                    }
                                    flush_spacer(ui, &mut pending_spacer_h);

                                    match item {
                                        transcript::TranscriptDisplayItem::Chapter {
                                            title,
                                            start_ms,
                                        } => {
                                            let (resp, h) = Self::paint_transcript_chapter_row(
                                                ui,
                                                title,
                                                list_width,
                                                item_idx == 0,
                                                dark_theme,
                                                &palette,
                                            );
                                            if let Some(slot) = cache.item_heights.get_mut(item_idx)
                                            {
                                                *slot = Some((width_key, show_timestamps, h));
                                            }
                                            if resp.clicked() {
                                                seek_to_ms = Some(*start_ms);
                                                user_clicked_segment = true;
                                            }
                                        }
                                        transcript::TranscriptDisplayItem::Segment {
                                            segment_index,
                                        } => {
                                            let cue = &cache.segments[*segment_index];
                                            let is_active = active_segment == Some(*segment_index);
                                            let (resp, row_rect, h) =
                                                Self::paint_transcript_segment_row(
                                                    ui,
                                                    cue,
                                                    list_width,
                                                    is_active,
                                                    show_timestamps,
                                                    None,
                                                    font_size,
                                                    dark_theme,
                                                );
                                            if let Some(slot) = cache.item_heights.get_mut(item_idx)
                                            {
                                                *slot = Some((width_key, show_timestamps, h));
                                            }
                                            if should_scroll_to_active && is_active {
                                                ui.scroll_to_rect(
                                                    row_rect.expand2(egui::vec2(0.0, 24.0)),
                                                    Some(egui::Align::Center),
                                                );
                                                did_auto_scroll = true;
                                            }
                                            if resp.clicked() {
                                                seek_to_ms = Some(cue.start_ms);
                                                user_clicked_segment = true;
                                            }
                                        }
                                    }
                                }
                                flush_spacer(ui, &mut pending_spacer_h);
                            }
                        });

                    self.watch.extras.transcript_display_cache = cache;

                    if did_auto_scroll {
                        self.watch.extras.transcript_last_scrolled_segment = active_segment;
                    } else if query_is_empty {
                        let pointer_scrolled = ctx.input(|input| {
                            input.pointer.hover_pos().is_some_and(|pos| {
                                scroll_out.inner_rect.contains(pos)
                                    && input.smooth_scroll_delta.y.abs() > 0.5
                            })
                        });
                        let offset_moved = (scroll_out.state.offset.y
                            - self.watch.extras.transcript_last_scroll_offset)
                            .abs()
                            > 2.5
                            && active_segment == self.watch.extras.transcript_last_scrolled_segment;
                        if pointer_scrolled || offset_moved {
                            self.watch.extras.transcript_user_scrolled = true;
                        }
                    }
                    self.watch.extras.transcript_last_scroll_offset = scroll_out.state.offset.y;

                    if self.watch.extras.subtitles.len() > 1 {
                        ui.add_space(6.0);
                        Self::paint_hairline_divider(ui, &palette);
                        ui.add_space(6.0);
                        let current_label = self
                            .watch
                            .extras
                            .preferred_transcript_track(pt)
                            .map(|track| Self::transcript_track_display_label(track, pt))
                            .unwrap_or_else(|| {
                                if pt {
                                    "Idioma".to_owned()
                                } else {
                                    "Language".to_owned()
                                }
                            });
                        let current_url = self
                            .watch
                            .extras
                            .preferred_transcript_track(pt)
                            .map(|track| track.url.clone());
                        let lang_resp = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{current_label} \u{25be}"))
                                        .size(13.0)
                                        .strong()
                                        .color(palette.channel_fg),
                                )
                                .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        egui::Popup::menu(&lang_resp).show(|ui| {
                            ui.set_min_width(220.0);
                            for track in &self.watch.extras.subtitles {
                                let label = Self::transcript_track_display_label(track, pt);
                                let selected = current_url.as_deref() == Some(track.url.as_str());
                                if styled_menu_item_button(ui, &label, selected, &palette).clicked()
                                {
                                    switch_track = Some(track.clone());
                                    ui.close();
                                }
                            }
                        });
                    }
                }
            });

        if close_transcript {
            self.watch.extras.close_transcript();
        }
        if copy_transcript {
            let formatted =
                transcript::format_txt(&self.watch.extras.transcript_display_cache.segments);
            let _ = copy_text_to_clipboard(ctx, &formatted);
            self.watch.extras.transcript_export_status = Some((
                true,
                if pt {
                    format!(
                        "{} trecho(s) copiados para a \u{00e1}rea de transfer\u{00ea}ncia.",
                        self.watch.extras.transcript_display_cache.segments.len()
                    )
                } else {
                    format!(
                        "{} segment(s) copied to clipboard.",
                        self.watch.extras.transcript_display_cache.segments.len()
                    )
                },
            ));
        }
        if export_transcript {
            let title = self
                .selected
                .as_ref()
                .map(|v| v.title.as_str())
                .unwrap_or("Transcricao")
                .to_owned();
            let segments = self.watch.extras.transcript_display_cache.segments.clone();
            self.watch
                .extras
                .start_transcript_export(&title, &segments, ctx);
        }
        if reload_transcript {
            self.watch.extras.reload_transcript(pt, ctx);
        }
        if let Some(track) = switch_track {
            self.watch.extras.start_transcript(&track, ctx);
        }
        if jump_to_active {
            self.watch.extras.transcript_user_scrolled = false;
            self.watch.extras.transcript_last_scrolled_segment = None;
        }
        if let Some(start_ms) = seek_to_ms {
            if user_clicked_segment {
                self.watch.extras.transcript_user_scrolled = false;
            }
            self.seek_to_seconds(start_ms as f64 / 1000.0);
        }
    }
}
