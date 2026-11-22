use super::{
    empty_state_card_ui, library_variable_visible_range, paint_library_card_hover_glow,
    paint_section_hairline,
};
use crate::ui_icons::*;
use crate::*;
use eframe::egui::{self, RichText};
use std::collections::HashMap;

#[derive(Clone, Default)]
struct DownloadsHeightCache {
    width_key: i32,
    heights: HashMap<u64, f32>,
}

impl FastTube {
    pub(crate) fn downloads_section_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let palette = self.palette();
        let pt = self.ui_pt();
        let effective_dir = downloads::DownloadManager::effective_download_dir(
            self.settings.prefs.value.download_dir_override.as_deref(),
        );
        let avail_w = ui.available_width().max(320.0);
        let two_col = avail_w >= 860.0;

        ui.label(
            RichText::new("Downloads")
                .size(28.0)
                .strong()
                .color(palette.channel_fg),
        );
        ui.add_space(4.0);
        let active_running = self.downloads.manager.running_count();
        let max_conc = self.downloads.manager.effective_max_concurrent();
        ui.label(
            RichText::new(if pt {
                format!(
                    "Salvos localmente via yt-dlp · Sem necessidade de assinatura · Downloads simultâneos: {active_running}/{max_conc}"
                )
            } else {
                format!(
                    "Saved offline via yt-dlp · No subscription required · Concurrent downloads: {active_running}/{max_conc}"
                )
            })
            .size(13.0)
            .color(palette.neutral_status_fg),
        );
        ui.add_space(10.0);

        let chips = if pt {
            [
                (0_u8, "Tudo"),
                (1, "Vídeos"),
                (2, "Áudio"),
                (3, "Em andamento"),
                (4, "Concluídos"),
            ]
        } else {
            [
                (0_u8, "All"),
                (1, "Videos"),
                (2, "Audio"),
                (3, "In progress"),
                (4, "Completed"),
            ]
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (tab, lbl) in chips {
                if styled_pill_button(ui, lbl, self.downloads.filter_tab == tab, &palette).clicked()
                {
                    self.downloads.filter_tab = tab;
                }
            }
        });
        ui.add_space(10.0);
        paint_section_hairline(ui, &palette);
        ui.add_space(12.0);

        if let Some(notice) = self.downloads.notice.clone() {
            let mut dismiss = false;
            egui::Frame::new()
                .fill(palette.notice_bg)
                .stroke(egui::Stroke::new(1.0, palette.notice_stroke))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(notice).small().color(palette.notice_fg));
                        if styled_icon_chip_button(ui, true, "✕", &palette).clicked() {
                            dismiss = true;
                        }
                    });
                });
            if dismiss {
                self.downloads.notice = None;
            }
            ui.add_space(8.0);
        }

        if !two_col {
            self.downloads_direct_rail_ui(ui, ctx, &effective_dir, &palette);
            ui.add_space(14.0);
            self.downloads_list_column_ui(ui, ctx, &palette);
        } else {
            let right_w = 320.0;
            let left_w = (avail_w - right_w - 24.0).max(340.0);
            let col_h = ui.available_height().max(480.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(left_w);
                        self.downloads_list_column_ui(ui, ctx, &palette);
                    },
                );
                ui.add_space(24.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, col_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(right_w);
                        self.downloads_direct_rail_ui(ui, ctx, &effective_dir, &palette);
                    },
                );
            });
        }
    }

    fn downloads_direct_rail_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        effective_dir: &std::path::Path,
        palette: &ThemePalette,
    ) {
        let pt = self.ui_pt();
        let mut submit_download = false;

        egui::Frame::new()
            .fill(palette.card_alt_bg)
            .stroke(egui::Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(14.0)
            .inner_margin(egui::Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new(if pt {
                        "Baixar vídeo ou áudio"
                    } else {
                        "Download video or audio"
                    })
                    .size(15.5)
                    .strong()
                    .color(palette.channel_fg),
                );
                ui.add_space(8.0);
                let resp = ui.add_sized(
                    [ui.available_width(), 32.0],
                    egui::TextEdit::singleline(&mut self.downloads.direct_url_input).hint_text(
                        if pt {
                            "Cole uma URL ou ID do YouTube..."
                        } else {
                            "Paste YouTube URL or video ID..."
                        },
                    ),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit_download = true;
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for fmt in downloads::DownloadFormat::ALL {
                        let short_label = match fmt {
                            downloads::DownloadFormat::VideoMp4Best => "MP4",
                            downloads::DownloadFormat::Video1080p => "1080p",
                            downloads::DownloadFormat::Video720p => "720p",
                            downloads::DownloadFormat::Video480p => "480p",
                            downloads::DownloadFormat::AudioM4a => "M4A",
                            downloads::DownloadFormat::AudioOpus => "Opus",
                            downloads::DownloadFormat::AudioMp3 => "MP3",
                        };
                        if styled_pill_button(
                            ui,
                            short_label,
                            self.downloads.selected_format == fmt,
                            palette,
                        )
                        .clicked()
                        {
                            self.downloads.selected_format = fmt;
                        }
                    }
                });
                ui.add_space(10.0);
                if styled_pill_button(
                    ui,
                    if pt { "⬇ Baixar" } else { "⬇ Download" },
                    true,
                    palette,
                )
                .clicked()
                {
                    submit_download = true;
                }
                ui.add_space(10.0);
                paint_section_hairline(ui, palette);
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        if pt {
                            "Pasta de destino"
                        } else {
                            "Destination folder"
                        },
                        effective_dir.display()
                    ))
                    .size(12.0)
                    .color(palette.neutral_status_fg),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                    if styled_compact_button(
                        ui,
                        if pt { "Abrir pasta" } else { "Open folder" },
                        false,
                        palette,
                    )
                    .on_hover_text(effective_dir.to_string_lossy().as_ref())
                    .clicked()
                    {
                        let _ = std::fs::create_dir_all(effective_dir);
                        if let Err(err) = settings::open_in_explorer(effective_dir) {
                            self.message = err.clone();
                            self.toasts.error(err);
                        }
                    }
                    let has_terminal = self
                        .downloads
                        .manager
                        .items
                        .iter()
                        .any(|item| item.status.is_terminal());
                    if has_terminal
                        && styled_ghost_button(
                            ui,
                            if pt {
                                "Limpar concluídos"
                            } else {
                                "Clear finished"
                            },
                            palette,
                        )
                        .clicked()
                    {
                        self.downloads.manager.clear_finished();
                    }
                    if self.downloads.manager.has_active()
                        && styled_danger_button(
                            ui,
                            if pt { "Cancelar todos" } else { "Cancel all" },
                            palette,
                        )
                        .clicked()
                    {
                        self.downloads.manager.cancel_all();
                    }
                });
                match crate::downloads::has_ffmpeg_cached() {
                    None => {
                        crate::downloads::trigger_ffmpeg_probe(ctx.clone());
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(if pt {
                                "Verificando suporte a FFmpeg em segundo plano..."
                            } else {
                                "Checking FFmpeg support in background..."
                            })
                            .size(11.5)
                            .color(palette.neutral_status_fg),
                        );
                    }
                    Some(false) => {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(if pt {
                                "Modo direto ativo (sem dependência de FFmpeg ou assinatura)"
                            } else {
                                "Direct stream mode active (no FFmpeg or subscription needed)"
                            })
                            .size(11.5)
                            .color(palette.neutral_status_fg),
                        );
                    }
                    Some(true) => {}
                }
            });

        if submit_download {
            let raw = self.downloads.direct_url_input.trim().to_owned();
            if let Some(video) = parse_direct_youtube_input(&raw) {
                self.downloads.direct_url_input.clear();
                self.downloads.notice = None;
                self.start_download_video(video, self.downloads.selected_format, ctx);
            } else {
                self.downloads.notice = Some(
                    if pt {
                        "Informe um link ou ID válido de vídeo do YouTube."
                    } else {
                        "Enter a valid YouTube video URL or ID."
                    }
                    .to_owned(),
                );
            }
        }
    }

    fn downloads_list_column_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: &ThemePalette,
    ) {
        let pt = self.ui_pt();
        let tab = self.downloads.filter_tab;
        let filtered_indices: Vec<usize> = self
            .downloads
            .manager
            .items
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, item)| match tab {
                1 => !download_format_is_audio(item.format),
                2 => download_format_is_audio(item.format),
                3 => matches!(
                    item.status,
                    downloads::DownloadStatus::Queued
                        | downloads::DownloadStatus::Running { .. }
                        | downloads::DownloadStatus::Paused
                ),
                4 => matches!(item.status, downloads::DownloadStatus::Completed { .. }),
                _ => true,
            })
            .map(|(idx, _)| idx)
            .collect();

        if self.downloads.manager.items.is_empty() {
            empty_state_card_ui(
                ui,
                if pt {
                    "Nenhum download na fila"
                } else {
                    "No downloads yet"
                },
                if pt {
                    "Cole um link no painel ao lado ou use o menu '...' de qualquer vídeo para baixar MP4 ou áudio."
                } else {
                    "Paste a link on the right rail or use the '...' menu on any video card to download MP4 or audio."
                },
                palette,
            );
            return;
        }

        if filtered_indices.is_empty() {
            empty_state_card_ui(
                ui,
                if pt {
                    "Nenhum download neste filtro"
                } else {
                    "No downloads match this filter"
                },
                if pt {
                    "Altere os chips acima ou selecione 'Tudo'."
                } else {
                    "Change the filter chips above or select 'All'."
                },
                palette,
            );
            return;
        }

        let mut cancel_id: Option<u64> = None;
        let mut pause_id: Option<u64> = None;
        let mut resume_id: Option<u64> = None;
        let mut retry_id: Option<u64> = None;
        let mut remove_id: Option<u64> = None;
        let mut open_path: Option<std::path::PathBuf> = None;
        let mut play_offline_req: Option<(std::path::PathBuf, String, String)> = None;
        let thumb_w: f32 = if ui.available_width() >= 640.0 {
            196.0
        } else {
            144.0
        };
        let thumb_h = (thumb_w * 9.0 / 16.0).round();

        let items = std::mem::take(&mut self.downloads.manager.items);
        self.thumbnails.begin_pass();
        egui::ScrollArea::vertical()
            .id_salt("browse_downloads_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail_w = ui.available_width().max(240.0);
                let width_key = avail_w.round() as i32;
                let est_row_h = thumb_h.max(106.0);
                let height_cache_id = egui::Id::new("browse_downloads_height_cache");
                let mut height_cache = ctx
                    .data(|d| d.get_temp::<DownloadsHeightCache>(height_cache_id))
                    .unwrap_or_default();
                let mut cache_dirty = false;
                if height_cache.width_key != width_key || height_cache.heights.len() > 200 {
                    height_cache.width_key = width_key;
                    height_cache.heights.clear();
                    cache_dirty = true;
                }

                let row_heights: Vec<f32> = filtered_indices
                    .iter()
                    .map(|&idx| {
                        let id = items[idx].id;
                        height_cache.heights.get(&id).copied().unwrap_or(est_row_h)
                    })
                    .collect();

                let row_gap = 12.0;
                let inner_spacing_y = ui.spacing().item_spacing.y;
                ui.spacing_mut().item_spacing.y = 0.0;
                let list_top = ui.cursor().top();
                let clip = ui.clip_rect();
                let (visible_range, top_spacer_h, bottom_spacer_h) = library_variable_visible_range(
                    &row_heights,
                    row_gap,
                    list_top,
                    clip.top(),
                    clip.bottom(),
                    2,
                );

                if top_spacer_h > 0.0 {
                    ui.allocate_exact_size(egui::vec2(avail_w, top_spacer_h), egui::Sense::hover());
                }

                let rendered_end = visible_range.end;
                for pos in visible_range {
                    let item = &items[filtered_indices[pos]];
                    let fmt_label = if pt {
                        item.format.label_pt()
                    } else {
                        item.format.label_en()
                    };
                    let thumb_url = download_item_thumb_url(item);
                    let row_start_y = ui.cursor().top();
                    let row_resp = ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.y = inner_spacing_y;
                        ui.set_min_width(ui.available_width());
                        let _ = self.thumbnails.show_rounded(
                            ui,
                            ctx,
                            thumb_url.as_ref(),
                            egui::vec2(thumb_w, thumb_h),
                            10.0,
                        );
                        ui.add_space(14.0);
                        ui.vertical(|ui| {
                            ui.set_max_width((ui.available_width() - 150.0).max(120.0));
                            ui.label(
                                RichText::new(&item.video.title)
                                    .size(15.5)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                            ui.add_space(3.0);
                            ui.label(
                                RichText::new(format!("{} · {fmt_label}", item.video.channel))
                                    .size(12.5)
                                    .color(palette.neutral_status_fg),
                            );
                            ui.add_space(6.0);
                            match &item.status {
                                downloads::DownloadStatus::Queued => {
                                    ui.label(
                                        RichText::new(if pt {
                                            "Na fila aguardando..."
                                        } else {
                                            "Queued..."
                                        })
                                        .small()
                                        .color(palette.neutral_status_fg),
                                    );
                                }
                                downloads::DownloadStatus::Running {
                                    progress_percent,
                                    speed,
                                    eta,
                                } => {
                                    let frac = (*progress_percent / 100.0).clamp(0.0, 1.0);
                                    let mut details = vec![format!("{progress_percent:.1}%")];
                                    if let Some(spd) = speed {
                                        details.push(spd.clone());
                                    }
                                    if let Some(rem) = eta {
                                        details.push(format!("ETA {rem}"));
                                    }
                                    ui.add(
                                        egui::ProgressBar::new(frac)
                                            .desired_width(ui.available_width().min(360.0))
                                            .text(details.join(" · ")),
                                    );
                                }
                                downloads::DownloadStatus::Paused => {
                                    let pct = self
                                        .downloads
                                        .manager
                                        .paused_progress(item.id)
                                        .clamp(0.0, 100.0);
                                    let frac = (pct / 100.0).clamp(0.0, 1.0);
                                    let lbl = if pt {
                                        format!("Pausado ({pct:.1}%)")
                                    } else {
                                        format!("Paused ({pct:.1}%)")
                                    };
                                    ui.add(
                                        egui::ProgressBar::new(frac)
                                            .desired_width(ui.available_width().min(360.0))
                                            .text(lbl),
                                    );
                                }
                                downloads::DownloadStatus::Completed { output_path } => {
                                    let label = if let Some(path) = output_path {
                                        format!(
                                            "{}: {}",
                                            if pt { "Concluído" } else { "Completed" },
                                            path.file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("arquivo")
                                        )
                                    } else if pt {
                                        "Concluído".to_owned()
                                    } else {
                                        "Completed".to_owned()
                                    };
                                    ui.label(
                                        RichText::new(label)
                                            .small()
                                            .color(palette.watched_status_fg),
                                    );
                                }
                                downloads::DownloadStatus::Failed(err) => {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}: {err}",
                                            if pt { "Falhou" } else { "Failed" }
                                        ))
                                        .small()
                                        .color(palette.error_banner_fg),
                                    );
                                }
                                downloads::DownloadStatus::Cancelled => {
                                    ui.label(
                                        RichText::new(if pt { "Cancelado" } else { "Cancelled" })
                                            .small()
                                            .color(palette.neutral_status_fg),
                                    );
                                }
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            match &item.status {
                                downloads::DownloadStatus::Queued
                                | downloads::DownloadStatus::Running { .. } => {
                                    if styled_danger_button(
                                        ui,
                                        if pt { "Cancelar" } else { "Cancel" },
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        cancel_id = Some(item.id);
                                    }
                                    if styled_compact_button(
                                        ui,
                                        if pt { "⏸ Pausar" } else { "⏸ Pause" },
                                        false,
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        pause_id = Some(item.id);
                                    }
                                }
                                downloads::DownloadStatus::Paused => {
                                    if styled_icon_chip_button(ui, true, "✕", palette)
                                        .on_hover_text(if pt {
                                            "Remover da lista"
                                        } else {
                                            "Remove from list"
                                        })
                                        .clicked()
                                    {
                                        remove_id = Some(item.id);
                                    }
                                    if styled_danger_button(
                                        ui,
                                        if pt { "Cancelar" } else { "Cancel" },
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        cancel_id = Some(item.id);
                                    }
                                    if styled_compact_button(
                                        ui,
                                        if pt { "▶ Retomar" } else { "▶ Resume" },
                                        true,
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        resume_id = Some(item.id);
                                    }
                                }
                                downloads::DownloadStatus::Completed { output_path } => {
                                    if styled_icon_chip_button(ui, true, "✕", palette)
                                        .on_hover_text(if pt {
                                            "Remover da lista"
                                        } else {
                                            "Remove from list"
                                        })
                                        .clicked()
                                    {
                                        remove_id = Some(item.id);
                                    }
                                    let folder_target = output_path
                                        .as_deref()
                                        .and_then(|p| p.parent())
                                        .map(|p| p.to_path_buf())
                                        .unwrap_or_else(|| item.output_dir.clone());
                                    if styled_compact_button(
                                        ui,
                                        if pt {
                                            "Mostrar na pasta"
                                        } else {
                                            "Show in folder"
                                        },
                                        false,
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        open_path = Some(folder_target);
                                    }
                                    if let Some(path) = output_path
                                        && styled_compact_button(
                                            ui,
                                            if pt {
                                                "▶ Reproduzir offline"
                                            } else {
                                                "▶ Play offline"
                                            },
                                            true,
                                            palette,
                                        )
                                        .clicked()
                                    {
                                        play_offline_req = Some((
                                            path.clone(),
                                            item.video.title.clone(),
                                            item.video.channel.clone(),
                                        ));
                                    }
                                }
                                downloads::DownloadStatus::Failed(_)
                                | downloads::DownloadStatus::Cancelled => {
                                    if styled_icon_chip_button(ui, true, "✕", palette)
                                        .on_hover_text(if pt {
                                            "Remover da lista"
                                        } else {
                                            "Remove from list"
                                        })
                                        .clicked()
                                    {
                                        remove_id = Some(item.id);
                                    }
                                    if styled_compact_button(
                                        ui,
                                        if pt {
                                            "↻ Tentar novamente"
                                        } else {
                                            "↻ Retry"
                                        },
                                        false,
                                        palette,
                                    )
                                    .clicked()
                                    {
                                        retry_id = Some(item.id);
                                    }
                                }
                            }
                        });
                    });
                    paint_library_card_hover_glow(
                        ui,
                        row_resp.response.rect.expand2(egui::vec2(6.0, 4.0)),
                        10.0,
                        ui.make_persistent_id(("download_card_hover", item.id)),
                        palette,
                    );
                    let measured_h = (ui.cursor().top() - row_start_y).max(40.0);
                    let prev_h = height_cache.heights.get(&item.id).copied();
                    if prev_h.is_none_or(|h| (h - measured_h).abs() > 1.0) {
                        height_cache.heights.insert(item.id, measured_h);
                        cache_dirty = true;
                    }
                    if pos + 1 < rendered_end {
                        ui.add_space(row_gap);
                    }
                }

                if bottom_spacer_h > 0.0 {
                    ui.allocate_exact_size(
                        egui::vec2(avail_w, bottom_spacer_h),
                        egui::Sense::hover(),
                    );
                }

                if cache_dirty {
                    ctx.data_mut(|d| d.insert_temp(height_cache_id, height_cache));
                }
            });
        self.thumbnails.end_pass(ctx);
        self.downloads.manager.items = items;

        if let Some(id) = pause_id {
            self.downloads.manager.pause(id, ctx);
        }
        if let Some(id) = resume_id {
            self.downloads.manager.resume(id, ctx);
        }
        if let Some(id) = cancel_id {
            self.downloads.manager.cancel(id, ctx);
        }
        if let Some(id) = retry_id {
            self.downloads.manager.retry(id, ctx);
        }
        if let Some(id) = remove_id {
            self.downloads.manager.remove(id);
        }
        if let Some((path, title, channel)) = play_offline_req {
            self.play_offline_download(&path, &title, &channel, ctx);
        }
        if let Some(path) = open_path
            && let Err(err) = settings::open_in_explorer(&path)
        {
            self.message = err.clone();
            self.toasts.error(err);
        }
    }

    pub(crate) fn play_offline_download(
        &mut self,
        output_path: &std::path::Path,
        title: &str,
        channel: &str,
        _ctx: &egui::Context,
    ) {
        let pt = self.ui_pt();
        if !output_path.exists() {
            let msg = if pt {
                format!("Arquivo offline não encontrado: {}", output_path.display())
            } else {
                format!("Offline file not found: {}", output_path.display())
            };
            self.message = msg.clone();
            self.toasts.error(msg);
            return;
        }
        let local_url = output_path.to_string_lossy().into_owned();
        let offline_video = Video {
            title: title.to_owned(),
            channel: channel.to_owned(),
            url: local_url,
            thumbnail: String::new(),
            duration: String::new(),
        };
        self.playback.pending_open_video = Some(offline_video);
    }

    pub(crate) fn start_batch_download_playlist(
        &mut self,
        videos: &[Video],
        format: downloads::DownloadFormat,
        ctx: &egui::Context,
    ) {
        let pt = self.ui_pt();
        let output_dir = downloads::DownloadManager::effective_download_dir(
            self.settings.prefs.value.download_dir_override.as_deref(),
        );
        let requests = downloads::build_batch_playlist_requests(
            videos,
            format,
            &output_dir,
            downloads::DownloadOptions::default(),
        );
        let count = requests.len();
        if count == 0 {
            return;
        }
        self.downloads.manager.enqueue_batch(requests, ctx);
        let msg = if pt {
            format!("{count} vídeos da playlist adicionados à fila de downloads.")
        } else {
            format!("{count} playlist videos added to downloads queue.")
        };
        self.downloads.notice = Some(msg.clone());
        self.message = msg.clone();
        self.toasts.success(msg);
    }
}

fn download_format_is_audio(fmt: downloads::DownloadFormat) -> bool {
    matches!(
        fmt,
        downloads::DownloadFormat::AudioM4a
            | downloads::DownloadFormat::AudioOpus
            | downloads::DownloadFormat::AudioMp3
    )
}

fn download_item_thumb_url(item: &downloads::DownloadItem) -> std::borrow::Cow<'_, str> {
    if !item.video.thumbnail.trim().is_empty() {
        std::borrow::Cow::Borrowed(item.video.thumbnail.as_str())
    } else if let Some(vid) = history::youtube_video_id(&item.video.url) {
        std::borrow::Cow::Owned(format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg"))
    } else {
        std::borrow::Cow::Borrowed("")
    }
}

pub(crate) fn parse_direct_youtube_input(raw: &str) -> Option<Video> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let video_id = if history::is_valid_video_id(trimmed) {
        Some(trimmed.to_owned())
    } else {
        history::youtube_video_id(trimmed)
    }?;
    Some(Video {
        title: format!("YouTube ({video_id})"),
        channel: "YouTube".to_owned(),
        url: history::youtube_watch_url(&video_id)?,
        thumbnail: format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"),
        duration: String::new(),
    })
}
